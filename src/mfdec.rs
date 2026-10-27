//! Media Foundation H.264 デコーダ。
//! Annex-B バイト列を受け取り、NV12 を経て RGBA フレームを返す。
//! クライアント側の描画（canvasへの直接供給）用。HW MFT を優先し、
//! 無ければ MS ソフトウェアデコーダ (CLSID_MSH264DecoderMFT) を使う。

#![cfg(windows)]

use std::collections::VecDeque;

use rayon::prelude::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;
use windows::core::Interface;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8 (canvas ImageData と同一レイアウト)
    pub rgba: Vec<u8>,
}

pub struct H264Decoder {
    t: IMFTransform,
    evgen: Option<IMFMediaEventGenerator>,
    is_async: bool,
    pending: VecDeque<Vec<u8>>,
    out_sample_size: usize,
    mft_provides_samples: bool,
    out_width: u32,
    out_height: u32,
}

/// `width`/`height` は期待される映像サイズ (サーバーのconfig通知と一致させる)。
/// 失敗時 None。
pub fn new(width: u32, height: u32) -> Option<H264Decoder> {
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL).ok()?;

        // ---- HW デコーダを優先 ----
        let input_ti = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let output_ti = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let mut acts: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut nacts: u32 = 0;
        if MFTEnumEx(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input_ti),
            Some(&output_ti),
            &mut acts,
            &mut nacts,
        )
        .is_ok()
            && !acts.is_null()
        {
            let list = std::slice::from_raw_parts(acts, nacts as usize);
            for act in list.iter().flatten() {
                if let Ok(t) = act.ActivateObject::<IMFTransform>()
                    && let Some(d) = H264Decoder::configure(t, width, height)
                {
                    CoTaskMemFree(Some(acts.cast()));
                    return Some(d);
                }
            }
            CoTaskMemFree(Some(acts.cast()));
        }

        // ---- ソフトウェアデコーダ ----
        let t: IMFTransform =
            CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER).ok()?;
        H264Decoder::configure(t, width, height)
    }
}

impl H264Decoder {
    fn configure(t: IMFTransform, width: u32, height: u32) -> Option<H264Decoder> {
        unsafe {
            // 入力: H264
            let inp = MFCreateMediaType().ok()?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ok()?;
            let _ = inp.SetUINT64(&MF_MT_FRAME_SIZE, pack32(width, height));
            t.SetInputType(0, &inp, 0).ok()?;

            // 出力: NV12 を要求。駄目なら利用可能タイプの先頭を使う
            let mut set = false;
            let out = MFCreateMediaType().ok()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).ok()?;
            let _ = out.SetUINT64(&MF_MT_FRAME_SIZE, pack32(width, height));
            if t.SetOutputType(0, &out, 0).is_ok() {
                set = true;
            } else {
                for i in 0..8 {
                    let Ok(mt) = t.GetOutputAvailableType(0, i) else {
                        break;
                    };
                    if t.SetOutputType(0, &mt, 0).is_ok() {
                        set = true;
                        break;
                    }
                }
            }
            if !set {
                return None;
            }

            let info = t.GetOutputStreamInfo(0).ok()?;
            let mft_provides_samples =
                (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            let out_sample_size = info.cbSize.max(1 << 22) as usize;

            let attrs = t.GetAttributes().ok();
            let is_async = attrs
                .as_ref()
                .and_then(|a| a.GetUINT32(&MF_TRANSFORM_ASYNC).ok())
                .map(|v| v != 0)
                .unwrap_or(false);
            let evgen = if is_async {
                attrs
                    .as_ref()?
                    .SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)
                    .ok()?;
                t.cast::<IMFMediaEventGenerator>().ok()
            } else {
                None
            };

            t.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .ok()?;

            Some(H264Decoder {
                t,
                evgen,
                is_async,
                pending: VecDeque::new(),
                out_sample_size,
                mft_provides_samples,
                out_width: width,
                out_height: height,
            })
        }
    }

    /// Annex-B アクセスユニットを投入し、出てきたフレーム(RGBA)を全て返す。
    pub fn decode(&mut self, annexb: &[u8]) -> Vec<Frame> {
        let mut out = Vec::new();
        if self.is_async {
            self.pending.push_back(annexb.to_vec());
            unsafe { self.pump(&mut out, 20) };
        } else {
            unsafe { self.decode_sync(annexb, &mut out) };
        }
        out
    }

    unsafe fn decode_sync(&mut self, annexb: &[u8], out: &mut Vec<Frame>) {
        unsafe {
            let Some(sample) = self.make_input(annexb) else {
                return;
            };
            match self.t.ProcessInput(0, &sample, 0) {
                Ok(()) | Err(_) => {}
            }
            self.drain(out);
        }
    }

    unsafe fn pump(&mut self, out: &mut Vec<Frame>, budget: u32) {
        unsafe {
            let Some(evgen) = self.evgen.clone() else {
                return;
            };
            for _ in 0..budget.max(1) {
                match evgen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(ev) => match ev.GetType().map(|t| t as i32) {
                        Ok(601) => {
                            // NeedInput
                            if let Some(data) = self.pending.pop_front()
                                && let Some(s) = self.make_input(&data)
                            {
                                let _ = self.t.ProcessInput(0, &s, 0);
                            }
                        }
                        Ok(602) => self.drain(out), // HaveOutput
                        _ => {}
                    },
                    Err(_) => {
                        if self.pending.is_empty() && !out.is_empty() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
        }
    }

    fn make_input(&self, annexb: &[u8]) -> Option<IMFSample> {
        unsafe {
            let buf = MFCreateMemoryBuffer(annexb.len() as u32).ok()?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None).ok()?;
            std::ptr::copy_nonoverlapping(annexb.as_ptr(), p, annexb.len());
            let _ = buf.SetCurrentLength(annexb.len() as u32);
            buf.Unlock().ok()?;
            let s = MFCreateSample().ok()?;
            s.AddBuffer(&buf).ok()?;
            Some(s)
        }
    }

    unsafe fn drain(&mut self, out: &mut Vec<Frame>) {
        unsafe {
            loop {
                let p_sample = if self.mft_provides_samples {
                    None
                } else {
                    let s = MFCreateSample().unwrap();
                    let b = MFCreateMemoryBuffer(self.out_sample_size as u32).unwrap();
                    s.AddBuffer(&b).unwrap();
                    Some(s)
                };
                let mut arr = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: std::mem::ManuallyDrop::new(p_sample),
                    dwStatus: 0,
                    pEvents: std::mem::ManuallyDrop::new(None),
                }];
                let mut status = 0u32;
                let r = self.t.ProcessOutput(0, &mut arr, &mut status);
                match r {
                    Ok(()) => {
                        if let Some(s) = arr[0].pSample.as_ref()
                            && let Some(f) = sample_to_rgba(s, self.out_width, self.out_height)
                        {
                            out.push(f);
                        }
                    }
                    Err(e) => {
                        if e.code() == MF_E_TRANSFORM_STREAM_CHANGE {
                            self.reset_output_type();
                            continue;
                        }
                        break;
                    }
                }
            }
        }
    }

    /// 解像度変更後の出力タイプ再設定
    unsafe fn reset_output_type(&mut self) {
        unsafe {
            for i in 0..8 {
                let Ok(mt) = self.t.GetOutputAvailableType(0, i) else {
                    break;
                };
                if self.t.SetOutputType(0, &mt, 0).is_ok() {
                    let v = mt.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
                    self.out_width = (v >> 32) as u32;
                    self.out_height = v as u32;
                    return;
                }
            }
        }
    }
}

fn pack32(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

/// NV12 サンプルを RGBA8 に変換 (BT.709 limited -> full)。
fn sample_to_rgba(s: &IMFSample, w: u32, h: u32) -> Option<Frame> {
    unsafe {
        let b = s.GetBufferByIndex(0).ok()?;
        // 2Dバッファならstrideが取れる。無ければwidth=strideとみなす
        let (ptr, stride, len) = match b.cast::<IMF2DBuffer>() {
            Ok(b2) => {
                let mut p = std::ptr::null_mut();
                let mut stride = 0i32;
                b2.Lock2D(&mut p, &mut stride).ok()?;
                let len = b.GetCurrentLength().unwrap_or(0) as usize;
                (p, stride.max(0) as usize, len)
            }
            Err(_) => {
                let mut p = std::ptr::null_mut();
                let mut len = 0u32;
                b.Lock(&mut p, None, Some(&mut len)).ok()?;
                (p, w as usize, len as usize)
            }
        };
        if ptr.is_null() || len == 0 {
            let _ = b.Unlock();
            return None;
        }
        let (w, h, stride) = (w as usize, h as usize, stride);
        let y_size = stride * h;
        if len < y_size + stride * h / 2 {
            let _ = b.Unlock();
            return None;
        }
        let y_plane = std::slice::from_raw_parts(ptr, y_size);
        let uv_plane = std::slice::from_raw_parts(ptr.add(y_size), stride * h / 2);

        let mut rgba = vec![0u8; w * h * 4];
        rgba.par_chunks_mut(w * 4)
            .enumerate()
            .for_each(|(row, rrow)| {
                let yrow = &y_plane[row * stride..row * stride + w];
                let uvrow = &uv_plane[(row / 2) * stride..];
                for x in 0..w {
                    let yy = yrow[x] as i32 - 16;
                    let u = uvrow[(x / 2) * 2] as i32 - 128;
                    let v = uvrow[(x / 2) * 2 + 1] as i32 - 128;
                    let c = (yy * 76309) >> 16; // 1.164 * (Y-16)
                    let r = c + ((v * 117489) >> 16);
                    let g = c - ((u * 13975) >> 16) - ((v * 34952) >> 16);
                    let bl = c + ((u * 138438) >> 16);
                    let o = x * 4;
                    rrow[o] = r.clamp(0, 255) as u8;
                    rrow[o + 1] = g.clamp(0, 255) as u8;
                    rrow[o + 2] = bl.clamp(0, 255) as u8;
                    rrow[o + 3] = 255;
                }
            });
        let _ = b.Unlock();
        Some(Frame {
            width: w as u32,
            height: h as u32,
            rgba,
        })
    }
}

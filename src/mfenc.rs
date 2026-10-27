//! Media Foundation H.264 ハードウェアエンコーダ (NVENC/QuickSync/AMF)。
//! BGRA -> NV12 (BT.601) -> MFT -> Annex-B NAL 列を出力する。

#![cfg(windows)]

use rayon::prelude::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Variant::VARIANT;
use windows::core::Interface;

pub struct Encoded {
    /// Annex-B バイト列 (SPS/PPS はキーフレーム時に前方へ付加済み)
    pub data: Vec<u8>,
    pub keyframe: bool,
}

pub struct H264Encoder {
    t: IMFTransform,
    codec_api: Option<ICodecAPI>,
    evgen: Option<IMFMediaEventGenerator>,
    pub is_async: bool,
    pending: std::collections::VecDeque<Vec<u8>>,
    sps_pps: Vec<u8>, // Annex-B (start code付き) の SPS/PPS 連結
    out_sample_size: usize,
    mft_provides_samples: bool,
    time: i64,
    frame_duration: i64,
    width: u32,
    height: u32,
}

/// `bitrate` は bps。fps は目標値。失敗時 None。
/// ハードウェアMFT (NVENC/QuickSync/AMF) を優先し、無ければMSソフトウェアMFT。
pub fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Option<H264Encoder> {
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL).ok()?;

        // ---- ハードウェアMFTを先に試す ----
        let input_ti = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output_ti = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut acts: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut nacts: u32 = 0;
        if MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input_ti),
            Some(&output_ti),
            &mut acts,
            &mut nacts,
        )
        .is_ok()
        {
            let list = std::slice::from_raw_parts(acts, nacts as usize);
            for act in list.iter().flatten() {
                if let Ok(t) = act.ActivateObject::<IMFTransform>()
                    && let Some(e) = H264Encoder::configure(t, width, height, fps, bitrate)
                {
                    CoTaskMemFree(Some(acts.cast()));
                    return Some(e);
                }
            }
            CoTaskMemFree(Some(acts.cast()));
        }

        // ---- ソフトウェアMFTにフォールバック ----
        let t: IMFTransform =
            CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER).ok()?;
        H264Encoder::configure(t, width, height, fps, bitrate)
    }
}

impl H264Encoder {
    /// 与えられた IMFTransform に NV12->H264 を設定する。失敗時 None。
    fn configure(
        t: IMFTransform,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> Option<H264Encoder> {
        unsafe {
            // ---- 出力メディアタイプ (先に設定するのがMS H264 MFTの流儀) ----
            let out = MFCreateMediaType().ok()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ok()?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack32(width, height))
                .ok()?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack32(fps, 1)).ok()?;
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack32(1, 1))
                .ok()?;
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .ok()?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate).ok()?;
            t.SetOutputType(0, &out, 0).ok()?;

            // ---- 入力メディアタイプ: NV12 ----
            let inp = MFCreateMediaType().ok()?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok()?;
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12).ok()?;
            inp.SetUINT64(&MF_MT_FRAME_SIZE, pack32(width, height))
                .ok()?;
            inp.SetUINT64(&MF_MT_FRAME_RATE, pack32(fps, 1)).ok()?;
            inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack32(1, 1))
                .ok()?;
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
                .ok()?;
            inp.SetUINT32(&MF_MT_DEFAULT_STRIDE, width).ok()?;
            t.SetInputType(0, &inp, 0).ok()?;

            // ---- 低遅延モード (リアルタイム配信に必須) ----
            let codec_api = t.cast::<ICodecAPI>().ok();
            if let Some(api) = &codec_api {
                let r1 = api.SetValue(&CODECAPI_AVLowLatencyMode, &VARIANT::from(true));
                // GOPは短めに (参加直後のキーフレーム待ちを抑える)
                let r2 = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &VARIANT::from(fps * 2));
                // Bフレームを無効化して並べ替え遅延を潰す
                let r3 = api.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &VARIANT::from(0u32));
                if std::env::var_os("SHUT_ENC_DEBUG").is_some() {
                    eprintln!("SetValue lowLatency={r1:?} gop={r2:?} bframes={r3:?}");
                }
            }

            // ---- シーケンスヘッダ (SPS/PPS) を取得 ----
            let cur = t.GetOutputCurrentType(0).ok()?;
            let blob_len = cur.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).unwrap_or(0);
            let mut seq = vec![0u8; blob_len as usize];
            if blob_len > 0 {
                let _ = cur.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut seq, None);
            }
            let sps_pps = extract_annexb(&seq);

            // ---- 出力ストリーム情報: サンプルを自前で用意するか ----
            let info = t.GetOutputStreamInfo(0).ok()?;
            let mft_provides_samples =
                (info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32) != 0;
            let out_sample_size = info.cbSize.max(1 << 20) as usize;

            // ---- 非同期MFTかどうか: 非同期ならイベント駆動に切り替える ----
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

            let frame_duration = 10_000_000i64 / fps.max(1) as i64;
            Some(H264Encoder {
                t,
                codec_api,
                evgen,
                is_async,
                pending: std::collections::VecDeque::new(),
                sps_pps,
                out_sample_size,
                mft_provides_samples,
                time: 0,
                frame_duration,
                width,
                height,
            })
        }
    }
}

fn pack32(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

/// シーケンスヘッダblobからAnnex-B形式のSPS/PPSを取り出す。
/// blob は start-code 区切りか 4バイト長+NAL の並びのいずれか。
fn extract_annexb(blob: &[u8]) -> Vec<u8> {
    if blob
        .windows(4)
        .any(|w| w == [0, 0, 0, 1] || w[..3] == [0, 0, 1])
    {
        return blob.to_vec(); // 既にAnnex-B
    }
    // 4バイトBE長 + NAL の並びとして読む
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= blob.len() {
        let n = u32::from_be_bytes(blob[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + n > blob.len() || n == 0 {
            return Vec::new(); // 解釈失敗: キーフレームにSPS/PPSを付けられないだけなので諦める
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&blob[i..i + n]);
        i += n;
    }
    out
}

/// 出力を Annex-B に正規化。MS H264 MFT はデフォルトで Annex-B を出すので
/// 先頭が start code ならそのまま通し、AVCC (4B長+NAL) の時だけ変換する。
fn normalize_annexb(src: &[u8], out: &mut Vec<u8>) {
    if src.starts_with(&[0, 0, 0, 1]) || src.starts_with(&[0, 0, 1]) {
        out.extend_from_slice(src);
        return;
    }
    let mut i = 0usize;
    while i + 4 <= src.len() {
        let n = u32::from_be_bytes(src[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        if i + n > src.len() {
            break;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&src[i..i + n]);
        i += n;
    }
}

impl H264Encoder {
    /// 次の出力を IDR (キーフレーム) にする。新規クライアント参加時に呼ぶ。
    pub fn force_keyframe(&self) {
        unsafe {
            if let Some(api) = &self.codec_api {
                let _ = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::default());
            }
        }
    }

    /// BGRA 1フレームを投入し、出てきたアクセスユニットを全て返す。
    /// 非同期MFTでは内部キューに乗せてイベントで消化する。
    pub fn encode(&mut self, bgra: &[u8]) -> Vec<Encoded> {
        let nv12 = bgra_to_nv12(bgra, self.width as usize, self.height as usize);
        let mut out_units = Vec::new();
        if self.is_async {
            self.pending.push_back(nv12);
            unsafe {
                self.pump(&mut out_units);
            }
        } else {
            unsafe { self.encode_sync(&nv12, &mut out_units) };
        }
        out_units
    }

    /// 終了時/診断用: DRAINして残りを全て引き出す。
    #[allow(dead_code)]
    pub fn drain_all(&mut self) -> Vec<Encoded> {
        let mut out = Vec::new();
        unsafe {
            let _ = self.t.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0);
            if self.is_async {
                for _ in 0..400 {
                    let n0 = out.len();
                    self.pump_wait(&mut out, 10);
                    if self.pending.is_empty() && out.len() == n0 {
                        break;
                    }
                }
            } else {
                self.drain(&mut out);
            }
        }
        out
    }

    /// 同期モード: 直接 ProcessInput/ProcessOutput。
    unsafe fn encode_sync(&mut self, nv12: &[u8], out: &mut Vec<Encoded>) {
        unsafe {
            let sample = match self.make_input(nv12) {
                Some(s) => s,
                None => return,
            };
            let pi = self.t.ProcessInput(0, &sample, 0);
            if std::env::var_os("SHUT_ENC_DEBUG").is_some() {
                eprintln!("PI: {pi:?}");
            }
            match pi {
                Ok(()) => {}
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {}
                Err(e) if e.code() == MF_E_NOTACCEPTING => {
                    self.drain(out);
                    let _ = self.t.ProcessInput(0, &sample, 0);
                }
                Err(_) => return,
            }
            self.drain(out);
            self.time += self.frame_duration;
        }
    }

    /// 非同期MFTのイベントポンプ (イベントが来るまで最大 budget ms 待つ)
    unsafe fn pump_wait(&mut self, out: &mut Vec<Encoded>, budget: u32) {
        unsafe {
            let Some(evgen) = self.evgen.clone() else {
                return;
            };
            for _ in 0..budget.max(1) {
                match evgen.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                    Ok(ev) => {
                        match ev.GetType().map(|t| t as i32) {
                            Ok(601) => {
                                // METransformNeedInput
                                if let Some(nv) = self.pending.pop_front()
                                    && let Some(s) = self.make_input(&nv)
                                {
                                    let _ = self.t.ProcessInput(0, &s, 0);
                                }
                            }
                            Ok(602) => self.drain(out), // METransformHaveOutput
                            _ => {}
                        }
                    }
                    Err(_) => {
                        if self.pending.is_empty() && !out.is_empty() {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            }
            self.time += self.frame_duration;
        }
    }

    /// 非同期MFT: 溜まったイベントを捌き、出力がまだ無ければ最大 ~16ms 待つ
    unsafe fn pump(&mut self, out: &mut Vec<Encoded>) {
        unsafe {
            self.pump_wait(out, 16);
        }
    }

    /// 入力サンプル作成。
    fn make_input(&self, nv12: &[u8]) -> Option<IMFSample> {
        unsafe {
            let buf = MFCreateMemoryBuffer(nv12.len() as u32).ok()?;
            let mut p = std::ptr::null_mut();
            buf.Lock(&mut p, None, None).ok()?;
            std::ptr::copy_nonoverlapping(nv12.as_ptr(), p, nv12.len());
            let _ = buf.SetCurrentLength(nv12.len() as u32);
            buf.Unlock().ok()?;
            let s = MFCreateSample().ok()?;
            s.AddBuffer(&buf).ok()?;
            s.SetSampleTime(self.time).ok()?;
            s.SetSampleDuration(self.frame_duration).ok()?;
            Some(s)
        }
    }

    /// 出力を全部引き出す。
    unsafe fn drain(&mut self, out: &mut Vec<Encoded>) {
        unsafe {
            loop {
                // MFTがサンプルを供給しない構成なら自前で用意する
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
                if std::env::var_os("SHUT_ENC_DEBUG").is_some() {
                    let n = arr[0]
                        .pSample
                        .as_ref()
                        .and_then(|s| s.GetBufferCount().ok())
                        .unwrap_or(0);
                    eprintln!(
                        "PO: {r:?} dwStatus={:#x} pdwStatus={status:#x} bufs={n}",
                        arr[0].dwStatus
                    );
                }
                match r {
                    Ok(()) => {
                        if let Some(s) = arr[0].pSample.as_ref()
                            && let Some(bytes) = sample_bytes(s)
                        {
                            if std::env::var_os("SHUT_ENC_DEBUG").is_some() {
                                let head: Vec<String> =
                                    bytes.iter().take(32).map(|b| format!("{b:02x}")).collect();
                                eprintln!("  raw {}B [{}]", bytes.len(), head.join(" "));
                            }
                            let key = s.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
                            let mut data = Vec::with_capacity(bytes.len() + 16);
                            if key && !self.sps_pps.is_empty() {
                                data.extend_from_slice(&self.sps_pps);
                            }
                            normalize_annexb(&bytes, &mut data);
                            out.push(Encoded {
                                data,
                                keyframe: key,
                            });
                        }
                    }
                    Err(_) => break, // NEED_MORE_INPUT でも異常終了でも抜ける
                }
            }
        }
    }
}

/// IMFSample の全バッファを連結して返す。
fn sample_bytes(s: &IMFSample) -> Option<Vec<u8>> {
    unsafe {
        let mut out = Vec::new();
        let n = s.GetBufferCount().ok()?;
        for i in 0..n {
            let b = s.GetBufferByIndex(i).ok()?;
            let mut p = std::ptr::null_mut();
            let mut len = 0u32;
            b.Lock(&mut p, None, Some(&mut len)).ok()?;
            out.extend_from_slice(std::slice::from_raw_parts(p, len as usize));
            b.Unlock().ok()?;
        }
        Some(out)
    }
}

/// BGRA8 -> NV12 (BT.601 limited)。行単位で並列化。
pub fn bgra_to_nv12(bgra: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut y = vec![0u8; w * h];
    let mut uv = vec![0u8; w * h / 2];

    // Y プレーン: 1行ずつ並列
    y.par_chunks_mut(w).enumerate().for_each(|(row, yrow)| {
        let src = &bgra[row * w * 4..(row + 1) * w * 4];
        for (x, px) in src.as_chunks::<4>().0.iter().enumerate() {
            let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
            let v = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            yrow[x] = v.clamp(0, 255) as u8;
        }
    });

    // UV プレーン: NV12はUVインターリーブ。2x2ブロックの平均で半解像度
    let uvw = w / 2;
    uv.par_chunks_mut(w)
        .enumerate()
        .for_each(|(row_uv, uvrow)| {
            // row_uv = UV面の行index; 元画像では 2*row_uv 行目のペア
            let y0 = row_uv * 2;
            for x in 0..uvw {
                let mut su = 0i32;
                let mut sv = 0i32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let px = &bgra[((y0 + dy) * w + (x * 2 + dx)) * 4..][..4];
                        let (b, g, r) = (px[0] as i32, px[1] as i32, px[2] as i32);
                        su += (-38 * r - 74 * g + 112 * b + 128) >> 8;
                        sv += (112 * r - 94 * g - 18 * b + 128) >> 8;
                    }
                }
                uvrow[x * 2] = ((su / 4) + 128).clamp(0, 255) as u8;
                uvrow[x * 2 + 1] = ((sv / 4) + 128).clamp(0, 255) as u8;
            }
        });

    let mut nv12 = Vec::with_capacity(y.len() + uv.len());
    nv12.extend_from_slice(&y);
    nv12.extend_from_slice(&uv);
    nv12
}

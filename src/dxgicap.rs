//! DXGI Desktop Duplication による GPU 側キャプチャ。
//! GDI(screenshots)と違い、画面が変化した時だけフレームが届くので待機コストがほぼゼロ。

#![cfg(windows)]

use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

/// 1フレーム分の CPU 側画像 (BGRA)。
pub struct Frame {
    pub data: Vec<u8>, // BGRA8, row-major, stride = width*4 (隙間なし)
}

pub struct Duplicator {
    _device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    staging: ID3D11Texture2D,
    width: u32,
    height: u32,
}

/// プライマリ出力を Duplication 開始。失敗したら None (呼び出し側はGDIにフォールバック)。
pub fn new() -> Option<Duplicator> {
    unsafe {
        let mut device = None;
        let mut ctx = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )
        .ok()?;
        let device = device?;
        let ctx = ctx?;

        let dxgi_dev: IDXGIDevice = device.cast().ok()?;
        let adapter = dxgi_dev.GetAdapter().ok()?;
        let output = adapter.EnumOutputs(0).ok()?;
        let desc = output.GetDesc().ok()?;
        let output1: IDXGIOutput1 = output.cast().ok()?;
        let dup = output1.DuplicateOutput(&device).ok()?;

        let (width, height) = (
            (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32,
            (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32,
        );

        // CPU 読み出し用ステージングテクスチャ (BGRA8)
        let td = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging = None;
        device
            .CreateTexture2D(&td, None, Some(&mut staging))
            .ok()?;

        Some(Duplicator {
            _device: device,
            ctx,
            dup,
            staging: staging?,
            width,
            height,
        })
    }
}

impl Duplicator {
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 次の画面更新を最大 `timeout_ms` 待つ。変化がなければ None。
    pub fn next_frame(&mut self, timeout_ms: u32) -> Option<Frame> {
        unsafe {
            let mut res: Option<IDXGIResource> = None;
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            match self
                .dup
                .AcquireNextFrame(timeout_ms, &mut info, &mut res)
            {
                Ok(()) => {}
                // TIMEOUT = 画面変化なし
                Err(_) => return None,
            }
            let res = res?;
            let tex: ID3D11Texture2D = res.cast().ok()?;
            self.ctx.CopyResource(&self.staging, &tex);
            self.dup.ReleaseFrame().ok()?;

            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .ok()?;
            let src = mapped.pData as *const u8;
            let stride = mapped.RowPitch as usize;
            let w = self.width as usize;
            let h = self.height as usize;
            let mut data = vec![0u8; w * 4 * h];
            if stride == w * 4 {
                std::ptr::copy_nonoverlapping(src, data.as_mut_ptr(), data.len());
            } else {
                for y in 0..h {
                    std::ptr::copy_nonoverlapping(
                        src.add(y * stride),
                        data.as_mut_ptr().add(y * w * 4),
                        w * 4,
                    );
                }
            }
            self.ctx.Unmap(&self.staging, 0);
            Some(Frame {
                data,
            
            })
        }
    }

    /// 遅れを取り戻すため溜まったフレームを捨てる。
    pub fn flush(&mut self) {
        while self.next_frame(0).is_some() {}
    }
}

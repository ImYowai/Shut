//! DXGI キャプチャ + MF H.264 エンコードの配信ハブ。
//! 購読者がいる間だけ動作し、画面が変化した時だけアクセスユニットを送出する。
//! パケット形式: [1B flags(bit0=keyframe)][Annex-B H.264]

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::broadcast;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

use crate::{dxgicap, log, mfenc};

pub struct VideoHub {
    tx: broadcast::Sender<Vec<u8>>,
    want_keyframe: AtomicBool,
    dims: OnceLock<(u32, u32)>,
    failed: AtomicBool,
}

static HUB: OnceLock<Arc<VideoHub>> = OnceLock::new();

/// ハブを取得。初回呼び出しでエンコーダスレッドが起動する。
pub fn hub() -> Arc<VideoHub> {
    HUB.get_or_init(|| {
        let (tx, _) = broadcast::channel(60);
        let hub = Arc::new(VideoHub {
            tx,
            want_keyframe: AtomicBool::new(true),
            dims: OnceLock::new(),
            failed: AtomicBool::new(false),
        });
        let h = hub.clone();
        std::thread::spawn(move || encoder_thread(h));
        hub
    })
    .clone()
}

impl VideoHub {
    pub fn subscribe(&self) -> broadcast::Receiver<Vec<u8>> {
        // 既存購読者とは別に、新しい受信口を返す
        self.tx.subscribe()
    }

    /// 次のエンコードをキーフレームにするよう要求
    pub fn kick(&self) {
        self.want_keyframe.store(true, Ordering::SeqCst);
    }

    /// エンコーダの準備ができるまで待つ (px 単位の w,h)。失敗なら None。
    pub async fn dims(&self) -> Option<(u32, u32)> {
        for _ in 0..50 {
            if self.failed.load(Ordering::SeqCst) {
                return None;
            }
            if let Some(d) = self.dims.get() {
                return Some(*d);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        None
    }
}

fn encoder_thread(hub: Arc<VideoHub>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let mut dup = match dxgicap::new() {
        Some(d) => d,
        None => {
            log::line("video: DXGI duplication unavailable, video stream disabled");
            hub.failed.store(true, Ordering::SeqCst);
            return;
        }
    };
    let (w, h) = dup.size();
    let mut enc = match mfenc::new(w, h, 60, 8_000_000) {
        Some(e) => e,
        None => {
            log::line("video: H.264 MFT encoder unavailable");
            hub.failed.store(true, Ordering::SeqCst);
            return;
        }
    };
    let _ = hub.dims.set((w, h));
    log::line(&format!("video: encoder ready {w}x{h}"));

    loop {
        if hub.tx.receiver_count() == 0 {
            std::thread::sleep(Duration::from_millis(200));
            dup.flush();
            continue;
        }
        if hub.want_keyframe.swap(false, Ordering::SeqCst) {
            enc.force_keyframe();
        }
        if let Some(f) = dup.next_frame(200) {
            for u in enc.encode(&f.data) {
                let mut pkt = Vec::with_capacity(1 + u.data.len());
                pkt.push(u8::from(u.keyframe));
                pkt.extend_from_slice(&u.data);
                let _ = hub.tx.send(pkt);
            }
        }
    }
}

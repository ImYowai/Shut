//! デコード済み最新フレームの保持と、WebView への供給（カスタムプロトコル）。
//!
//! フロー: WS受信スレッド -> mfdec::decode -> publish() -> LATEST
//! JS側: fetch("shut-frame://x/f?seq=N") -> 200 [w u32][h u32][seq u64][RGBA]
//!        同じ seq なら 204 を返して putImageData を省く。
//!        まだフレームが無い間は 404。

use std::borrow::Cow;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub seq: u64,
    pub rgba: Vec<u8>,
}

static LATEST: RwLock<Option<Frame>> = RwLock::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);
static NOTIFY: std::sync::Condvar = std::sync::Condvar::new();
static NOTIFY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// デコード済みフレームを公開する。古いものは即座に捨てる（遅延しない）。
pub fn publish(w: u32, h: u32, rgba: Vec<u8>) {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    *LATEST.write().unwrap() = Some(Frame { w, h, seq, rgba });
    NOTIFY.notify_all();
}

/// 接続開始時に呼び、残ったフレームを消す。
pub fn reset() {
    *LATEST.write().unwrap() = None;
}

/// seq > `after` のフレームが出るまで最大 `ms` ミリ秒待って複製を返す。
/// タイムアウト時は None。同じフレームは二度返さない前提で使う。
pub fn wait_newer(after: u64, ms: u64) -> Option<Frame> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        {
            let g = LATEST.read().unwrap();
            if let Some(f) = &*g
                && f.seq > after
            {
                return Some(Frame {
                    w: f.w,
                    h: f.h,
                    seq: f.seq,
                    rgba: f.rgba.clone(),
                });
            }
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return None;
        }
        let g = NOTIFY_LOCK.lock().unwrap();
        let _ = NOTIFY.wait_timeout(g, deadline - now).unwrap();
    }
}

/// カスタムプロトコル "shut-frame://" のハンドラ（旧経路。
/// WebView2のfetchがカスタムスキームを拒否するため framesrv のWS経路が主系統）。
#[allow(dead_code)]
pub fn protocol(
    _id: dioxus::desktop::wry::WebViewId,
    req: dioxus::desktop::wry::http::Request<Vec<u8>>,
) -> dioxus::desktop::wry::http::Response<Cow<'static, [u8]>> {
    use dioxus::desktop::wry::http::Response;

    let seen_seq: u64 = req
        .uri()
        .query()
        .and_then(|q| q.strip_prefix("seq="))
        .and_then(|v| v.split('&').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let base = || {
        Response::builder()
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
    };

    let guard = LATEST.read().unwrap();
    match &*guard {
        None => base().status(404).body(Cow::Borrowed(&[][..])).unwrap(),
        Some(f) if f.seq <= seen_seq => base().status(204).body(Cow::Borrowed(&[][..])).unwrap(),
        Some(f) => {
            let mut body = Vec::with_capacity(16 + f.rgba.len());
            body.extend_from_slice(&f.w.to_le_bytes());
            body.extend_from_slice(&f.h.to_le_bytes());
            body.extend_from_slice(&f.seq.to_le_bytes());
            body.extend_from_slice(&f.rgba);
            base()
                .status(200)
                .header("Content-Type", "application/octet-stream")
                .body(Cow::Owned(body))
                .unwrap()
        }
    }
}

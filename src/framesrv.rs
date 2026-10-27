//! クライアント内のローカルWSフレームブローカー。
//! 127.0.0.1 のみで待ち受け、デコード済み最新フレームを購読WSへ即時pushする。
//!
//! フレーム形式: [w u32le][h u32le][seq u64le][RGBA...]
//! 古いフレームは配信前に捨てる（常に最新のみ送る）。
#![cfg(windows)]

use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU16, Ordering};

use tungstenite::Message;

use crate::framechan;

static PORT: AtomicU16 = AtomicU16::new(0);
static STARTED: OnceLock<()> = OnceLock::new();

/// サーバーを起動し、WSポート番号を返す（多重呼び出し可）。
pub fn port() -> u16 {
    STARTED.get_or_init(|| {
        std::thread::Builder::new()
            .name("framesrv".into())
            .spawn(run)
            .expect("framesrv spawn");
    });
    // bindが終わるまで待つ
    for _ in 0..200 {
        let p = PORT.load(Ordering::SeqCst);
        if p != 0 {
            return p;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    0
}

fn run() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(_) => return,
    };
    PORT.store(
        listener.local_addr().map(|a| a.port()).unwrap_or(0),
        Ordering::SeqCst,
    );
    for conn in listener.incoming() {
        let Ok(stream) = conn else { continue };
        std::thread::spawn(move || {
            let _ = stream.set_nodelay(true);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(100)));
            let Ok(mut ws) = tungstenite::accept(stream) else { return };
            let mut sent = 0u64;
            loop {
                // 新しいフレームが出るまで待機（最大100msで抜けて生存確認）
                let next = framechan::wait_newer(sent, 100);
                match next {
                    Some(f) => {
                        let mut body =
                            Vec::with_capacity(16 + f.rgba.len());
                        body.extend_from_slice(&f.w.to_le_bytes());
                        body.extend_from_slice(&f.h.to_le_bytes());
                        body.extend_from_slice(&f.seq.to_le_bytes());
                        body.extend_from_slice(&f.rgba);
                        if ws.send(Message::Binary(body.into())).is_err() {
                            return;
                        }
                        sent = f.seq;
                    }
                    None => {
                        // 読み込みで切断を検知する（pingでも良いがreadで十分）
                        match ws.read() {
                            Err(tungstenite::Error::Io(e))
                                if e.kind() == ErrorKind::WouldBlock
                                    || e.kind() == ErrorKind::TimedOut => {}
                            Err(_) | Ok(Message::Close(_)) => return,
                            _ => {}
                        }
                    }
                }
            }
        });
    }
}

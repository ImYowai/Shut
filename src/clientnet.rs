//! クライアント側の通信層。
//! - パスワード認証 -> セッショントークン取得
//! - 入力/操作イベントをワーカースレッドで直列送信
//! - H.264 WS 受信 -> mfdec デコード -> framechan へ公開

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use shut_protocol::InputEvent;

use crate::framechan;

const MAX_QUEUE: usize = 128;
const MAX_MESSAGES: usize = 24;

#[derive(Debug)]
pub enum Job {
    Input(InputEvent),
    ClipboardGet,
    ClipboardSet(String),
    Upload { name: String, bytes: Vec<u8> },
    Arm,
    Disarm,
    Shutdown { now: bool },
    Cancel,
}

/// UIスレッドへ返す共有ボックス。ステータス行とクリップボード取り込み。
#[derive(Clone)]
pub struct Shared {
    pub messages: Arc<Mutex<VecDeque<String>>>,
    pub clipboard: Arc<Mutex<Option<String>>>,
}

impl Shared {
    fn say(&self, msg: impl Into<String>) {
        let mut q = self.messages.lock().unwrap();
        if q.len() >= MAX_MESSAGES {
            q.pop_front();
        }
        q.push_back(msg.into());
    }
}

#[derive(Clone)]
pub struct Remote {
    tx: Sender<Job>,
    pub shared: Shared,
    pub endpoint: String,
    /// 映像受信スレッドのハンドル。Dropで受信停止。
    _video: Option<Arc<VideoHandle>>,
}

/// Dioxusのprops比較用。チャネルの同一性ではなく接続先で比較する。
impl PartialEq for Remote {
    fn eq(&self, other: &Self) -> bool {
        self.endpoint == other.endpoint
    }
}

impl Remote {
    pub fn send(&self, job: Job) {
        if self.tx.send(job).is_err() {
            self.shared.say("送信スレッドが停止しています");
        }
    }

    pub fn input(&self, ev: InputEvent) {
        self.send(Job::Input(ev));
    }
}

// Remote は Clone 可能 (videoはArc共有)。全参照が消えた時点で
// VideoHandle::drop が受信スレッドを止めてフレームを掃除する。

/// 接続を別スレッドで試行し、結果をチャネルで返す（UIをブロックしない）。
/// `token` が空ならパスワード認証で取得する。
pub fn connect(base: String, token: String, password: String) -> Receiver<Result<Remote, String>> {
    let (done_tx, done_rx) = channel();
    std::thread::spawn(move || {
        let result = handshake(&base, &token, &password);
        let _ = done_tx.send(result);
    });
    done_rx
}

/// パスワード -> セッショントークン
fn authenticate(base: &str, password: &str) -> Result<String, String> {
    let resp = ureq::post(&format!("{base}/api/auth"))
        .send_json(serde_json::json!({"password": password}))
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let msg = resp
            .into_body()
            .read_json::<serde_json::Value>()
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or_else(|| "認証に失敗しました".into());
        return Err(msg);
    }
    resp.into_body()
        .read_json::<serde_json::Value>()
        .ok()
        .and_then(|v| v.get("token").and_then(|t| t.as_str()).map(String::from))
        .ok_or_else(|| "認証応答を読み取れませんでした".into())
}

fn handshake(base: &str, token: &str, password: &str) -> Result<Remote, String> {
    let base = base.trim_end_matches('/');
    let token = if token.is_empty() {
        authenticate(base, password)?
    } else {
        token.to_string()
    };
    let resp = ureq::get(&format!("{base}/api/state"))
        .header("x-shut-token", &token)
        .call()
        .map_err(|e| format!("接続できませんでした: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("接続を拒否されました（HTTP {}）", resp.status()));
    }
    let (tx, rx) = channel::<Job>();
    let shared = Shared {
        messages: Arc::new(Mutex::new(VecDeque::new())),
        clipboard: Arc::new(Mutex::new(None)),
    };
    let worker_shared = shared.clone();
    let worker_base = base.to_string();
    let worker_token = token.clone();
    std::thread::spawn(move || worker(rx, worker_base, worker_token, worker_shared));
    let ws_base = base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let video_ws_url = format!("{ws_base}/ws/video?t={token}");
    let video = spawn_video(&video_ws_url, shared.clone()).map(Arc::new);
    Ok(Remote {
        tx,
        shared,
        endpoint: base.to_string(),
        _video: video,
    })
}

// ---------- 映像受信スレッド ----------

struct VideoHandle {
    stop: Arc<AtomicBool>,
    th: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for VideoHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(th) = self.th.lock().unwrap().take() {
            let _ = th.join();
        }
        framechan::reset();
    }
}

/// H.264 WS を受信してデコードし、最新フレームを framechan に公開する。
fn spawn_video(ws_url: &str, shared: Shared) -> Option<VideoHandle> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let url = ws_url.to_string();
    let th = std::thread::spawn(move || video_thread(url, stop2, shared));
    Some(VideoHandle {
        stop,
        th: Mutex::new(Some(th)),
    })
}

#[cfg(windows)]
fn video_thread(url: String, stop: Arc<AtomicBool>, shared: Shared) {
    use tungstenite::Message;

    framechan::reset();
    let Ok((mut ws, _)) = tungstenite::connect(&url) else {
        shared.say("映像チャネルに接続できませんでした");
        return;
    };
    // 停止検知のため読み取りにタイムアウトを設定
    if let tungstenite::stream::MaybeTlsStream::Plain(sock) = ws.get_mut() {
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(500)));
        let _ = sock.set_nodelay(true);
    }
    let mut dec: Option<crate::mfdec::H264Decoder> = None;
    let mut pkts = 0u64;
    let mut frames = 0u64;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match ws.read() {
            Ok(Message::Binary(pkt)) => {
                if pkt.len() < 2 {
                    continue;
                }
                pkts += 1;
                let Some(d) = dec.as_mut() else {
                    continue;
                };
                for f in d.decode(&pkt[1..]) {
                    frames += 1;
                    if frames <= 3 || frames.is_multiple_of(120) {
                        shared.say(format!(
                            "decoded frame #{frames} {}x{} (pkts {pkts})",
                            f.width, f.height
                        ));
                    }
                    framechan::publish(f.width, f.height, f.rgba);
                }
            }
            Ok(Message::Text(t)) => {
                if let Ok(m) = serde_json::from_str::<serde_json::Value>(&t) {
                    if m.get("type").and_then(|v| v.as_str()) == Some("config") {
                        let w = m.get("w").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        let h = m.get("h").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        shared.say(format!("config受信 {w}x{h}"));
                        dec = crate::mfdec::new(w, h);
                        if dec.is_none() {
                            shared.say("H.264デコーダを初期化できませんでした");
                        }
                    } else if let Some(e) = m.get("error").and_then(|v| v.as_str()) {
                        shared.say(format!("映像エラー: {e}"));
                    }
                }
            }
            Err(tungstenite::Error::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    let _ = ws.close(None);
}

#[cfg(not(windows))]
fn video_thread(_url: String, _stop: Arc<AtomicBool>, shared: Shared) {
    shared.say("このOSではH.264デコードは未対応です");
}

// ---------- 操作イベントワーカー ----------

fn worker(rx: Receiver<Job>, base: String, token: String, shared: Shared) {
    'outer: while let Ok(first) = rx.recv() {
        // 追いついている間に溜まった分をまとめて取り、連続する move は最新だけ残す
        let mut jobs = Vec::with_capacity(16);
        jobs.push(first);
        while jobs.len() < MAX_QUEUE {
            match rx.try_recv() {
                Ok(job) => jobs.push(job),
                Err(_) => break,
            }
        }
        let mut pending_move: Option<InputEvent> = None;
        for job in jobs {
            if let Job::Input(ev) = &job
                && ev.kind == "move"
            {
                pending_move = Some(ev.clone());
                continue;
            }
            if let Some(mv) = pending_move.take()
                && dispatch(&Job::Input(mv), &base, &token, &shared)
            {
                break 'outer;
            }
            if dispatch(&job, &base, &token, &shared) {
                break 'outer;
            }
        }
        if let Some(mv) = pending_move.take()
            && dispatch(&Job::Input(mv), &base, &token, &shared)
        {
            break 'outer;
        }
    }
}

/// 戻り値が true ならワーカーを終了する（接続断とみなす）。
fn dispatch(job: &Job, base: &str, token: &str, shared: &Shared) -> bool {
    match job {
        Job::Input(ev) => {
            if let Err(e) = post_json(&format!("{base}/input"), token, ev) {
                shared.say(format!("入力を送れませんでした: {e}"));
            }
            false
        }
        Job::ClipboardGet => {
            match ureq::get(&format!("{base}/api/clipboard"))
                .header("x-shut-token", token)
                .call()
            {
                Ok(mut resp) => {
                    match resp
                        .body_mut()
                        .read_json::<serde_json::Value>()
                        .ok()
                        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(String::from))
                    {
                        Some(text) => *shared.clipboard.lock().unwrap() = Some(text),
                        None => shared.say("クリップボードを読み取れませんでした"),
                    }
                }
                Err(e) => shared.say(format!("クリップボードの取得に失敗: {e}")),
            }
            false
        }
        Job::ClipboardSet(text) => {
            let body = serde_json::json!({"text": text});
            match post_json(&format!("{base}/api/clipboard"), token, &body) {
                Ok(()) => shared.say("PCのクリップボードへ送りました"),
                Err(e) => shared.say(format!("クリップボード送信に失敗: {e}")),
            }
            false
        }
        Job::Upload { name, bytes } => {
            let url = format!("{base}/api/upload?name={}", urlencode(name));
            match ureq::post(&url)
                .header("x-shut-token", token)
                .content_type("application/octet-stream")
                .send(&bytes[..])
            {
                Ok(resp) if resp.status().is_success() => {
                    shared.say(format!("{name} を送信しました"))
                }
                Ok(resp) => shared.say(format!(
                    "{name} の送信を拒否されました（HTTP {}）",
                    resp.status()
                )),
                Err(e) => shared.say(format!("{name} の送信に失敗: {e}")),
            }
            false
        }
        Job::Arm => {
            shared.say(match post_empty(&format!("{base}/api/arm"), token) {
                Ok(()) => "シャットダウン監視を開始しました".to_string(),
                Err(e) => format!("監視の開始に失敗: {e}"),
            });
            false
        }
        Job::Disarm => {
            shared.say(match post_empty(&format!("{base}/api/disarm"), token) {
                Ok(()) => "監視を解除しました".to_string(),
                Err(e) => format!("監視の解除に失敗: {e}"),
            });
            false
        }
        Job::Shutdown { now } => {
            let body = serde_json::json!({"now": now});
            match post_json(&format!("{base}/api/shutdown"), token, &body) {
                Ok(()) => shared.say("シャットダウンを送信しました"),
                Err(e) => shared.say(format!("シャットダウン送信に失敗: {e}")),
            }
            false
        }
        Job::Cancel => {
            shared.say(match post_empty(&format!("{base}/api/cancel"), token) {
                Ok(()) => "シャットダウンを中止しました".to_string(),
                Err(e) => format!("中止に失敗: {e}"),
            });
            false
        }
    }
}

fn post_json(url: &str, token: &str, body: &impl Serialize) -> Result<(), String> {
    let resp = ureq::post(url)
        .header("x-shut-token", token)
        .send_json(body)
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

fn post_empty(url: &str, token: &str) -> Result<(), String> {
    let resp = ureq::post(url)
        .header("x-shut-token", token)
        .send_empty()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

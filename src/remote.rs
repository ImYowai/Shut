use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::state::{AppState, TunnelState};

// ---------- 画面キャプチャ ----------

/// モニタ一覧（JSON用）。{i, x, y, w, h, primary}
pub fn monitor_list() -> Vec<serde_json::Value> {
    screenshots::Screen::all()
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let d = &s.display_info;
            serde_json::json!({
                "i": i, "w": d.width, "h": d.height,
                "primary": d.is_primary,
            })
        })
        .collect()
}

/// 指定モニタをJPEG化して返す（範囲外はプライマリにフォールバック）
pub fn capture_jpeg(monitor: usize, max_width: u32, quality: u8) -> Option<Vec<u8>> {
    let screens = screenshots::Screen::all().ok()?;
    let screen = screens.get(monitor).or_else(|| screens.first())?;
    let img = screen.capture().ok()?; // RgbaImage
    let (w, h) = (img.width(), img.height());
    let img = if w > max_width {
        let nh = h * max_width / w;
        image::imageops::resize(&img, max_width, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality)
        .encode(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ColorType::Rgba8,
        )
        .ok()?;
    Some(buf)
}

// ---------- 入力注入 ----------

#[derive(Deserialize)]
pub struct InputEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub button: String,
    #[serde(default)]
    pub dy: f64,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub text: String,
    /// 対象モニタ（ミラー表示と同じインデックス）
    #[serde(default)]
    pub m: usize,
}

fn screen_size() -> (i32, i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// モニタの仮想画面上の矩形 (x, y, w, h)。プライマリ以外は負座標もあり得る
fn monitor_rect(m: usize) -> (i32, i32, i32, i32) {
    if let Ok(screens) = screenshots::Screen::all() {
        if let Some(s) = screens.get(m).or_else(|| screens.first()) {
            let d = &s.display_info;
            return (d.x, d.y, d.width as i32, d.height as i32);
        }
    }
    let (w, h) = screen_size();
    (0, 0, w, h)
}

fn set_cursor(m: usize, nx: f64, ny: f64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let (ox, oy, w, h) = monitor_rect(m);
    unsafe {
        SetCursorPos(
            ox + (nx.clamp(0.0, 1.0) * (w - 1) as f64) as i32,
            oy + (ny.clamp(0.0, 1.0) * (h - 1) as f64) as i32,
        );
    }
}

fn mouse(flags: u32, data: u32) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    let inputs = [input];
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

fn key(vk: u16, up: bool) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [input];
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

fn key_text(s: &str) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        KEYEVENTF_UNICODE,
    };
    let mut inputs = Vec::new();
    for unit in s.encode_utf16() {
        for up in [false, true] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
    if !inputs.is_empty() {
        unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
    }
}

pub fn handle_input(ev: &InputEvent) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN,
        MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL,
    };
    match ev.kind.as_str() {
        "move" => set_cursor(ev.m, ev.x, ev.y),
        "down" => {
            set_cursor(ev.m, ev.x, ev.y);
            mouse(if ev.button == "right" { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_LEFTDOWN }, 0);
        }
        "up" => {
            set_cursor(ev.m, ev.x, ev.y);
            mouse(if ev.button == "right" { MOUSEEVENTF_RIGHTUP } else { MOUSEEVENTF_LEFTUP }, 0);
        }
        "click" => {
            set_cursor(ev.m, ev.x, ev.y);
            let (d, u) = if ev.button == "right" {
                (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
            } else {
                (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
            };
            mouse(d, 0);
            mouse(u, 0);
        }
        "scroll" => {
            // ホイールはカーソル直下のウィンドウに届くので先に移動する
            set_cursor(ev.m, ev.x, ev.y);
            mouse(MOUSEEVENTF_WHEEL, (ev.dy * -120.0) as i32 as u32);
        }
        "key" => {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
            let vk = match ev.key.as_str() {
                "Enter" => VK_RETURN,
                "Backspace" => VK_BACK,
                "Escape" => VK_ESCAPE,
                "Tab" => VK_TAB,
                "Delete" => VK_DELETE,
                "Home" => VK_HOME,
                "End" => VK_END,
                "Up" => VK_UP,
                "Down" => VK_DOWN,
                "Left" => VK_LEFT,
                "Right" => VK_RIGHT,
                "Win" => VK_LWIN,
                "Space" => VK_SPACE,
                _ => 0,
            };
            if vk != 0 {
                key(vk, false);
                key(vk, true);
            }
        }
        "text" => key_text(&ev.text),
        _ => {}
    }
}

// ---------- Cloudflare Tunnel ----------

pub fn cloudflared_path() -> PathBuf {
    crate::data_dir().join("cloudflared.exe")
}

/// cloudflaredが無ければcurl.exeでダウンロードする
fn ensure_cloudflared() -> Result<PathBuf, String> {
    let path = cloudflared_path();
    if path.exists() {
        return Ok(path);
    }
    let url = "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-windows-amd64.exe";
    let status = Command::new("curl.exe")
        .args(["-L", "-f", "-sS", "-o"])
        .arg(&path)
        .arg(url)
        .status()
        .map_err(|e| format!("curl.exe の起動に失敗: {e}"))?;
    if !status.success() || !path.exists() {
        let _ = std::fs::remove_file(&path);
        return Err(format!("cloudflared のダウンロードに失敗（exit {:?}）", status.code()));
    }
    Ok(path)
}

pub fn start_tunnel(state: &AppState, port: u16) {
    {
        let mut st = state.lock().unwrap();
        if !matches!(st.tunnel, TunnelState::Off | TunnelState::Failed(_)) {
            return;
        }
        st.tunnel = TunnelState::Starting;
    }
    let cf = match ensure_cloudflared() {
        Ok(p) => p,
        Err(e) => {
            state.lock().unwrap().tunnel = TunnelState::Failed(e);
            return;
        }
    };
    let child = Command::new(cf)
        .args(["tunnel", "--url", &format!("http://127.0.0.1:{port}"), "--no-autoupdate"])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            state.lock().unwrap().tunnel = TunnelState::Failed(e.to_string());
            return;
        }
    };
    let stderr = child.stderr.take().map(BufReader::new);
    {
        let mut st = state.lock().unwrap();
        if matches!(st.tunnel, TunnelState::Off) {
            // Starting中に停止が押されていた：spawn済みの子だけ片付ける
            let _ = child.kill();
            return;
        }
        st.tunnel_child = Some(child);
    }
    let st2 = state.clone();
    std::thread::spawn(move || {
        // trycloudflareのURLはstderrに出る
        if let Some(err) = stderr {
            for line in err.lines() {
                let Ok(line) = line else { break };
                if let Some(pos) = line.find("https://") {
                    let tail = &line[pos..];
                    let end = tail
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(tail.len());
                    let url = tail[..end].to_string();
                    if url.contains("trycloudflare.com") {
                        let mut st = st2.lock().unwrap();
                        st.tunnel = TunnelState::Running(url);
                    }
                }
                // プロセス終了検知
                if st2.lock().unwrap().tunnel_child.is_none() {
                    return;
                }
            }
        }
        let mut st = st2.lock().unwrap();
        if st.tunnel_child.is_some() && !matches!(st.tunnel, TunnelState::Running(_)) {
            st.tunnel = TunnelState::Failed("トンネルが終了しました".into());
        }
        st.tunnel_child = None;
    });
}

pub fn stop_tunnel(state: &AppState) {
    let mut st = state.lock().unwrap();
    if let Some(mut c) = st.tunnel_child.take() {
        let _ = c.kill();
    }
    st.tunnel = TunnelState::Off;
}

/// LAN内の自分のIP
pub fn lan_ip() -> Option<String> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

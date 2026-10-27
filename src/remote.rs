use std::io::{BufRead, BufReader};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

/// 仮想タッチパッドをPCの現在位置から操作するための読み取り専用状態。
pub fn cursor_info() -> Option<serde_json::Value> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut point = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    let screens = screenshots::Screen::all().ok()?;
    screens.iter().enumerate().find_map(|(m,screen)|{
        let d=&screen.display_info;
        let x=point.x-d.x;let y=point.y-d.y;
        if x<0||y<0||x>=d.width as i32||y>=d.height as i32{return None;}
        Some(serde_json::json!({"m":m,"x":x as f64/(d.width.saturating_sub(1).max(1)) as f64,"y":y as f64/(d.height.saturating_sub(1).max(1)) as f64}))
    })
}

/// キャプチャ内容の軽量ハッシュ。フルスキャンなので小さな変化も拾う。
fn frame_hash(buf: &[u8]) -> u64 {
    let mut h = buf.len() as u64;
    for chunk in buf.as_chunks::<16>().0 {
        let (a, b) = chunk.split_at(8);
        let x = u64::from_le_bytes(a.try_into().unwrap());
        let y = u64::from_le_bytes(b.try_into().unwrap());
        h = (h ^ x).wrapping_mul(0x9e3779b97f4a7c15).rotate_left(13) ^ y;
    }
    h
}

/// 指定モニタをJPEG化して返す（範囲外はプライマリにフォールバック）。
/// 戻り値は (JPEG, 生フレームのハッシュ)。ハッシュが前回と同じなら送信を省ける。
pub fn capture_jpeg(monitor: usize, max_width: u32, quality: u8) -> Option<(Vec<u8>, u64)> {
    let screens = screenshots::Screen::all().ok()?;
    let screen = screens.get(monitor).or_else(|| screens.first())?;
    let img = screen.capture().ok()?; // RgbaImage
    let hash = frame_hash(img.as_raw());
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
    Some((buf, hash))
}

// ---------- 入力注入 ----------

pub use shut_protocol::InputEvent;

fn screen_size() -> (i32, i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// モニタの仮想画面上の矩形 (x, y, w, h)。プライマリ以外は負座標もあり得る
fn monitor_rect(m: usize) -> (i32, i32, i32, i32) {
    if let Ok(screens) = screenshots::Screen::all()
        && let Some(s) = screens.get(m).or_else(|| screens.first())
    {
        let d = &s.display_info;
        return (d.x, d.y, d.width as i32, d.height as i32);
    }
    let (w, h) = screen_size();
    (0, 0, w, h)
}

fn set_cursor(m: usize, nx: f64, ny: f64) -> Result<(), InputError> {
    use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;
    let (ox, oy, w, h) = monitor_rect(m);
    let ok = unsafe {
        SetCursorPos(
            ox + (nx.clamp(0.0, 1.0) * (w - 1) as f64) as i32,
            oy + (ny.clamp(0.0, 1.0) * (h - 1) as f64) as i32,
        )
    };
    if ok == 0 {
        Err(InputError::Injection)
    } else {
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum InputError {
    Invalid(&'static str),
    Injection,
}

impl InputError {
    pub fn message(&self) -> &str {
        match self {
            Self::Invalid(message) => message,
            Self::Injection => {
                "PCへの入力に失敗しました。操作先の権限やロック状態を確認してください。一部だけ入力された可能性があるため、画面を確認してから再送してください。"
            }
        }
    }
}

fn send_inputs(
    inputs: &[windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT],
) -> Result<(), InputError> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{INPUT, SendInput};
    if inputs.is_empty() {
        return Ok(());
    }
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InputError::Injection)
    }
}

fn mouse_input(flags: u32, data: u32) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_MOUSE, MOUSEINPUT,
    };
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse(flags: u32, data: u32) -> Result<(), InputError> {
    send_inputs(&[mouse_input(flags, data)])
}

fn keyboard_input(
    vk: u16,
    scan: u16,
    flags: u32,
) -> windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn text_inputs(s: &str) -> Vec<windows_sys::Win32::UI::Input::KeyboardAndMouse::INPUT> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_RETURN, VK_TAB,
    };
    let mut inputs = Vec::new();
    // 改行とタブはUnicode文字ではなく実際のキーとして送る。CRLFは1回だけ。
    let normalized = s.replace("\r\n", "\n").replace('\r', "\n");
    for unit in normalized.encode_utf16() {
        let (vk, scan, flags) = match unit {
            10 => (VK_RETURN, 0, 0),
            9 => (VK_TAB, 0, 0),
            _ => (0, unit, KEYEVENTF_UNICODE),
        };
        for up in [false, true] {
            inputs.push(keyboard_input(
                vk,
                scan,
                flags | if up { KEYEVENTF_KEYUP } else { 0 },
            ));
        }
    }
    inputs
}

pub fn handle_input(ev: &InputEvent) -> Result<(), InputError> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
        MOUSEEVENTF_WHEEL,
    };
    // 同時リクエストでもdown/upやテキストを途中で混在させない。
    static INPUT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = INPUT_LOCK.lock().map_err(|_| InputError::Injection)?;
    if !ev.x.is_finite() || !ev.y.is_finite() || !ev.dy.is_finite() {
        return Err(InputError::Invalid("入力座標が不正です"));
    }
    match ev.kind.as_str() {
        "move" => set_cursor(ev.m, ev.x, ev.y),
        "down" => {
            set_cursor(ev.m, ev.x, ev.y)?;
            mouse(
                if ev.button == "right" {
                    MOUSEEVENTF_RIGHTDOWN
                } else {
                    MOUSEEVENTF_LEFTDOWN
                },
                0,
            )
        }
        "up" => {
            set_cursor(ev.m, ev.x, ev.y)?;
            mouse(
                if ev.button == "right" {
                    MOUSEEVENTF_RIGHTUP
                } else {
                    MOUSEEVENTF_LEFTUP
                },
                0,
            )
        }
        "click" => {
            set_cursor(ev.m, ev.x, ev.y)?;
            let (d, u) = if ev.button == "right" {
                (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
            } else {
                (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)
            };
            let pressed = mouse(d, 0);
            let released = mouse(u, 0);
            pressed.and(released)
        }
        "scroll" => {
            // ホイールはカーソル直下のウィンドウに届くので先に移動する
            set_cursor(ev.m, ev.x, ev.y)?;
            // APIの正dyは下方向。Windowsの正ホイールは上方向なのでここで変換。
            mouse(
                MOUSEEVENTF_WHEEL,
                (ev.dy.clamp(-20.0, 20.0) * -120.0) as i32 as u32,
            )
        }
        "double_click" => {
            set_cursor(ev.m, ev.x, ev.y)?;
            // 通信遅延から独立して2クリックをまとめて注入する。
            send_inputs(&[
                mouse_input(MOUSEEVENTF_LEFTDOWN, 0),
                mouse_input(MOUSEEVENTF_LEFTUP, 0),
                mouse_input(MOUSEEVENTF_LEFTDOWN, 0),
                mouse_input(MOUSEEVENTF_LEFTUP, 0),
            ])
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
                "PageUp" => VK_PRIOR,
                "PageDown" => VK_NEXT,
                "F1" => VK_F1,
                "F2" => VK_F2,
                "F3" => VK_F3,
                "F4" => VK_F4,
                "F5" => VK_F5,
                "F6" => VK_F6,
                "F7" => VK_F7,
                "F8" => VK_F8,
                "F9" => VK_F9,
                "F10" => VK_F10,
                "F11" => VK_F11,
                "F12" => VK_F12,
                value if value.len() == 1 && value.as_bytes()[0].is_ascii_alphanumeric() => {
                    value.as_bytes()[0].to_ascii_uppercase() as u16
                }
                _ => 0,
            };
            if vk == 0 {
                return Err(InputError::Invalid("未対応のキーです"));
            }
            let modifiers: Result<Vec<_>, _> = ev
                .modifiers
                .iter()
                .map(|modifier| match modifier.as_str() {
                    "Control" => Ok(VK_CONTROL),
                    "Alt" => Ok(VK_MENU),
                    "Shift" => Ok(VK_SHIFT),
                    _ => Err(InputError::Invalid("未対応の修飾キーです")),
                })
                .collect();
            let mut modifiers = modifiers?;
            modifiers.sort_unstable();
            modifiers.dedup();
            let mut inputs: Vec<_> = modifiers
                .iter()
                .map(|&vk| keyboard_input(vk, 0, 0))
                .collect();
            inputs.extend([
                keyboard_input(vk, 0, 0),
                keyboard_input(vk, 0, KEYEVENTF_KEYUP),
            ]);
            inputs.extend(
                modifiers
                    .iter()
                    .rev()
                    .map(|&vk| keyboard_input(vk, 0, KEYEVENTF_KEYUP)),
            );
            send_inputs(&inputs)
        }
        "text" => {
            if ev.text.encode_utf16().count() > 4096 || ev.text.contains('\0') {
                return Err(InputError::Invalid(
                    "文字入力は4096 UTF-16単位以内、NULなしで送信してください",
                ));
            }
            // 長文は分割して送る。巨大な一括SendInputは途中で失敗・欠落しやすい。
            const CHUNK: usize = 256;
            let inputs = text_inputs(&ev.text);
            let n_chunks = inputs.len().div_ceil(CHUNK);
            for (i, chunk) in inputs.chunks(CHUNK).enumerate() {
                send_inputs(chunk)?;
                if i + 1 < n_chunks {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            Ok(())
        }
        _ => Err(InputError::Invalid("未対応の入力操作です")),
    }
}

// ---------- クリップボード ----------

pub fn clipboard_get() -> Result<String, String> {
    let mut cb =
        arboard::Clipboard::new().map_err(|e| format!("クリップボードを開けませんでした: {e}"))?;
    cb.get_text()
        .map_err(|e| format!("クリップボードを読めませんでした: {e}"))
}

pub fn clipboard_set(text: &str) -> Result<(), String> {
    let mut cb =
        arboard::Clipboard::new().map_err(|e| format!("クリップボードを開けませんでした: {e}"))?;
    cb.set_text(text)
        .map_err(|e| format!("クリップボードへ書き込めませんでした: {e}"))
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
        return Err(format!(
            "cloudflared のダウンロードに失敗（exit {:?}）",
            status.code()
        ));
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
        .args([
            "tunnel",
            "--url",
            &format!("http://127.0.0.1:{port}"),
            "--no-autoupdate",
        ])
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
                    let end = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
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

#[cfg(test)]
mod input_tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VK_RETURN, VK_TAB,
    };

    #[test]
    fn japanese_and_emoji_use_utf16_down_up_pairs() {
        let expected: Vec<_> = "日本語😀".encode_utf16().collect();
        let inputs = text_inputs("日本語😀");
        assert_eq!(inputs.len(), expected.len() * 2);
        for (pair, unit) in inputs.as_chunks::<2>().0.iter().zip(expected) {
            let down = unsafe { pair[0].Anonymous.ki };
            let up = unsafe { pair[1].Anonymous.ki };
            assert_eq!(
                (down.wVk, down.wScan, down.dwFlags),
                (0, unit, KEYEVENTF_UNICODE)
            );
            assert_eq!(up.dwFlags, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
        }
    }

    #[test]
    fn newlines_are_normalized_and_tabs_use_virtual_keys() {
        let inputs = text_inputs("\r\n\t\r\n");
        let keys: Vec<_> = inputs
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| unsafe { (p[0].Anonymous.ki.wVk, p[0].Anonymous.ki.dwFlags) })
            .collect();
        assert_eq!(keys, vec![(VK_RETURN, 0), (VK_TAB, 0), (VK_RETURN, 0)]);
    }

    #[test]
    fn invalid_events_fail_without_injecting() {
        let ev = |value| serde_json::from_value::<InputEvent>(value).unwrap();
        assert!(matches!(
            handle_input(&ev(serde_json::json!({"type":"unknown"}))),
            Err(InputError::Invalid(_))
        ));
        assert!(matches!(
            handle_input(&ev(serde_json::json!({"type":"key","key":"Unknown"}))),
            Err(InputError::Invalid(_))
        ));
        assert!(matches!(
            handle_input(&ev(
                serde_json::json!({"type":"text","text":"x".repeat(4097)})
            )),
            Err(InputError::Invalid(_))
        ));
    }
}

#![windows_subsystem = "windows"]

mod auth;
mod clientnet;
mod dxgicap;
mod filewatch;
mod framechan;
mod framesrv;
mod host;
mod log;
mod mfdec;
mod mfenc;
mod power;
mod remote;
mod server;
mod startup;
mod state;
mod ui;
mod video;

use std::path::PathBuf;

fn data_dir() -> PathBuf {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Shut");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 外部トリガーファイル。`shut done` が作成し、評価側が検出したら消す。
pub fn trigger_file() -> PathBuf {
    data_dir().join("trigger.flag")
}

fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("done") | Some("trigger") => {
            attach_console();
            match std::fs::write(trigger_file(), b"1") {
                Ok(()) => println!("shut: トリガーを送信しました"),
                Err(e) => eprintln!("shut: トリガーの送信に失敗しました: {e}"),
            }
        }
        Some("--set-password") => {
            attach_console();
            let pw = std::env::args().nth(2).unwrap_or_default();
            if pw.chars().count() < 6 {
                eprintln!("shut: パスワードは6文字以上にしてください");
                std::process::exit(1);
            }
            match auth::hash_password(&pw) {
                Ok(h) => {
                    let mut s = state::Settings::load();
                    s.password_hash = Some(h);
                    s.save();
                    println!("shut: 接続パスワードを設定しました");
                }
                Err(e) => {
                    eprintln!("shut: パスワードの設定に失敗しました: {e}");
                    std::process::exit(1);
                }
            }
        }
        Some("--startup") => {
            attach_console();
            match std::env::args().nth(2).as_deref() {
                Some("on") => match startup::set(true) {
                    Ok(()) => println!("shut: ログイン時の自動起動を登録しました"),
                    Err(e) => eprintln!("shut: 登録に失敗: {e}"),
                },
                Some("off") => match startup::set(false) {
                    Ok(()) => println!("shut: 自動起動を解除しました"),
                    Err(e) => eprintln!("shut: 解除に失敗: {e}"),
                },
                _ => match startup::registered() {
                    Some(cmd) => println!("shut: 登録あり: {cmd}"),
                    None => println!("shut: 自動起動は未登録です"),
                },
            }
        }
        Some("-h") | Some("--help") | Some("/?") => {
            attach_console();
            println!(
                "Shut - リモート操作とスケジュールシャットダウン\n\
                 \n\
                 使い方:\n\
                 \tshut            GUIを起動（ホスト/接続を選択）\n\
                 \tshut --host     ホストとして直接起動（スタートアップ登録用）\n\
                 \tshut --set-password <pw>  接続パスワードを設定\n\
                 \tshut --startup on|off|status  自動起動の登録/解除/確認\n\
                 \tshut done       起動中のホストに外部トリガーを送信\n\
                 \tshut --help     このヘルプ"
            );
        }
        _ => run(),
    }
}

fn run() {
    let force_host = std::env::args().any(|a| a == "--host");

    // シングルトン。検証用に SHUT_MULTI=1 で無効化可能。
    if std::env::var_os("SHUT_MULTI").is_none() && already_running() {
        attach_console();
        eprintln!("shut: 既に起動しています");
        return;
    }

    // 検証用: SHUT_WINDOW_POS="x,y[,w,h]" で初期位置・サイズを指定
    let pos: Vec<f64> = std::env::var("SHUT_WINDOW_POS")
        .ok()
        .map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect())
        .unwrap_or_default();
    let mut wb = dioxus::desktop::WindowBuilder::new()
        .with_title("Shut")
        .with_inner_size(dioxus::desktop::LogicalSize::new(980.0, 900.0));
    if pos.len() >= 2 {
        wb = wb.with_position(dioxus::desktop::LogicalPosition::new(pos[0], pos[1]));
        if pos.len() >= 4 {
            wb = wb.with_inner_size(dioxus::desktop::LogicalSize::new(pos[2], pos[3]));
        }
    }

    let boot = ui::Boot {
        force_host,
        state: state::new_state(),
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(wb)
                .with_menu(None),
        )
        .with_context(boot)
        .launch(ui::App);
}

/// 二重起動防止用のnamed mutex。起動済みならtrue。
#[cfg(windows)]
fn already_running() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\ShutSingletonMutex\0".encode_utf16().collect();
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if h.is_null() {
            return false;
        }
        // ハンドルはプロセス終了まで保持し続ける
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

#[cfg(not(windows))]
fn already_running() -> bool {
    false
}

/// windows_subsystem="windows" では標準出力が無いので、
/// CLIサブコマンド時は親プロセスのコンソールにアタッチして表示できるようにする。
#[cfg(windows)]
fn attach_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

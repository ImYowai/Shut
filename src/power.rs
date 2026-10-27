use std::io;
use std::process::Command;

/// シャットダウンを即時実行する。`force` が true のとき /f でアプリを強制終了する。
pub fn shutdown(force: bool) -> io::Result<()> {
    spawn(&args(force, 0))
}

/// カウントダウン付きでスケジュールする。タイマーはWindows側が持つため、
/// このプロセスが終了してもシャットダウンは実行される。`secs==0` は即時。
pub fn schedule(secs: u32, force: bool) -> io::Result<()> {
    spawn(&args(force, secs))
}

fn args(force: bool, secs: u32) -> Vec<String> {
    let mut a = vec!["/s".into(), "/t".into(), secs.to_string()];
    if force {
        a.push("/f".into());
    }
    a
}

fn spawn(args: &[String]) -> io::Result<()> {
    Command::new("shutdown").args(args).spawn().map(|_| ())
}

/// 保留中のシャットダウンを中止する。
pub fn abort() -> io::Result<()> {
    Command::new("shutdown").args(["/a"]).spawn().map(|_| ())
}

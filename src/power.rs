use std::io;
use std::process::Command;

/// シャットダウンを実行する。`force` が true のとき /f でアプリを強制終了する。
pub fn shutdown(force: bool) -> io::Result<()> {
    let mut cmd = Command::new("shutdown");
    cmd.args(["/s", "/t", "0"]);
    if force {
        cmd.arg("/f");
    }
    cmd.spawn().map(|_| ())
}

/// 保留中のシャットダウンを中止する（保険）。
pub fn abort() -> io::Result<()> {
    Command::new("shutdown").args(["/a"]).spawn().map(|_| ())
}

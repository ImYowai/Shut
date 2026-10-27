//! Windows スタートアップ登録 (HKCU\...\Run)。
//! `shut --host` でホストモードとして自動起動させる。

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "Shut";

/// 登録されている場合 Some(コマンドライン文字列)
pub fn registered() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .ok()?;
    key.get_value::<String, _>(VALUE_NAME).ok()
}

/// 有効化/無効化。exe_path は現在の実行ファイル。
pub fn set(enable: bool) -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let cmd = format!("\"{}\" --host", exe.display());
        key.set_value(VALUE_NAME, &cmd).map_err(|e| e.to_string())
    } else {
        let _ = key.delete_value(VALUE_NAME);
        Ok(())
    }
}

use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::{config_file, trigger_file};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TimerMode {
    AtTime,
    AfterMinutes,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub timer_enabled: bool,
    pub timer_mode: TimerMode,
    pub at_time: String,
    pub after_minutes: u32,
    pub trigger_file_enabled: bool,
    pub filewatch_enabled: bool,
    pub watch_dirs: String,
    pub file_idle_minutes: u32,
    pub force_close: bool,
    pub countdown_secs: u32,
    /// 画面ミラーリング配信を許可
    pub mirror_enabled: bool,
    /// リモート操作（マウス/キー注入）を許可
    pub control_enabled: bool,
    /// 接続パスワード (argon2id PHC文字列)。未設定のホストは認証を受理しない。
    #[serde(default)]
    pub password_hash: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            timer_enabled: false,
            timer_mode: TimerMode::AfterMinutes,
            at_time: "02:00".into(),
            after_minutes: 60,
            trigger_file_enabled: true,
            filewatch_enabled: true,
            watch_dirs: "~/.claude/projects, ~/.codex/sessions, %APPDATA%/Devin/User/acp-messages, %APPDATA%/Devin/cli".into(),
            file_idle_minutes: 3,
            force_close: false,
            countdown_secs: 60,
            mirror_enabled: true,
            control_enabled: true,
            password_hash: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(config_file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(config_file(), json);
        }
    }
}

#[derive(Clone)]
pub enum Phase {
    /// 設定中（未監視）
    Config,
    /// 監視中
    Armed { timer_deadline: Option<i64> },
    /// シャットダウンカウントダウン中
    Countdown { deadline: Instant, reason: String },
    /// 実行済み
    Done { message: String },
}

#[derive(Clone, Default)]
pub enum TunnelState {
    #[default]
    Off,
    Starting,
    Running(String),
    Failed(String),
}

/// FileWatcherのスナップショット。実体はevalスレッドが保持し、
/// ここにはロック時間を最小化するため値だけを載せる。
#[derive(Clone, Default)]
pub struct FwStatus {
    /// 監視開始以降に新規書き込みを観測したか
    pub saw_activity: bool,
    /// 最終更新からの経過秒
    pub ago_secs: Option<u64>,
}

pub struct Shared {
    pub settings: Settings,
    pub phase: Phase,
    pub fw_status: FwStatus,
    pub tunnel: TunnelState,
    pub tunnel_child: Option<Child>,
    /// ミラー視聴クライアント数
    pub mirror_clients: usize,
    /// パスワード認証の発行トークンとレート制限
    pub auth: crate::auth::AuthState,
}

pub type AppState = Arc<Mutex<Shared>>;

pub fn new_state() -> AppState {
    let settings = Settings::load();
    Arc::new(Mutex::new(Shared {
        settings,
        phase: Phase::Config,
        fw_status: FwStatus::default(),
        tunnel: TunnelState::Off,
        tunnel_child: None,
        mirror_clients: 0,
        auth: crate::auth::AuthState::default(),
    }))
}

impl Shared {
    /// 監視開始。失敗時はエラーメッセージ。
    pub fn arm(&mut self) -> Result<(), String> {
        let s = &self.settings;
        if !s.timer_enabled && !s.trigger_file_enabled && !s.filewatch_enabled {
            return Err("トリガー条件を1つ以上有効にしてください".into());
        }
        if s.filewatch_enabled && s.watch_dirs.split([',', '\n']).all(|d| d.trim().is_empty()) {
            return Err("監視するディレクトリを入力してください".into());
        }
        let timer_deadline = if s.timer_enabled {
            Some(timer_deadline(s)?)
        } else {
            None
        };
        let _ = std::fs::remove_file(trigger_file());
        self.settings.save();
        crate::log::line("armed");
        self.phase = Phase::Armed { timer_deadline };
        Ok(())
    }

    /// カウントダウン開始。タイマー本体は `shutdown /s /t N` として
    /// Windows側にスケジュールする（このプロセスが終了しても実行される）。
    pub fn fire(&mut self, reason: String) {
        crate::log::line(&format!("fire: {reason}"));
        let secs = self.settings.countdown_secs;
        match crate::power::schedule(secs, self.settings.force_close) {
            Ok(()) => {
                self.phase = Phase::Countdown {
                    deadline: Instant::now() + Duration::from_secs(secs as u64),
                    reason,
                };
            }
            Err(e) => {
                crate::log::line(&format!("schedule failed: {e}"));
                self.phase = Phase::Done {
                    message: format!("シャットダウンの予約に失敗しました: {e}"),
                };
            }
        }
        let _ = std::fs::remove_file(trigger_file());
    }

    /// 発火条件を評価。Armed時のみ呼ぶ。
    pub fn evaluate(&self) -> Option<String> {
        let s = &self.settings;
        let Phase::Armed { timer_deadline } = &self.phase else {
            return None;
        };
        if s.trigger_file_enabled && trigger_file().exists() {
            return Some("外部トリガー（shut done）".into());
        }
        if let Some(dl) = timer_deadline {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if now >= *dl {
                return Some("タイマー: 指定時刻に到達".into());
            }
        }
        if s.filewatch_enabled
            && self.fw_status.saw_activity
            && self
                .fw_status
                .ago_secs
                .is_some_and(|a| a >= s.file_idle_minutes as u64 * 60)
        {
            return Some(format!(
                "セッションログが{}分間更新されていません",
                s.file_idle_minutes
            ));
        }
        None
    }
}

/// タイマー発火時刻（unix ts）を計算
fn timer_deadline(s: &Settings) -> Result<i64, String> {
    use chrono::{Local, NaiveTime};
    match s.timer_mode {
        TimerMode::AfterMinutes => {
            if s.after_minutes == 0 {
                return Err("分数は1以上にしてください".into());
            }
            Ok((Local::now() + chrono::Duration::minutes(s.after_minutes as i64)).timestamp())
        }
        TimerMode::AtTime => {
            let t = NaiveTime::parse_from_str(s.at_time.trim(), "%H:%M")
                .map_err(|_| format!("時刻の形式が不正です: {}", s.at_time))?;
            let now = Local::now();
            let mut date = now.date_naive();
            let dt = date.and_time(t);
            if dt <= now.naive_local() {
                date = date
                    .checked_add_days(chrono::Days::new(1))
                    .ok_or("日付エラー")?;
            }
            Ok(date
                .and_time(t)
                .and_local_timezone(Local)
                .unwrap()
                .timestamp())
        }
    }
}

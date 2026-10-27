//! Shut のホスト・クライアント間で共有するプロトコル型。
//! JSON で直列化して REST/WebSocket/DataChannel のいずれでも運べる形にする。

use serde::{Deserialize, Serialize};

/// クライアント→ホストの入力イベント。
/// `kind` で内容が決まる:
/// `down`/`up`/`move`/`click`/`scroll` … x,y (0.0-1.0) + button/dy
/// `key` … key 名 + modifiers
/// `text` … text (Unicode注入)
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub modifiers: Vec<String>,
    #[serde(default)]
    pub text: String,
    /// 対象モニタ（ミラー表示と同じインデックス）
    #[serde(default)]
    pub m: usize,
}

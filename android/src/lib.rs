//! Androidクライアントの接続検証は shared protocol crate に移譲。
//! 既存の import 経路（`shut_android::{...}`）は再エクスポートで維持する。
pub use shut_protocol::{ConnectionTarget, validate_connection};

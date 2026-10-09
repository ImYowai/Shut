# Shut

AIエージェント（Claude Code / Codex / Devin など）の作業終了や、時刻指定でPCを自動シャットダウンするWindows向けユーティリティ。Rust製。

GUIはローカルHTTPサーバー + WebView2ウィンドウのWebUI（ガラスモーフィズム）。スマホからも同一UIで、画面ミラーリング・画面操作・シャットダウンがWAN経由で可能。

## 使い方

```
shut            GUIを起動（WebView2ウィンドウで開く）
shut done       起動中のGUIに外部トリガーを送信（発火）
shut --help     ヘルプ
```

## トリガー条件（いずれか成立で発火・OR条件）

| 条件 | 内容 |
|---|---|
| タイマー | 時刻指定（HH:MM、過ぎていれば翌日）または「あとN分」 |
| セッションログ監視 | 監視ディレクトリ内の最新ファイル更新がN分間止まったら発火 |
| 外部トリガー | `shut done` コマンド / リモート画面のシャットダウンボタン |

発火すると猶予時間（既定60秒）のカウントダウンが表示され、キャンセル可能。0秒で `shutdown /s /t 0` を実行。

## セッションログ監視

AIエージェントはAPI応答待ちの間CPUがほぼ0%になるため、CPU使用率では判定できません。エージェントはイベント毎にセッションログを追記するので**ファイル更新の停止**が終了の合図になります。

- 既定の監視先:
  - `~/.claude/projects` … Claude Code（セッションjsonl）
  - `~/.codex/sessions` … Codex（rollout-*.jsonl）
  - `%APPDATA%/Devin/User/acp-messages` … Devin Desktop（ACPメッセージDB）
  - `%APPDATA%/Devin/cli` … Devin（sessions.db WAL）
- `~` と `%VAR%` 形式の環境変数が使えます（カンマ区切りで複数指定）
- 監視開始以降に一度でも書き込みを観測してから更新停止が発火条件（誤発火防止）

## リモートアクセス

- **LAN**: `http://<lan-ip>:47823/?t=<token>` を表示。スマホで直接開ける
- **WAN**: 「トンネル開始」で cloudflared（初回自動DL: `%LOCALAPPDATA%\Shut\cloudflared.exe`）のQuick Tunnelを起動し `https://*.trycloudflare.com` のURLを発行。LAN/WANいずれのURLもQRコードで共有可能
- **認証**: 起動毎にランダムな128bitトークンをURL埋め込み（`?t=`）。全エンドポイントで必須
- **画面ミラー**: MJPEG（`/stream.mjpeg`）
- **画面操作**: タップ=クリック / 長押し=右クリック / ドラッグ=スクロール（マウスドラッグモード切替可）/ キーボード入力・特殊キー送信（SendInput注入）
- ミラー配信・リモート操作はそれぞれUI上でON/OFF可能

## 設定・データ

- 設定: `%LOCALAPPDATA%\Shut\config.json`（自動保存）
- トリガーファイル: `%LOCALAPPDATA%\Shut\trigger.flag`
- デバッグ用: 環境変数 `SHUT_TOKEN` でトークンを固定可能

## ビルド

```
cargo build --release
# target\release\shut.exe
```

要WebView2ランタイム（Windows 10/11に同梱されることが多い）。


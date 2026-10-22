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
- **画面ミラー**: MJPEG（`/stream.mjpeg`、60ms間隔。送信が遅い場合は自動で間隔を延長）
- **画面操作**: タップ=クリック / ダブルタップ=ダブルクリック / 長押し=右クリック / 上スワイプで下へスクロール（スマホと同じ向き、マウスドラッグモード切替可）/ 右端のスクロールバー / 右下のクイックキー（Enter・Esc・Tab・⌫）/ 仮想タッチパッド（画面下端ドッグ、画面は隠れません）/ 特殊キー送信（SendInput注入）
- **再接続**: ヘッダー横の ↻ ボタンで配信を張り直し。切断時は1.5秒後に自動リトライ、アプリが前面に戻った時も自動で再接続します
- **文字入力**: 「まとめて」送信と「直接入力」の2モード。直接入力は打った文字・Backspace・EnterをそのままPCへ転送します。入力欄がどこにもあたっていない状態でキーを押しても、画面がフォーカス中ならそのまま転送します
- **クリップボード共有**: 「PCのコピー ↓」でPCのクリップボードを読み、「PCへコピー ↑」で入力欄の文字をPCのクリップボードへ書きます（`/api/clipboard`）
- **ファイル送信**: 「ファイルを送る」で選択したファイル（256MBまで）をPCの `%LOCALAPPDATA%\Shut\incoming` へ保存します（`/api/upload`、同名は自動で連番）
- ミラー配信・リモート操作はそれぞれUI上でON/OFF可能

文字入力（まとめて）は、PC画面の入力先をタップしてから入力欄に文章を入れ、「送信」を押します。日本語の変換確定Enterでは送信せず、通常のEnterで送信、Shift + Enterで改行します。PCへの入力APIが成功した場合に入力欄をクリアします。失敗時は下書きを保持して理由を表示し、重複を避けるため自動再送しません。Windowsの権限制約によって管理者権限のアプリやロック画面へ入力できない場合があります。成功応答はWindowsが入力イベントを受理したことを示し、各アプリへの文字反映は画面でも確認してください。

「カウントダウンのテスト」は表示だけを試し、PCの電源操作を実行しません。実際のシャットダウンは専用ボタンまたは監視条件の成立で開始します。

## Android版

`android/` に独立したRust / Dioxusクライアントがあります。QR読取・URL入力・接続先確認はアプリ側、接続後の操作はWindowsサーバーが配信する既存UIをアプリ内WebViewで表示します。Windows側は上記の修正版を使ってください。

ビルド・端末要件・導入手順は [android/README.md](android/README.md) を参照してください。QRはGoogle Play開発者サービスを利用し、利用できない端末ではURLを手入力します。Android側は `ephemeral=1` で接続し、WebViewのストレージへ認証トークンを保存しません。

UIの画像案は [docs/design/android-ui-concept.png](docs/design/android-ui-concept.png)。組み込み画像生成ツールで作成したデザイン案で、実装画面そのものではありません。使用プロンプトは [docs/design/android-ui-prompt.txt](docs/design/android-ui-prompt.txt)。

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

## 検証

```powershell
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --release --locked
# Node.jsとPlaywright（Chromium）が利用可能な環境:
node --test tests/remote-ui.cjs
```

ブラウザーテストはAPIと画面を専用fixtureに置き換え、実PCへの入力注入や電源操作を実行しません（19件）。日本語IME・改行・送信失敗・通信順序・タッチ方向・ドラッグ中断・直接入力・クリップボード・ファイル送信・スクロールバー/クイックキー・トンネル状態・表示テストを確認します。スクリーンショットは `target/verification/` に出力します。


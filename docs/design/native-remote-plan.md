# Shut 再設計計画: ネイティブリモートクライアント（WebRTC基盤）

日付: 2026-10-27（改訂: WebRTC方針へ更新）
状態: 計画承認済み・Aフェーズ着手

## 方針

製品クラスのリモートデスクトップを自前構築する。RustDeskフォーク（AGPL・リレー基盤が過剰）は取らず、許諾の緩い部品を組み合わせる。

- 転送: **WebRTC**（`webrtc` crate・MIT）— ハードウェアH.264・DTLS暗号化・ICE NAT越えが規格品質で得られる
- キャプチャ: `scrap`（MIT・DXGI）または自前DXGI Desktop Duplication
- 入力: `enigo`（MIT）または既存SendInput
- UI: **Dioxus一本化**（Bevyは不適: IME/フォーム弱い。Florence系 atelier デザイン言語を適用）
- LAN: ICEでUDP直結。WAN: シグナリングはcloudflared越え＋UDP直結を試み、塞がればWebSocketフォールバック

## 構成

```
Shut/
  shut/                Windows ホスト（現行crate）
    - 管理UI: wry WebView のまま（ローカル管理面）
    - DXGIキャプチャ → H.264(MF HW) → webrtc peer
    - 入力注入 / クリップボード / ファイル / 電源・監視
  crates/protocol/     共有型（InputEvent, Frame/Ws メッセージ: serde）
  crates/client-core/  クライアント共通（接続・フレーム処理・入力送信）
  crates/client-desktop/  Dioxus `shut-remote`
  android/             Dioxus Android（RemoteActivityをネイティブ化）
```

## プロトコル段階

1. **互換MVP**: 既存 `/stream.mjpeg` + `POST /input` でクライアント先行
2. **`/ws` 双方向**: 入力・フレーム・状態・クリップボードを1接続に集約
3. **WebRTC**: 映像を H.264 MediaStream + 入力/制御を DataChannel へ

## 入力モデル

DOM入力フィールドを介さない:

- Desktop: 生 `keydown/keyup` → `{key,code,down,modifiers}` → VK/SendInput
- Android: Kotlin `dispatchKeyEvent`/`InputConnection.commitText` をフック→Rustへ直渡し（DOM経由にしないのでIME/229問題が構造ごと消える）
- まとめ送信・クリップボード: `type:'text'` Unicode注入を継続

## シャットダウン堅牢化（最優先・着手済み）

- `fire()` 時点で `shutdown /s /t N` を即スケジュール（タイマーはWindowsが保持→プロセスが死んでも実行される）
- cancel/disarmは `shutdown /a`
- `data_dir()/shut.log` に arm/fire/cancel/shutdown 結果を追記

## フェーズ

- **A**: 堅牢化＋workspace化＋protocol分離＋`/ws`
- **B**: DesktopクライアントMVP（Dioxus・atelierデザイン）
- **C**: Androidネイティブ化（WebView廃止）
- **D**: WebRTC映像＋適応品質＋ファイル/音声等の製品化

## デザイン指針

atelier-design スキル準拠: 紙の質感・水彩の滲み・物理オブジェクトのある机の雰囲気。企業的なダッシュボード感を排除。

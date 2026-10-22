# Android 検証記録

「APKが生成できた」「署名できた」「実機に入った」「起動した」「QRを読めた」「実PCを操作できた」は別の事実。混ぜずに記録する。

## 2026-10-21 自動ビルド確認（build-android.ps1 -SignForTesting）

| 項目 | 状態 | 証拠 |
|---|---|---|
| Rust/Dioxusクライアントビルド | 済 | `dx build --platform android --release` 成功 |
| Kotlin bridgeコンパイル | 済 | RemoteActivity.kt（onShowFileChooser含む）がclasses.dex内に存在を確認（dex文字列検査） |
| リリースAPK生成 | 済 | `target/dx/shut-android/release/android/app/app/build/outputs/apk/release/app-release-unsigned.apk` |
| 開発署名 | 済 | v2/v3スキーム検証OK（v1は未署名） |
| 出力APK | 済 | `android/artifacts/Shut-android-aarch64-linux-android-development.apk` |
| SHA-256 | `43FD7E48977DA32569D70CF48C30C8F3A4B7222B69250177E9F71DE97AC06EFF` | Get-FileHash |
| cargo check / clippy --all-targets | 済 | warnings なし（host / aarch64-linux-android 双方） |
| cargo test --lib --no-default-features | 済 | 4/4 PASS（URL検証契約テスト） |
| 実機/エミュレータインストール | 済 | `adb install -r` 成功（emulator-5554, AVD `dev`, arm64変換で動作） |
| アプリ起動 | 済 | Dioxus接続画面が正常描画（screencap確認） |
| URL手入力→接続 | 済 | `http://10.0.2.2:47823/?t=…` → 確認シート → RemoteActivityのWebViewでリモートUI表示・PC画面ミラー動作 |
| 前回の接続履歴・再接続 | 済 | 接続成功時にアプリ専用SharedPreferencesへ同期コミット（最大3件）。アプリ強制停止→再起動で「前回つないだPC」カード表示（トークン非表示）→確認シート経由で再接続を確認 |
| WebViewでPC操作 | 済 | 画面内タップがPCへクリックとして届くことを確認。ストリップ/クイックキー/ドック/リロードボタン描画済み |
| パネルドッグ（マウス/文字/ツール） | 済 | マウス・文字・ツール全パネルが画面を覆わずstage下端の`.sh-paneldock`にドッグし、canvasが可視のまま操作できることを確認 |
| バックグラウンド復帰 | 済 | HOME→復帰でonResume→visibilitychange→配信自動再接続を確認 |
| QRスキャン実読取 | 未 | エミュレータはGoogle Play開発者サービス未搭載でCode Scanner不可。実機で要確認 |
| ファイル選択UI | コードは同梱済・動作未確認 | dex内に実装を確認。WebView内ファイル選択の実動作は未試行 |
| LAN HTTP（cleartext） | 済 | dx生成のnetwork_security_configが127.0.0.1のみ許可だった問題を `config/network-security-config.xml` で置き換えて解消（build-android.ps1が自動パッチ） |

## 判定

- ビルド・署名・検証・インストール・起動・LAN接続・画面ミラー・タップ操作・背景復帰: **エミュレータで確認済み**。
- QR実読取・実機でのタッチ操作感・ファイル選択・HTTPS(WAN)経路: 未確認。実機手順は android/README.md「ビルド・確認」末尾の adb 手順を使う。
- 開発用署名鍵は `android/target/signing/shut-development.jks`。削除すると更新インストール不可になる。
- `dx build` 単体で作ったAPKはnetwork_security_configが未パッチ（生成物の既定が127.0.0.1のみcleartext許可）。リリース用途は必ず `build-android.ps1` 経由にする。

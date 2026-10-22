# Shut Android

PC側のShutに接続するAndroidクライアントです。QR読み取り、URL手入力、接続先確認をRust / Dioxus 0.7.10で実装し、接続後はPCが配信する既存UIをアプリ内WebViewで表示します。PCのAPIや認証方式は変更しません。

## 使い方

1. PC側のShutを起動し、LANまたはWANの接続QRを表示します。
2. Androidアプリの「QRコードを読み取る」を押します。読み取れない端末では接続URLを入力できます。
3. 表示された接続先のホスト・ポートがPC側と一致することを確認し、「確認して接続」を押します。
4. 画面ミラー・タッチ操作・文字入力（まとめて/直接入力）・クリップボード共有・ファイル送信・シャットダウンはPC側の既存画面を使います。ファイル選択はWebViewのファイル選択UIをアプリ側が受け付けます。Androidの戻る操作で接続画面を閉じます。

Android 7.0 / API 24以上。QRはGoogle Play 開発者サービスのGoogle Code Scannerを使用し、カメラ権限をアプリから要求しません。画像処理は端末内で行われます。初回はスキャナーモジュールのダウンロードにネット接続が必要です。Google Play非対応、読取エラー、キャンセルの場合もURL手入力を利用できます。

## ビルド・確認

必要環境はRust、Dioxus CLI 0.7.10、Java 17、Android SDK / NDKです。利用するターミナルに`JAVA_HOME`、`ANDROID_HOME`、`ANDROID_NDK_HOME`を設定してください。本プロジェクトはユーザー全体のSDK設定を変更しません。Dioxusとsccacheの衝突を避けるため、`.cargo/config.toml`で`rustc-wrapper = ""`を指定しています。

```powershell
cd C:\Users\yamau\Desktop\Shut\android
cargo check --all-targets
cargo test --lib --no-default-features
./build-android.ps1 -SignForTesting
# エミュレータ用
./build-android.ps1 -Target x86_64-linux-android -SignForTesting
```

`build-android.ps1`は`dx build --platform android --release`を実行します。Rust / Gradleの出力は`android/target/`に置きます。`-SignForTesting`を付けると、同ディレクトリ内の開発用鍵で署名したAPKを`android/artifacts/`に出力し、署名とSHA-256を検証します。この鍵は配布ストア用ではありません。`target/signing`を消すと次回の鍵が変わり、既存インストールへの更新ができなくなります。

```powershell
adb install -r ./artifacts/Shut-android-aarch64-linux-android-development.apk
adb shell am start -n jp.shut.remote/dev.dioxus.main.MainActivity
```

UIのWindowsプレビューは`dx serve --platform desktop`。Android以外ではQRスキャン・接続表示の代わりに案内を表示します。検査用のURL判定は`src/lib.rs`の純粋関数で、接続先への通信を起こしません。

## 接続契約 v2

- 入力は`http(s)://host[:port]/?t=<32桁hex>`。`ephemeral=1`の追加だけを許可します。
- userinfo、fragment、ルート以外のpath、重複・未知のquery、外部HTTPは拒否します。HTTPはprivate / loopback / link-localのIP、および`localhost`に限定します。HTTPSは任意ホストに対応し、接続前にホストを確認します。
- 読み取り直後の接続はしません。接続ボタンで初めてWebViewを開きます。確認画面にはトークンを表示しません。
- Rustが表示URLへ`ephemeral=1`を付加します。PC側`ui/index.html`はこの指定で`localStorage`の読み書きを行わない必要があります。古いPC版では動作を保証しません。
- Android側はWebViewのDOM storage / WebSQL / Cookie / キャッシュ保存を無効化し、URLをIntent / Bundle / ログへ出力しません。直近の接続URL（最大3件）はアプリ終了・クラッシュ後の再接続用にアプリ専用の`SharedPreferences`（`MODE_PRIVATE`）にのみ保持し、画面にはトークンを除いたエンドポイントだけを表示します（「消す」でいつでも消去可能、再接続時も同じURL検証と確認シートを通ります）。
- リモートWebViewはRust IPCや`JavascriptInterface`を持ちません。接続先origin以外の読み込み、外部ナビゲーション、`intent:` / `file:` / `content:`、新規ウィンドウを拒否し、SSLエラーは続行しません。WebViewデバッグも無効です。
- RemoteActivityは接続中の消灯を抑止（KEEP_SCREEN_ON）し、バックグラウンドではWebViewをonPause、復帰時にonResumeします。ページ側は`visibilitychange`で配信を張り直すため、裏から戻っても画面が自動復帰します。回転は`configChanges`でActivityを破棄しないためセッションが切れません。
- 認証トークンの寿命はPC側の現行仕様のままです。QRの短命化や端末別権限は今回の範囲外です。

LAN HTTPのためcleartextを許可します。ただしdx生成の`res/xml/network_security_config.xml`は既定で127.0.0.1のみ許可するため、`build-android.ps1`が`config/network-security-config.xml`（base-configでcleartext許可）に置き換えてからAPK化します。`dx build`単体の出力にはこの置き換えが入りません。実際の接続先はRustのURL境界とWebViewのorigin制限で絞ります。Android 17 / SDK 37以上へtargetを上げる場合は、LANアクセスのruntime permission対応が別途必要です。現状はtarget SDK 36です。

## 実装の境界と根拠

Google Code Scanner、Android Activity / WebViewはJavaベースのプラットフォームSDKです。そのSDKを使う最小のKotlin bridgeだけを`native/`に置き、アプリUI、接続URL検証、確認フローはRust / Dioxusで維持します。Windows版の全面移行はしていません。

- [Dioxus Mobile公式ガイド](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile/)
- [Google Code Scanner公式](https://developers.google.com/ml-kit/vision/barcode-scanning/code-scanner?hl=en)
- [Android WebView公式](https://developer.android.com/develop/ui/views/layout/webapps/webview)
- [Androidのcleartext設定](https://developer.android.com/privacy-and-security/security-config)
- [AndroidのLAN permission](https://developer.android.com/privacy-and-security/local-network-permission?hl=en)
- [Dioxus v0.7.10設定schema](https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/cli/schema.json)
- [Dioxus JNI classloaderの既知問題 #5629](https://github.com/DioxusLabs/dioxus/issues/5629)。本実装はActivityのclassloaderを使用し、Dioxus本体を改変しません。

検証結果・APK情報は`VERIFICATION.md`に記録します。APKが生成できたこと、起動画面を確認できたこと、実端末でQRを読めたこと、実PCを操作できたことはそれぞれ別の確認です。

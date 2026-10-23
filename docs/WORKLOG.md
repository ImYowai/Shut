# 2026-10-21 安定化・Android対応

契約 v2: Windows側の既存APIと起動ごとのトークンを維持する。`/input` は成功を確認して応答し、UIは操作を順に送信する。タッチの上スワイプは下方向のスクロール。日本語の変換確定は送信しない。Androidの接続画面はDioxus、QRはAndroid SDKの薄いブリッジ、接続後はIPCを公開しない独立WebViewで既存UIを使う。Androidからは `ephemeral=1` を追加し、既存UIもlocalStorageを使わない。

| 担当 | 所有範囲 | 状態 | 証拠 |
|---|---|---|---|
| main | 既存src・ui、root文書・tests・統合 | Windows修正・検証済み、統合中 | Rust3件・browser9件・clippy・release build PASS |
| android_feasibility | android/のみ | 調査から実装へ | Dioxus 0.7.10、SDK/NDK/Javaの存在を確認。実ビルドは未確認 |
| main | UI画像案 | 生成済み | docs/design/android-ui-concept.png。組み込み画像生成を使用。実装済み画面とは別 |
| windows_review | main差分の読取専用レビュー | 作業中 | 低速回線時にup破棄の再現あり、mainで修正・回帰追加中 |

独立したAndroid実装を1体へ委任。共有ファイルへの同時編集なし。子の起動前はmainだけで、現行同時枠は4。総トークン・速度向上・待機処理量は未測定。再委任は禁止。

レビューで、キュー64件上限がマウスボタン解放まで拒否する不具合を独立fixtureで再現。解放は必ず順番に送信し、未送信の連続move/scrollをまとめる修正を追加。Windows側の担当ファイルに競合なし。Android側はDioxus生成スクリプト由来のAGENTS.mdも適用する。

# 2026-10-21 後続: 操作性・転送機能・Androidリリース

前段の修正に続き、リモートデスクトップとしての完成度を上げる変更を同一担当で実施。共有ファイルの競合なし。

- 入力: 「直接入力」モード追加（入力差分→text/Backspace/Enter転送、Androidのgo/enterキーは暗黙フォーム送信で捕捉）。入力欄非フォーカス時のdocument単位キー転送を追加。`/input` のtextはUTF-16単位でチャンク化してSendInput。
- 転送: `/api/clipboard`（GET/SET、arboard）と`/api/upload`（256MB上限、ファイル名サニタイズ、`%LOCALAPPDATA%\Shut\incoming`、同名連番）を追加。UIにボタン行。
- ストリーム: MJPEG間隔120ms→60ms。送信が間隔を超えた場合はスキップせず即次フレーム（実効レートはネットワーク速度に追従）。
- モバイル操作: 画面右端のスクロールストリップ（#screen内・全画面時も利用可、選択中モニタに追随）、右下クイックキー（Enter/Esc/Tab/⌫）、クイックキー押下の触覚フィードバック（navigator.vibrate 10ms、対応端末のみ）。
- Android: RemoteActivityに`onShowFileChooser`実装（WebViewからのファイル選択、single-select、URIのみ・contentResolver権限付与）。`Intent`のimport漏れでKotlinコンパイル失敗→修正。`build-android.ps1`を`dx build`後に生成Gradleプロジェクトの`assembleRelease`へ接続し、未署名APK検出→開発鍵署名→`apksigner verify`→`artifacts/`出力→SHA-256報告まで自動化。
- 検証: browser回帰は19件に拡張（直接入力差分・クリップボードGET/SET・ファイル送信・ストリップ/クイックキー描画と動作・非フォーカスキー転送を追加）。Rust 3件・clippy・release build PASS。モバイル/デスクトップのスクリーンショットを`target/verification/`と`docs/verification/`に出力し目視確認。

残課題（未検証・要実機）: APK実機インストール、QR実読取、実PC操作、実機での文字入力・スクロール体感、ファイル選択UIの実機動作。ストリーミングの適応的品質（解像度/JPEG品質の自動調整）、再接続・自動再開、双方向の自動クリップボード同期、PC→端末のファイル転送は未実装。

# 2026-10-21 エミュレータ実測レビュー → 修正

AVD `dev`（arm64変換エミュレータ）にAPKをインストールして実動作検証。発見と修正:

- **接続不能（ERR_CLEARTEXT_NOT_PERMITTED相当）**: dx生成の `res/xml/network_security_config.xml` が127.0.0.1のみcleartext許可で、LAN/エミュレータ10.0.2.2へのHTTPをWebViewが拒否。`android/config/network-security-config.xml` を新設し `build-android.ps1` が生成物を置き換える方式に（dxテンプレは属性上書き不可のため生成物パッチ）。これでLAN接続・画面ミラー・タップ操作がエミュレータ上で実動作。
- **URL入力フィールドが文字を落とす**: 制御付きinput（`value: input()`）のRust→DOM書き戻しが高速入力を巻き戻す。非制御化して解消（エミュレータのスタイラスダイアログによる取りこぼしも混在していたが、防御的に是正）。
- **リロード手段がない**: ステージバーに再接続ボタン追加＋`visibilitychange`で前面復帰時に自動再接続（`shutControls.reload`）。RemoteActivity側はonPause/onResumeをWebViewに連動＋接続中のみKEEP_SCREEN_ON。
- **マウスパッドが画面を覆う**: モバイルのモーダルdrawerが画面を隠していた問題を解消。pointerパネルをモーダルではなくstage下端の常設ドッグ（.sh-paddock）化し、canvasが縮んで画面は見え続ける。デスクトップ/モバイル同一仕様。回帰テストに「パッド開放時もcanvas可視・drawer非表示」を追加。
- 実測確認済み: アプリ起動、URL手入力→確認→接続、WebViewミラー、画面タップ→PCクリック、パッドドッグ描画、HOME→復帰での配信自動復帰。
- 未確認: QR実読取（エミュレータにPlay開発者サービス無し）、実機タッチ体感、ファイル選択UI、HTTPS(WAN)経路、実機インストール。

# 2026-10-21 文字/ツールパネルのドッグ化・再接続履歴・ビルドスクリプト修正

- **文字・ツールパネルの画面非被覆化**: マウスパネルに続き、文字入力・ツールパネルもモーダルdrawerからstage下端の常設ドッグ（`.sh-paneldock`、高さ上限+縦スクロール）に統一。開いている間もリモート画面（`.sh-canvas`）が縮んで全体が見え続けることをブラウザ回帰テスト（19/19 PASS）とエミュレータ実画面で確認。
- **前回接続の履歴・ワンタップ再接続**: `ConnectionSession`で直近接続URL（最大3件）をアプリ専用`SharedPreferences`へ同期コミット（`commit()`）。ランディング画面に「前回つないだPC」カードを表示（トークンは非表示でエンドポイントのみ表示、「消す」で削除可）。タップ時は`validate_connection`で再検証し、確認シート経由で`RemoteActivity`へ再接続。`RemoteActivity`自体のプロセス再生成時も直近URLへフォールバック。
- **ビルドスクリプト修正（`android/build-android.ps1`）**:
  1. PowerShell 5.1がBOMなしUTF-8のマルチバイトコメントで構文解析を壊す問題をASCIIコメント化で解消。
  2. Gradle中間ディレクトリの再帰列挙エラーを避けるため、`target\dx\<pkg>\release\android\app`の固定パス参照に変更。
  3. 過去に別`Target`（`x86_64-linux-android`）でビルドした古い`jniLibs/x86_64/libmain.so`が残存し、エミュレータ上で古いRustバイナリが優先ロードされていた問題を特定・解消（対象ABI以外の`jniLibs`サブディレクトリとGradleネイティブ中間キャッシュをビルド前に除去）。

# 2026-10-22 リモート入力の修正（armed時の入力不能・IME合成の扱い）

「入力が上手くいかない」の報告を受けてエミュレータ+CDPで再現検証。発見と修正:

- **監視中（armed）にリモート入力欄が無効化される**: `armed` 時に全 `input` を `disabled` にする処理の除外リストが `kbd` のみで、`live_kbd`（直接入力）と `file_in`（ファイル選択）が巻き込まれていた。除外リスト2箇所（設定適用時・ポーリング時）に `live_kbd`/`file_in` を追加。監視を掛けたままリモート操作する通常利用で「文字が入らない・ファイルが送れない」状態になっていたのが主因の可能性が高い。
- **直接入力モードのIME合成対応を強化**: 確定直後のIME書き戻しによる再送を防ぐためリセットを60ms遅延化、`compositionend` 直後の同期を40ms遅延（直後に続く合成イベントに置き換え可能に）、`compositioncancel`/`blur` で未確定テキストを送信せず破棄。`focus` 時は破棄フラグを解除して即再入力可能。
- 検証で判明した非バグ: CDP `Input.insertText` はアクティブ合成を確定→追記することがあり、フィールドに「読み+確定」の両方が残る場合に両方が転送されるのはフィールド内容の忠実なミラーであり製品不具合ではない（実IMEの確定置き換えシーケンスでは単一送信を確認）。
- 検証: `node --test tests/remote-ui.cjs` 22/22 PASS（IME確定単一送信・キャンセル/blur破棄・armed時の入力有効性の回帰追加）。CDP経由の実キー/合成イベントシナリオ（commit×6・cancel・blur+再入力）全PASS。エミュレータ→Notepadへのend-to-end着弾を確認。

# 2026-10-23 日本語IME入力の修正（v0.2.1）

実機報告「IMEが日本語入力のとき入力がおかしくなる」への対応。原因として考えうる経路を全て修正:

- **確定Enterの誤爆**: `compositionend` 直後に届くEnter（keyCode 13のkeydown・フォームsubmit）をIMEの確定操作の延長とみなし200msはPCへ送らない。従来は確定のたびにPC側で改行が挿入され、まとめて送信では変換途中に送信が走っていた可能性。
- **リセットが合成中テキストを消す**: 送信後のフィールドリセット（60ms）が次の文節の合成中に発火すると入力中の文字が消えIME内部状態と乖離していた。合成中はリセットを延期するよう修正。
- **サロゲートペア切断**: 差分計算をUTF-16単位で切っており絵文字が壊れる・Backspaceがコードユニット数送られる問題を、コードポイント単位の境界補正で修正。
- 回帰テスト追加（計26件）: 確定Enter抑止（直接入力/まとめて）、送信直後の合成継続、絵文字差分。

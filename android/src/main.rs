use dioxus::prelude::*;
use shut_android::{ConnectionTarget, validate_connection};

mod native;
const CSS: Asset = asset!("/assets/main.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let mut input = use_signal(String::new);
    let mut candidate = use_signal(|| None::<ConnectionTarget>);
    let mut message = use_signal(String::new);
    let mut scanning = use_signal(|| false);
    // アプリ専用領域の直近接続（クラッシュ・終了後の再接続用）
    let mut recents = use_signal(native::recent);

    use_future(move || async move {
        // JNIブリッジは起動直後の初回描画では未初期化なことがあるため、
        // ポーリングループ内で履歴を再読込する（RemoteActivityから戻った後も更新される）。
        let mut ticks = 0u32;
        loop {
            if ticks.is_multiple_of(4) {
                let latest = native::recent();
                if latest != recents() {
                    recents.set(latest);
                }
            }
            ticks = ticks.wrapping_add(1);
            if let Some(event) = native::poll() {
                scanning.set(false);
                match event {
                    native::Event::Scanned(value) => match validate_connection(&value) {
                        Ok(target) => {
                            candidate.set(Some(target));
                            message.set(String::new());
                        }
                        Err(error) => {
                            candidate.set(None);
                            message.set(error.into());
                        }
                    },
                    native::Event::Cancelled => {
                        message.set("読み取りをキャンセルしました。".into())
                    }
                    native::Event::Error(error) => message.set(error),
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
    });

    rsx! {
        document::Stylesheet { href: CSS }
        document::Title { "Shut — 小さな操作室" }
        main { class: "shell",
            header { class: "topbar",
                div { class: "brand", span { class: "brand-dot" } "Shut" }
                span { class: "pill", "小さな操作室" }
            }
            section { class: "intro",
                h1 { "PCを、手元に。" }
                p { "PCのShutとつないで、画面の確認や操作を。" }
            }
            section { class: "card scan-card",
                button {
                    class: "scan-action", disabled: scanning(),
                    onclick: move |_| {
                        message.set(String::new()); candidate.set(None);
                        match native::scan() {
                            Ok(()) => scanning.set(true),
                            Err(error) => message.set(error),
                        }
                    },
                    div { class: "scan-mark", aria_hidden: "true",
                        span { class: "corner tl" } span { class: "corner tr" }
                        span { class: "corner bl" } span { class: "corner br" }
                        span { class: "qr-glyph", "▦" }
                    }
                    strong { if scanning() { "読み取り中…" } else { "QRコードを読み取る" } }
                    span { class: "scan-sub", "PCに表示されたQRをカメラで" }
                }
            }
            section { class: "manual-card",
                label { r#for: "connection-url", "または、接続URLを入力" }
                div { class: "input-row",
                    input {
                        id: "connection-url", r#type: "url",
                        placeholder: "https://…/?t=…", autocomplete: "off", spellcheck: "false",
                        autocapitalize: "none", inputmode: "url",
                        // 制御付き(value: input())にすると Rust→DOM の書き戻しが
                        // 入力中の文字を巻き戻して落とすことがあるため、DOM側の値を信頼する。
                        oninput: move |event| {
                            input.set(event.value()); candidate.set(None); message.set(String::new());
                        },
                    }
                    button {
                        class: "url-action", disabled: scanning(),
                        onclick: move |_| match validate_connection(&input()) {
                            Ok(target) => { candidate.set(Some(target)); message.set(String::new()); }
                            Err(error) => { candidate.set(None); message.set(error.into()); }
                        },
                        "確認"
                    }
                }
            }
            if !message().is_empty() {
                div { class: "notice", role: "status", "{message}" }
            }
            if !recents().is_empty() {
                section { class: "recent-card",
                    div { class: "recent-head",
                        span { "前回つないだPC" }
                        button {
                            class: "recent-clear",
                            onclick: move |_| {
                                native::forget_recent(); recents.set(Vec::new());
                            },
                            "消す"
                        }
                    }
                    for url in recents() {
                        button {
                            class: "recent-row",
                            onclick: move |_| match validate_connection(&url) {
                                Ok(target) => { candidate.set(Some(target)); message.set(String::new()); }
                                Err(_) => {
                                    // 無効な履歴は端末側の一覧からも除去する
                                    let kept: Vec<String> =
                                        recents().into_iter().filter(|u| *u != url).collect();
                                    native::forget_recent();
                                    for entry in kept.iter().rev() {
                                        native::record_recent(entry);
                                    }
                                    recents.set(kept);
                                }
                            },
                            span { class: "recent-endpoint",
                                match validate_connection(&url) {
                                    Ok(target) => target.endpoint.clone(),
                                    Err(_) => url.clone(),
                                }
                            }
                            span { class: "recent-action", "もう一度つなぐ" }
                        }
                    }
                }
            }
            details { class: "help",
                summary { "はじめての接続・読み取れないとき" }
                p { "PC側でShutを起動し、接続QRを表示してください。読み取った後に接続先を確認できます。" }
                p { "QRはGoogle Play 開発者サービスを使います。初回はネット接続と機能のダウンロードが必要です。非対応端末や読み取りエラー時は、PCの接続URLを入力してください。" }
            }
            footer { span { class: "privacy-dot" } "接続URLは、あなたのPCの鍵です。" }
        }
        if let Some(target) = candidate() {
            div { class: "scrim",
                section { class: "confirmation", role: "dialog", aria_modal: "true", aria_label: "接続先の確認",
                    div { class: "handle" }
                    p { class: "eyebrow", "接続先の確認" }
                    h2 { "このPCにつなぎますか？" }
                    p { class: "endpoint", "{target.endpoint}" }
                    if target.cleartext {
                        p { class: "http-warning", "HTTPは暗号化されません。信頼できる同じネットワークで、PCのアドレスと一致することを確認してください。" }
                    } else {
                        p { class: "confirm-note", "PCに表示されたアドレスと一致することを確認してください。" }
                    }
                    button {
                        class: "primary",
                        onclick: move |_| {
                            if let Some(confirmed) = candidate() {
                                let url = confirmed.ephemeral_url();
                                match native::open(&url) {
                                    // 入力欄はDOM側の値を保持するためsignalも消さず同期を保つ
                                    Ok(()) => {
                                        native::record_recent(&url);
                                        recents.set(native::recent());
                                        candidate.set(None);
                                        message.set(String::new());
                                    }
                                    Err(error) => message.set(error),
                                }
                            }
                        },
                        "確認して接続"
                    }
                    button { class: "quiet", onclick: move |_| candidate.set(None), "戻る" }
                }
            }
        }
    }
}

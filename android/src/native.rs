#[allow(dead_code)] // The desktop preview has no Android scanner events.
pub enum Event {
    Scanned(String),
    Cancelled,
    Error(String),
}

#[cfg(target_os = "android")]
mod platform {
    use super::Event;
    use manganis::jni::objects::{GlobalRef, JClass, JObject, JValue};
    use std::sync::OnceLock;

    #[manganis::ffi("/native")]
    extern "Kotlin" {
        pub type NativeBridge;
        pub fn start_scan(this: &NativeBridge);
        pub fn take_result(this: &NativeBridge) -> String;
        pub fn open_connection(this: &NativeBridge, url: String);
        pub fn save_recent(this: &NativeBridge, url: String);
        pub fn load_recent(this: &NativeBridge) -> String;
        pub fn clear_recent(this: &NativeBridge);
    }

    static BRIDGE: OnceLock<NativeBridge> = OnceLock::new();

    fn bridge() -> Result<&'static NativeBridge, String> {
        if let Some(bridge) = BRIDGE.get() {
            return Ok(bridge);
        }
        // Generated new() uses FindClass on an attached Rust thread. Load the APK
        // class through Activity's classloader instead (upstream issue #5629).
        let result = manganis::android::with_activity(|env, activity| {
            let created = (|| -> manganis::jni::errors::Result<GlobalRef> {
                let loader = env
                    .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                    .l()?;
                let name = env.new_string("jp.shut.bridge.NativeBridge")?;
                let class = env
                    .call_method(
                        &loader,
                        "loadClass",
                        "(Ljava/lang/String;)Ljava/lang/Class;",
                        &[JValue::Object(&name)],
                    )?
                    .l()?;
                let class: JClass<'_> = class.into();
                let instance: JObject<'_> = env.new_object(
                    &class,
                    "(Landroid/app/Activity;)V",
                    &[JValue::Object(activity)],
                )?;
                env.new_global_ref(instance)
            })();
            match created {
                Ok(global) => Some(NativeBridge::from_global_ref(global)),
                Err(_) => {
                    let _ = env.exception_clear();
                    None
                }
            }
        });
        let instance = result.ok_or_else(|| {
            "Androidの接続機能を開始できませんでした。アプリを開き直してください。".to_string()
        })?;
        let _ = BRIDGE.set(instance);
        BRIDGE
            .get()
            .ok_or_else(|| "接続機能を開始できませんでした。".into())
    }

    pub fn scan() -> Result<(), String> {
        start_scan(bridge()?)
            .map_err(|_| "QR読み取りを開始できません。接続URLを入力してください。".into())
    }
    pub fn poll() -> Option<Event> {
        let result = match take_result(BRIDGE.get()?) {
            Ok(result) => result,
            Err(_) => {
                return Some(Event::Error(
                    "接続機能を読み出せません。アプリを開き直してください。".into(),
                ));
            }
        };
        if let Some(value) = result.strip_prefix("scan:") {
            Some(Event::Scanned(value.to_string()))
        } else if result == "cancelled" {
            Some(Event::Cancelled)
        } else {
            result
                .strip_prefix("error:")
                .map(|error| Event::Error(error.to_string()))
        }
    }
    pub fn open(url: &str) -> Result<(), String> {
        open_connection(bridge()?, url.to_string())
            .map_err(|_| "接続画面を開始できません。アプリを開き直してください。".into())
    }
    pub fn record_recent(url: &str) {
        if let Ok(bridge) = bridge() {
            let _ = save_recent(bridge, url.to_string());
        }
    }
    pub fn recent() -> Vec<String> {
        match bridge() {
            Ok(b) => load_recent(b)
                .map(|raw| {
                    raw.split('\n')
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
    pub fn forget_recent() {
        if let Ok(bridge) = bridge() {
            let _ = clear_recent(bridge);
        }
    }
}

#[cfg(not(target_os = "android"))]
mod platform {
    use super::Event;
    pub fn scan() -> Result<(), String> {
        Err("QR読み取りはAndroidアプリで利用できます。接続URLの入力もお使いいただけます。".into())
    }
    pub fn poll() -> Option<Event> {
        None
    }
    pub fn open(_: &str) -> Result<(), String> {
        Err("PC画面への接続はAndroidアプリで利用できます。".into())
    }
    pub fn record_recent(_: &str) {}
    pub fn recent() -> Vec<String> {
        Vec::new()
    }
    pub fn forget_recent() {}
}

pub use platform::{forget_recent, open, poll, recent, record_recent, scan};

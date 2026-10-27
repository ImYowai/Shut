// エンコーダ単体診断: フレーム投入→出力AUのサイズ・フラグ・NALタイプを表示
#![allow(dead_code)] // 本体で使う共有モジュールのAPIがここでは未使用になり得る
#[cfg(windows)]
#[path = "../dxgicap.rs"]
mod dxgicap;
#[cfg(windows)]
#[path = "../mfenc.rs"]
mod mfenc;

#[cfg(windows)]
fn main() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(
            None,
            windows::Win32::System::Com::COINIT_MULTITHREADED,
        );
    }
    let mut dup = match dxgicap::new() {
        Some(d) => d,
        None => {
            println!("duplication unavailable");
            return;
        }
    };
    let (w, h) = dup.size();
    println!("capture {}x{}", w, h);
    let mut enc = match mfenc::new(w, h, 60, 8_000_000) {
        Some(e) => e,
        None => {
            println!("encoder unavailable");
            return;
        }
    };
    println!("encoder is_async={}", enc.is_async);
    let mut fed = 0;
    let mut total_out = 0usize;
    for _ in 0..400 {
        if fed >= 60 {
            break;
        }
        let Some(f) = dup.next_frame(100) else {
            continue;
        };
        fed += 1;
        let units = enc.encode(&f.data);
        total_out += units.len();
        for u in &units {
            let head: Vec<String> = u
                .data
                .iter()
                .take(10)
                .map(|b| format!("{b:02x}"))
                .collect();
            println!(
                "frame {fed} -> AU {}B key={} [{}]",
                u.data.len(),
                u.keyframe,
                head.join(" ")
            );
        }
    }
    println!("--- fed={fed} out={total_out} --- drain ---");
    for u in enc.drain_all() {
        let head: Vec<String> = u
            .data
            .iter()
            .take(10)
            .map(|b| format!("{b:02x}"))
            .collect();
        println!("drain -> AU {}B key={} [{}]", u.data.len(), u.keyframe, head.join(" "));
    }
}

#[cfg(not(windows))]
fn main() {}

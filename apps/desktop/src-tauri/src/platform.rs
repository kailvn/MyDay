//! 平台后端选择（OVERLAY-SPEC §2）：Linux Wayland 会话下整进程注入
//! `GDK_BACKEND=x11` 走 XWayland，换取置顶 / 自定位 / 穿透全部可用。
//! 原生 Wayland 是协议级限制（tao 标注 Unsupported），与 UI 框架无关，
//! 故不做窗口级方案（§2 决策 D1/D2/D5）。

use std::sync::OnceLock;

static NATIVE_WAYLAND: OnceLock<bool> = OnceLock::new();

fn session_is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE")
            .map(|v| v.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
}

/// 会话为 Wayland 且未注入 x11（`MYDAY_BACKEND=wayland` 或用户自设
/// `GDK_BACKEND=wayland`）：置顶 / 定位降级为合成器行为，位置不读写。
pub fn native_wayland() -> bool {
    *NATIVE_WAYLAND.get_or_init(|| false)
}

/// 在第一次 GTK 初始化（Builder::run/build）之前调用（OVERLAY-SPEC §2.3）。
/// 判定顺序：`MYDAY_BACKEND` 显式指定 > 已有 `GDK_BACKEND` 尊重不覆盖 >
/// Wayland 会话注入 `x11` > 其余不动。注入与否写一行日志，便于排查「为什么不置顶」。
pub fn force_x11_on_wayland() {
    let session_wayland = session_is_wayland();
    let forced = std::env::var("MYDAY_BACKEND")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty());
    let mut injected = false;
    let mut native = false;

    if session_wayland {
        match forced.as_deref() {
            Some("wayland") => native = true, // 强制原生（降级：无置顶/定位）
            Some("x11") => {}
            _ => match std::env::var("GDK_BACKEND")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
            {
                Some(existing) => native = existing.eq_ignore_ascii_case("wayland"), // 用户已设：尊重
                None => {
                    std::env::set_var("GDK_BACKEND", "x11");
                    injected = true;
                }
            },
        }
    }

    let _ = NATIVE_WAYLAND.set(native);
    eprintln!(
        "myday: 显示后端 = {}（会话 {}，注入 GDK_BACKEND=x11 = {injected}）",
        if native { "wayland 原生（降级：无置顶/定位）" } else { "x11" },
        if session_wayland { "wayland" } else { "x11/其他" },
    );
}

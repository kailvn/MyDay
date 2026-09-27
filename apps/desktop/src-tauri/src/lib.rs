//! MyDay 应用壳（Tauri 2，桌面 + 移动共用一个 src-tauri）。
//!
//! 职责（需求 §9.2 架构）：
//! - 承载 Web 前端（桌面：主窗口 + 快速添加窗口 + 今日悬浮窗；移动：单主窗口）
//! - 通过 [`myday_core::Store`] 直接读写本地数据
//! - 桌面专属：本地 IPC 服务（Unix socket，CLI 协同）、后台提醒循环（DBus
//!   系统通知）、系统托盘常驻（主窗口关闭仅隐藏、后台保活）
//!
//! 平台分界用 `cfg(desktop)` / `cfg(mobile)`（tauri-build 注入）划分，
//! 依赖表用 `target.'cfg(not(any(android, ios)))'` 对应隔离（见 Cargo.toml）。

pub mod commands;
pub mod i18n;
#[cfg(desktop)]
pub mod ipc_bridge;
pub mod logging;
#[cfg(desktop)]
pub mod platform;
pub mod reminder_loop;
#[cfg(mobile)]
pub mod sync_client;
#[cfg(desktop)]
pub mod sync_server;

use std::sync::Arc;

use myday_core::Store;
#[cfg(desktop)]
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};

pub struct AppState {
    pub store: Arc<Store>,
}

/// 回收站保留天数：删除超过该天数的条目在启动时彻底删除。
const TRASH_RETENTION_DAYS: i64 = 30;

/// 托盘菜单句柄：设置页改配置后同步勾选状态（OVERLAY-SPEC §8）；
/// 切换界面语言时整组 `set_text` 重写（i18n）。
#[cfg(desktop)]
pub struct OverlayTrayMenu {
    pub open: MenuItem<tauri::Wry>,
    pub quick: MenuItem<tauri::Wry>,
    pub show: CheckMenuItem<tauri::Wry>,
    pub lock: CheckMenuItem<tauri::Wry>,
    pub quit: MenuItem<tauri::Wry>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 必须在第一次 GTK 初始化（Builder::run/build）之前注入（OVERLAY-SPEC §2.3）
    #[cfg(desktop)]
    platform::force_x11_on_wayland();

    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(single_instance_handler))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ));
    let builder = builder.invoke_handler(commands::handler());

    // 桌面：启动即开库并管理状态。移动端不能这样开库——安卓没有 HOME/XDG，
    // `dirs` 系解析不出数据目录（io error: cannot resolve data dir），须等
    // tauri 的路径解析器给出应用私有目录，见 `mobile_setup`。
    #[cfg(desktop)]
    let builder = {
        let store = match Store::open_default() {
            Ok(s) => Arc::new(s),
            Err(e) => {
                // 数据目录不可用时日志也未初始化，只能 stderr
                eprintln!("myday: 无法打开数据库: {e}");
                std::process::exit(1);
            }
        };
        init_logging_and_trash_purge(&store);
        builder
            .manage(AppState {
                store: store.clone(),
            })
            .manage(sync_server::SyncRunning(std::sync::Mutex::new(None)))
            .setup(|_app| {
                desktop_setup(_app)?;
                Ok(())
            })
            .on_window_event(|window, event| {
                // 主窗口关闭 → 隐藏保活；快速窗口关闭 → 隐藏
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let _ = window.hide();
                    api.prevent_close();
                }
            })
    };

    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_notification::init());
    // 扫码配对：扫桌面端「移动端同步」小节里的二维码自动填地址 + 配对码
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_barcode_scanner::init());
    #[cfg(mobile)]
    let builder = builder.setup(|_app| {
        mobile_setup(_app)?;
        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 启动期装配：日志初始化 + 回收站过期清理（桌面 / 移动共用）。
fn init_logging_and_trash_purge(store: &Store) {
    logging::init(store.data_root());
    logging::log(&format!(
        "myday: 启动 v{}（数据目录 {}）",
        env!("CARGO_PKG_VERSION"),
        store.data_root().display()
    ));

    // 回收站过期清理：删除超过 30 天的条目彻底删除（GUI 常驻是唯一清理时机；
    // 失败只影响本次，下次启动重试）
    match store.purge_expired_trash(TRASH_RETENTION_DAYS) {
        Ok(0) => {}
        Ok(n) => logging::log(&format!(
            "myday: 已清理回收站 {n} 条（超 {TRASH_RETENTION_DAYS} 天）"
        )),
        Err(e) => logging::log(&format!("myday: 回收站清理失败: {e}")),
    }
}

/// 移动端 setup：数据目录用 tauri 解析出的应用私有目录（/data/user/0/<pkg>/files），
/// 借 core 的 `MYDAY_DATA_DIR` 覆盖机制喂给 [`Store::open_default`]。
/// 开库放在 setup（启动早期还拿不到路径）；命令调用均晚于 setup，时序安全。
#[cfg(mobile)]
fn mobile_setup(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Manager;
    let dir = app.path().app_data_dir()?;
    std::env::set_var("MYDAY_DATA_DIR", &dir);
    let store = Store::open_default().map_err(|e| {
        tauri::Error::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("无法打开数据库: {e}"),
        ))
    })?;
    let store = Arc::new(store);
    init_logging_and_trash_purge(&store);
    app.manage(AppState {
        store: store.clone(),
    });
    // 提醒循环：移动端走 tauri-plugin-notification（通知权限由前端运行时申请）
    reminder_loop::spawn(app.handle().clone(), store);
    Ok(())
}

/// 桌面端 setup：IPC 服务、提醒循环、托盘、悬浮窗恢复、测试钩子。
#[cfg(desktop)]
fn desktop_setup(app: &tauri::App) -> tauri::Result<()> {
    let store = app.state::<AppState>().store.clone();

    // IPC 服务：CLI → GUI 协同通道（需求 §2.4、§9.3）
    let handler = Arc::new(ipc_bridge::IpcBridge {
        app: app.handle().clone(),
        store: store.clone(),
    });
    match myday_core::ipc::bind(&myday_core::socket_path()?) {
        Ok(listener) => {
            std::thread::Builder::new()
                .name("myday-ipc".into())
                .spawn(move || myday_core::ipc::serve(listener, handler))?;
        }
        Err(e) => {
            logging::log(&format!("myday: IPC 服务启动失败（CLI 将直写数据库）: {e}"));
        }
    }

    // 提醒循环：每 30 秒扫描到期条目并发送 GNOME 通知（需求 §9）
    reminder_loop::spawn(app.handle().clone(), store.clone());

    setup_tray(app)?;
    apply_language(&store, app.handle());

    // 悬浮窗随启动常驻（OVERLAY-SPEC §8）：enabled 时恢复显示（不重复持久化）
    if commands::overlay_config(&store).unwrap_or_default().enabled {
        let _ = commands::set_overlay_visible(app.handle(), &store, true, false);
    }

    // 同步服务：上次会话开着就随启动拉起（托盘常驻 = 服务常在）
    if store.get_setting("sync.enabled").ok().flatten().as_deref() == Some("true") {
        let token = {
            let existing = store
                .get_setting("sync.token")
                .ok()
                .flatten()
                .unwrap_or_default();
            if existing.is_empty() {
                let t = sync_server::new_token();
                let _ = store.set_setting("sync.token", &t);
                t
            } else {
                existing
            }
        };
        let handle = app.handle().clone();
        let st = store.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = sync_server::server_start(&handle, st, token).await {
                logging::log(&format!("myday: 同步服务启动失败: {e}"));
            }
        });
    }

    // 测试钩子：MYDAY_TEST_POS="x,y" 时把主窗口放到固定位置并重新 show+focus
    // （自动化 UI 测试中窗口管理器可能把新窗口放到屏外，需要确定几何）
    if let Ok(pos) = std::env::var("MYDAY_TEST_POS") {
        let (x, y) = pos
            .split_once(',')
            .map(|(a, b)| (a.trim(), b.trim()))
            .unwrap_or(("", ""));
        if let (Ok(x), Ok(y)) = (x.parse::<f64>(), y.parse::<f64>()) {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_position(tauri::LogicalPosition::new(x, y));
                let _ = win.hide();
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    }
    Ok(())
}

#[cfg(desktop)]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let store = app.state::<AppState>().store.clone();
    let s = i18n::strings(i18n::Lang::from_store(&store));
    let open = MenuItem::with_id(app, "open", s.tray_open, true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", s.tray_quick_add, true, None::<&str>)?;
    // 悬浮窗两项（OVERLAY-SPEC §5.2/§8）：显示开关 + 锁定复选（锁定后窗口外解锁入口）
    let overlay_cfg = commands::overlay_config(&store).unwrap_or_default();
    let overlay_show = CheckMenuItem::with_id(
        app,
        "overlay-show",
        s.tray_overlay_show,
        true,
        overlay_cfg.enabled,
        None::<&str>,
    )?;
    let overlay_lock = CheckMenuItem::with_id(
        app,
        "overlay-lock",
        s.tray_overlay_lock,
        true,
        overlay_cfg.locked,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", s.tray_quit, true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &quick,
            &PredefinedMenuItem::separator(app)?,
            &overlay_show,
            &overlay_lock,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    // 句柄交给 managed state：勾选同步（overlay 配置变更）与语言切换重写文案
    app.manage(OverlayTrayMenu {
        open: open.clone(),
        quick: quick.clone(),
        show: overlay_show.clone(),
        lock: overlay_lock.clone(),
        quit: quit.clone(),
    });

    TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "quick" => ipc_bridge::show_quick_add(app, None, None),
            "overlay-show" => {
                let checked = overlay_show.is_checked().unwrap_or(false);
                let store = app.state::<AppState>().store.clone();
                let _ = commands::set_overlay_visible(app, &store, checked, true);
            }
            "overlay-lock" => {
                let checked = overlay_lock.is_checked().unwrap_or(false);
                let store = app.state::<AppState>().store.clone();
                let _ = commands::set_overlay_locked(app, &store, checked);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 界面语言落地：托盘整组改文案 + 两个副窗口标题（主窗口标题即产品名不动）。
/// 启动时与设置页切换语言（`set_ui_lang`）时各调一次。
#[cfg(desktop)]
pub fn apply_language(store: &Store, app: &tauri::AppHandle) {
    let s = i18n::strings(i18n::Lang::from_store(store));
    if let Some(tray_menu) = app.try_state::<OverlayTrayMenu>() {
        let _ = tray_menu.open.set_text(s.tray_open);
        let _ = tray_menu.quick.set_text(s.tray_quick_add);
        let _ = tray_menu.show.set_text(s.tray_overlay_show);
        let _ = tray_menu.lock.set_text(s.tray_overlay_lock);
        let _ = tray_menu.quit.set_text(s.tray_quit);
    }
    let titles = [
        ("quick-add", s.quickadd_title),
        ("overlay", s.overlay_title),
    ];
    for (label, title) in titles {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.set_title(title);
        }
    }
}

#[cfg(desktop)]
pub fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 单实例：二次启动带 --overlay 唤起悬浮窗，带 quick-add 参数唤起快速窗口，
/// 否则前置主窗口（需求 §七.6、OVERLAY-SPEC §8）。
#[cfg(desktop)]
fn single_instance_handler(app: &tauri::AppHandle, args: Vec<String>, _cwd: String) {
    let wants_quick = args.iter().any(|a| a == "quick-add");
    let wants_overlay = args.iter().any(|a| a == "--overlay" || a == "overlay");
    if wants_overlay {
        // 唤起但不持久化 enabled（与 quick-add 分支同款：只唤起，不改配置）
        let store = app.state::<AppState>().store.clone();
        let _ = commands::set_overlay_visible(app, &store, true, false);
    } else if wants_quick {
        let item_type = myday_core::model::ItemType::Log;
        ipc_bridge::show_quick_add(app, Some(item_type), None);
    } else {
        show_main(app);
    }
}

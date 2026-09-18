//! MyDay 桌面应用（Tauri 2）。
//!
//! 职责（需求 §9.2 架构）：
//! - 承载 Web 前端（主窗口 + 快速添加窗口 + 今日悬浮窗）
//! - 通过 [`myday_core::Store`] 直接读写本地数据
//! - 启动本地 IPC 服务（Unix socket），供 CLI 转发写操作 / 唤起快速窗口
//! - 后台提醒循环（30 秒，DBus 系统通知）
//! - 系统托盘常驻；主窗口关闭仅隐藏（后台保活）

pub mod commands;
pub mod i18n;
pub mod ipc_bridge;
pub mod logging;
pub mod platform;
pub mod reminder_loop;

use std::sync::Arc;

use myday_core::Store;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    Manager, WindowEvent,
};

pub struct AppState {
    pub store: Arc<Store>,
}

/// 托盘菜单句柄：设置页改配置后同步勾选状态（OVERLAY-SPEC §8）；
/// 切换界面语言时整组 `set_text` 重写（i18n）。
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
    platform::force_x11_on_wayland();

    let store = match Store::open_default() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            // 数据目录不可用时日志也未初始化，只能 stderr
            eprintln!("myday: 无法打开数据库: {e}");
            std::process::exit(1);
        }
    };
    logging::init(store.data_root());
    logging::log(&format!(
        "myday: 启动 v{}（数据目录 {}）",
        env!("CARGO_PKG_VERSION"),
        store.data_root().display()
    ));

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(single_instance_handler))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState { store: store.clone() })
        .invoke_handler(tauri::generate_handler![
            commands::list_items,
            commands::list_items_window,
            commands::tasks_view,
            commands::get_item,
            commands::add_item,
            commands::update_item,
            commands::delete_item,
            commands::complete_task,
            commands::snooze,
            commands::search_items,
            commands::list_templates,
            commands::add_template,
            commands::update_template,
            commands::delete_template,
            commands::move_template,
            commands::set_template_pinned,
            commands::list_field_defs,
            commands::add_field_def,
            commands::update_field_def,
            commands::delete_field_def,
            commands::count_items_with_field,
            commands::list_deleted_field_defs,
            commands::purge_deleted_field_defs,
            commands::add_attachment_b64,
            commands::attachment_abs_path,
            commands::get_setting,
            commands::set_setting,
            commands::check_conflict,
            commands::stats_summary,
            commands::view_list,
            commands::view_get,
            commands::view_create,
            commands::view_save,
            commands::view_delete,
            commands::view_reset,
            commands::view_duplicate,
            commands::query_view,
            commands::query_stats_page,
            commands::stats_restore_defaults,
            commands::reminder_history,
            commands::reminder_unread,
            commands::mark_reminders_seen,
            commands::convert_task_to_event,
            commands::event_to_log,
            commands::export_ics,
            commands::backup_zip,
            commands::load_holidays_json,
            commands::save_holidays_json,
            commands::reset_holidays_json,
            commands::app_info,
            commands::open_file_path,
            commands::reveal_file_path,
            commands::open_log_dir,
            commands::open_quick_add,
            commands::get_autostart,
            commands::set_autostart,
            commands::set_ui_lang,
            commands::overlay_today,
            commands::get_overlay_config,
            commands::set_overlay_config,
            commands::overlay_apply_window_state,
            commands::overlay_set_visible,
            commands::overlay_save_drag_pos,
            commands::overlay_save_resize_size,
            commands::overlay_show_main,
        ])
        .setup(move |app| {
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
            let store_state = app.state::<AppState>();
            if commands::overlay_config(&store_state.store).unwrap_or_default().enabled {
                let _ = commands::set_overlay_visible(app.handle(), &store, true, false);
            }

            // 测试钩子：MYDAY_TEST_POS="x,y" 时把主窗口放到固定位置并重新 show+focus
            // （自动化 UI 测试中窗口管理器可能把新窗口放到屏外，需要确定几何）
            if let Ok(pos) = std::env::var("MYDAY_TEST_POS") {
                let (x, y) = pos.split_once(',').map(|(a, b)| (a.trim(), b.trim())).unwrap_or(("", ""));
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
        })
        .on_window_event(|window, event| {
            // 主窗口关闭 → 隐藏保活；快速窗口关闭 → 隐藏
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

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
pub fn apply_language(store: &Store, app: &tauri::AppHandle) {
    let s = i18n::strings(i18n::Lang::from_store(store));
    if let Some(tray_menu) = app.try_state::<OverlayTrayMenu>() {
        let _ = tray_menu.open.set_text(s.tray_open);
        let _ = tray_menu.quick.set_text(s.tray_quick_add);
        let _ = tray_menu.show.set_text(s.tray_overlay_show);
        let _ = tray_menu.lock.set_text(s.tray_overlay_lock);
        let _ = tray_menu.quit.set_text(s.tray_quit);
    }
    let titles = [("quick-add", s.quickadd_title), ("overlay", s.overlay_title)];
    for (label, title) in titles {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.set_title(title);
        }
    }
}

pub fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

/// 单实例：二次启动带 --overlay 唤起悬浮窗，带 quick-add 参数唤起快速窗口，
/// 否则前置主窗口（需求 §七.6、OVERLAY-SPEC §8）。
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

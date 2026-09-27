//! 后台提醒循环（需求 §9 / SPRINT-SPEC §1）。
//!
//! 每 30 秒执行一轮 [`myday_core::reminder::tick_once`]：到期未发送的提醒 →
//! 系统通知。文案在本层按界面语言渲染（core 只传结构化数据，见 `i18n` 模块）。
//!
//! 平台分岔（`cfg(desktop)` / `cfg(mobile)`）：
//! - 桌面：notify-rust / DBus，通知带动作按钮（完成 / 稍后提醒 / 打开），
//!   窗口外错过聚合成摘要；
//! - 移动：tauri-plugin-notification 系统通知（无动作按钮，点击即唤起 App；
//!   前端负责运行时申请通知权限）。

use std::sync::Arc;
use std::time::Duration;

use myday_core::model::{Item, ItemType, Reminder};
use myday_core::reminder::Notifier;
use myday_core::Store;

use crate::i18n::{strings, Lang};
use crate::logging;

/// 通知摘要 / 正文的本地化组装（时间格式两侧统一 `MM-DD HH:MM`）。
fn render(lang: Lang, item: &Item, at: chrono::DateTime<chrono::Utc>) -> (String, String) {
    let s = strings(lang);
    let time = at
        .with_timezone(&chrono::Local)
        .format("%m-%d %H:%M")
        .to_string();
    match item.item_type {
        ItemType::Task => (
            s.sum_task
                .replace("{title}", &myday_core::model::display_title(item)),
            item.due_at
                .map(|_| s.body_task_due.replace("{time}", &time))
                .unwrap_or_else(|| s.body_task_plain.into()),
        ),
        ItemType::Event => (
            s.sum_event
                .replace("{title}", &myday_core::model::display_title(item)),
            item.start_at
                .map(|_| s.body_event_start.replace("{time}", &time))
                .unwrap_or_else(|| s.body_event_plain.into()),
        ),
        ItemType::Log => (
            s.sum_log
                .replace("{title}", &myday_core::model::display_title(item)),
            s.body_log.into(),
        ),
    }
}

/// 错过聚合摘要正文（两端共用）。
fn missed_body(lang: Lang, count: usize, lines: &[(String, String)]) -> String {
    let s = strings(lang);
    let mut body = String::new();
    for (title, at) in lines {
        body.push_str(&s.missed_line.replace("{title}", title).replace("{at}", at));
        body.push('\n');
    }
    if count > lines.len() {
        body.push_str(
            &s.missed_more
                .replace("{n}", &(count - lines.len()).to_string()),
        );
    }
    body.trim_end().to_string()
}

/// 提醒触发同时广播前端（提醒中心未读角标；通知无法保证携带回调，条目定位兜底）。
fn emit_reminder_fired(app: &tauri::AppHandle, reminder: &Reminder, item: &Item) {
    let _ = tauri::Emitter::emit(
        app,
        "reminder-fired",
        serde_json::json!({ "id": item.id, "reminder_id": reminder.id }),
    );
}

fn spawn_loop(app: tauri::AppHandle, store: Arc<Store>) {
    std::thread::Builder::new()
        .name("myday-reminders".into())
        .spawn(move || loop {
            if let Err(e) = tick(&app, &store) {
                logging::log(&format!("myday: 提醒循环错误: {e}"));
            }
            std::thread::sleep(Duration::from_secs(30));
        })
        .expect("spawn reminder loop");
}

#[cfg(desktop)]
fn tick(app: &tauri::AppHandle, store: &Arc<Store>) -> Result<(), myday_core::MyDayError> {
    let notifier = DesktopNotifier {
        app: app.clone(),
        store: store.clone(),
    };
    myday_core::reminder::tick_once(store, &notifier).map(|_| ())
}

#[cfg(mobile)]
fn tick(app: &tauri::AppHandle, store: &Arc<Store>) -> Result<(), myday_core::MyDayError> {
    let notifier = MobileNotifier {
        app: app.clone(),
        store: store.clone(),
    };
    myday_core::reminder::tick_once(store, &notifier).map(|_| ())
}

/// 启动提醒循环（两端共用入口；`mobile_setup` / `desktop_setup` 各自调用）。
pub fn spawn(app: tauri::AppHandle, store: Arc<Store>) {
    spawn_loop(app, store);
}

// ----------------------------------------------------------------------
// 桌面：notify-rust / DBus，通知带动作按钮
// ----------------------------------------------------------------------

#[cfg(desktop)]
struct DesktopNotifier {
    app: tauri::AppHandle,
    store: Arc<Store>,
}

/// 动作 ID → 本地化标签（`default` = 通知本体点击 = 打开定位）。
#[cfg(desktop)]
fn action_label(lang: Lang, id: &str) -> &'static str {
    let s = strings(lang);
    match id {
        "complete" => s.act_complete,
        "snooze" => s.act_snooze,
        _ => s.act_open,
    }
}

#[cfg(desktop)]
impl Notifier for DesktopNotifier {
    fn notify(
        &self,
        reminder: &Reminder,
        item: &Item,
        at: chrono::DateTime<chrono::Utc>,
        actions: &[&'static str],
    ) {
        let lang = Lang::from_store(&self.store);
        let (summary, body) = render(lang, item, at);
        let app = self.app.clone();
        let store = self.store.clone();
        let item_id = item.id.clone();
        let labels: Vec<(String, String)> = actions
            .iter()
            .map(|id| (id.to_string(), action_label(lang, id).to_string()))
            .collect();
        // wait_for_action 会阻塞到通知关闭，必须独立线程，否则卡死提醒环。
        let _ = std::thread::Builder::new()
            .name("myday-notify".into())
            .spawn(move || {
                // builder 方法返回 &mut Self，链式赋值会借用临时值，须逐条语句调用
                let mut n = notify_rust::Notification::new();
                n.appname("MyDay").summary(&summary).body(&body);
                for (id, label) in &labels {
                    n.action(id, label);
                }
                // 通知本体点击 = 打开定位（GNOME 上 default action 由通知体触发）
                n.action("default", action_label(lang, "default"));
                match n.show() {
                    Ok(handle) => handle.wait_for_action(|action| {
                        dispatch_action(&app, &store, action, &item_id);
                    }),
                    Err(e) => logging::log(&format!("myday: 发送通知失败: {e}")),
                }
            });
        emit_reminder_fired(&self.app, reminder, item);
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let lang = Lang::from_store(&self.store);
        let s = strings(lang);
        let body = missed_body(lang, count, lines);
        if let Err(e) = notify_rust::Notification::new()
            .appname("MyDay")
            .summary(&s.missed_summary.replace("{n}", &count.to_string()))
            .body(&body)
            .show()
        {
            logging::log(&format!("myday: 发送错过摘要失败: {e}"));
        }
    }
}

/// 通知按钮 / 通知体点击的分发（SPRINT-SPEC §1.2）。
/// complete / snooze 直接走 Store（与 CLI `item complete|snooze` 同一函数），
/// default（打开）唤起主窗口并让前端定位到条目详情。
#[cfg(desktop)]
fn dispatch_action(app: &tauri::AppHandle, store: &Store, action: &str, item_id: &str) {
    match action {
        "complete" => {
            if store.complete_task(item_id).is_ok() {
                emit_changed(app);
            }
        }
        "snooze" => {
            let until = chrono::Utc::now() + chrono::Duration::minutes(10);
            if store.snooze(item_id, until).is_ok() {
                emit_changed(app);
            }
        }
        "default" | "open" => {
            crate::show_main(app);
            let _ =
                tauri::Emitter::emit(app, "reminder-fired", serde_json::json!({ "id": item_id }));
        }
        // "closed"（无操作关闭）等其余动作不处理
        _ => {}
    }
}

#[cfg(desktop)]
fn emit_changed(app: &tauri::AppHandle) {
    let _ = tauri::Emitter::emit(app, "data-changed", serde_json::json!({}));
}

// ----------------------------------------------------------------------
// 移动：tauri-plugin-notification（无动作按钮，点击即唤起 App）
// ----------------------------------------------------------------------

#[cfg(mobile)]
struct MobileNotifier {
    app: tauri::AppHandle,
    store: Arc<Store>,
}

#[cfg(mobile)]
impl Notifier for MobileNotifier {
    fn notify(
        &self,
        reminder: &Reminder,
        item: &Item,
        at: chrono::DateTime<chrono::Utc>,
        _actions: &[&'static str],
    ) {
        let (summary, body) = render(Lang::from_store(&self.store), item, at);
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(summary)
            .body(body)
            .show()
        {
            logging::log(&format!("myday: 发送通知失败: {e}"));
        }
        emit_reminder_fired(&self.app, reminder, item);
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let lang = Lang::from_store(&self.store);
        let s = strings(lang);
        let body = missed_body(lang, count, lines);
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(s.missed_summary.replace("{n}", &count.to_string()))
            .body(body)
            .show()
        {
            logging::log(&format!("myday: 发送错过摘要失败: {e}"));
        }
    }
}

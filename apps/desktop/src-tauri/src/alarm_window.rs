//! 桌面闹钟弹窗（提醒档位 = alarm 的呈现层，需求 §9 桌面侧）。
//!
//! 通知档走系统通知；闹钟档「无法错过」：置顶弹窗 + 前端 WebAudio 循环提示音 +
//! 紧急（critical）系统通知兜底（reminder_loop 侧）。弹窗由 Rust 侧按需创建
//! （label = "alarm"，前端 App.svelte 按 label 路由到 AlarmPopup）。
//!
//! 正在响的闹钟集合由 [`AlarmPending`] 持有（managed state）：
//! 触发时入集合 + 建窗 + 推 `alarm-ringing`；弹窗就绪后主动拉全量
//! （`alarm_pending` 命令，防建窗与事件竞态）；动作经 `alarm_dismiss` 回流
//! （完成 / 稍后 / 打开 / 忽略），集合清空即关窗，声音随之停止。

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::i18n::{strings, Lang};
use crate::logging;
use crate::AppState;
use myday_core::Store;

/// 一条正在响的闹钟（弹窗展示 + 动作分发的最小载荷）。
/// key = "{reminder_id}-{at_millis}"，与移动端 AlarmSpec 同构同 key 语义。
#[derive(Debug, Clone, Serialize)]
pub struct AlarmEntry {
    pub key: String,
    pub item_id: String,
    pub title: String,
    pub body: String,
}

/// 正在响的闹钟集合（managed；空 = 无闹钟，弹窗应关闭）。
#[derive(Default)]
pub struct AlarmPending(pub Mutex<Vec<AlarmEntry>>);

fn store_of(app: &AppHandle) -> Arc<Store> {
    app.state::<AppState>().store.clone()
}

/// 闹钟触发：入集合、确保弹窗打开、推事件（弹窗未就绪也不丢，ready 后会拉全量）。
pub fn ring(app: &AppHandle, entry: AlarmEntry) {
    {
        let state = app.state::<AlarmPending>();
        let mut list = state.0.lock().expect("alarm pending poisoned");
        if list.iter().any(|e| e.key == entry.key) {
            return;
        }
        list.push(entry);
    }
    if let Err(e) = ensure_window(app) {
        logging::log(&format!("myday: 闹钟弹窗创建失败: {e}"));
    }
    // 全量推送：弹窗侧直接替换列表，无需自行合并
    emit_pending(app);
}

fn emit_pending(app: &AppHandle) {
    let snapshot = {
        let state = app.state::<AlarmPending>();
        let list = state.0.lock().expect("alarm pending poisoned");
        list.clone()
    };
    let _ = app.emit("alarm-ringing", &snapshot);
}

/// 弹窗已存在则仅聚焦；不存在则创建（前端按 label 渲染 AlarmPopup 组件）。
fn ensure_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(win) = app.get_webview_window("alarm") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }
    let title = strings(Lang::from_store(&store_of(app))).alarm_title;
    WebviewWindowBuilder::new(app, "alarm", WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(420.0, 200.0)
        .min_inner_size(360.0, 160.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(true)
        .center()
        .build()?;
    Ok(())
}

/// 动作分发（弹窗按钮）：complete / snooze 直接走 Store（与桌面通知动作同一套），
/// open 唤起主窗口定位，dismiss 仅移除。重复 key 幂等。
pub fn dispatch(app: &AppHandle, key: &str, action: &str) {
    let entry = {
        let state = app.state::<AlarmPending>();
        let mut list = state.0.lock().expect("alarm pending poisoned");
        list.iter().position(|e| e.key == key).map(|i| list.remove(i))
    };
    let Some(entry) = entry else { return };

    let item_id = entry.item_id.clone();
    match action {
        "complete" => {
            if store_of(app).complete_task(&item_id).is_ok() {
                let _ = app.emit("data-changed", serde_json::json!({}));
            }
        }
        "snooze" => {
            let until = chrono::Utc::now() + chrono::Duration::minutes(10);
            if store_of(app).snooze(&item_id, until).is_ok() {
                let _ = app.emit("data-changed", serde_json::json!({}));
            }
        }
        "open" => {
            crate::show_main(app);
            let _ = app.emit("reminder-fired", serde_json::json!({ "id": item_id }));
        }
        // "dismiss" 及未知动作：仅移除
        _ => {}
    }

    let now_empty = {
        let state = app.state::<AlarmPending>();
        let list = state.0.lock().expect("alarm pending poisoned");
        list.is_empty()
    };
    if now_empty {
        // 全部处理完：关窗停止提示音（close 会销毁窗口）
        if let Some(win) = app.get_webview_window("alarm") {
            let _ = win.close();
        }
    } else {
        emit_pending(app);
    }
}

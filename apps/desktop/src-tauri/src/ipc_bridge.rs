//! GUI 侧 IPC 处理器：接收 CLI 请求（需求 §2.4、§9.3）。
//!
//! 写操作由本进程的 Store 执行（单一写入口），
//! 完成后向前端广播 `data-changed`，各视图自行刷新 —— 对应验收 11。

use std::sync::Arc;

use serde_json::json;
use tauri::{Emitter, Manager};

use myday_core::ipc::{IpcHandler, IpcRequest, IpcResponse};
use myday_core::model::ItemType;
use myday_core::Store;

pub struct IpcBridge {
    pub app: tauri::AppHandle,
    pub store: Arc<Store>,
}

/// 弹出快速添加窗口并预填（GNOME 自定义快捷键 → `myday quick-add` 路径）。
pub fn show_quick_add(app: &tauri::AppHandle, item_type: Option<ItemType>, title: Option<String>) {
    if let Some(win) = app.get_webview_window("quick-add") {
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit(
            "quick-add",
            json!({
                // 无类型时不预填（面板按内容推断，避免随手记被预设开始变成日程）
                "type": item_type.map(|t| t.as_str()),
                "title": title,
            }),
        );
    }
}

impl IpcHandler for IpcBridge {
    fn handle(&self, req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::Ping => IpcResponse::ok(json!({
                "pong": true,
                "version": env!("CARGO_PKG_VERSION"),
            })),

            IpcRequest::ShowQuickAdd { item_type, title } => {
                show_quick_add(&self.app, item_type, title);
                IpcResponse::ok(json!({ "shown": true }))
            }

            IpcRequest::AddItem { new } => match self.store.add_item(new) {
                Ok(item) => {
                    broadcast_changed(&self.app);
                    IpcResponse::ok(serde_json::to_value(&item).unwrap_or(serde_json::Value::Null))
                }
                Err(e) => IpcResponse::from_error(&e),
            },

            IpcRequest::UpdateItem { id, patch } => match self.store.update_item(&id, patch) {
                Ok(item) => {
                    broadcast_changed(&self.app);
                    IpcResponse::ok(serde_json::to_value(&item).unwrap_or(serde_json::Value::Null))
                }
                Err(e) => IpcResponse::from_error(&e),
            },

            IpcRequest::DeleteItem { id } => match self.store.delete_item(&id) {
                Ok(item) => {
                    broadcast_changed(&self.app);
                    IpcResponse::ok(serde_json::to_value(&item).unwrap_or(serde_json::Value::Null))
                }
                Err(e) => IpcResponse::from_error(&e),
            },

            IpcRequest::CompleteTask { id } => match self.store.complete_task(&id) {
                Ok(item) => {
                    broadcast_changed(&self.app);
                    IpcResponse::ok(serde_json::to_value(&item).unwrap_or(serde_json::Value::Null))
                }
                Err(e) => IpcResponse::from_error(&e),
            },

            IpcRequest::Snooze { id, until } => match self.store.snooze(&id, until) {
                Ok(item) => {
                    broadcast_changed(&self.app);
                    IpcResponse::ok(serde_json::to_value(&item).unwrap_or(serde_json::Value::Null))
                }
                Err(e) => IpcResponse::from_error(&e),
            },

            // 类型转换（SPRINT2-SPEC §7）：task→event 替换，event→log 保留原日程
            IpcRequest::ConvertItem { id, to } => {
                let result = match to.as_str() {
                    "event" => self.store.convert_task_to_event(&id),
                    "log" => self.store.event_to_log(&id),
                    other => Err(myday_core::MyDayError::Invalid(format!(
                        "未知转换目标: {other}（可用 event / log）"
                    ))),
                };
                match result {
                    Ok(item) => {
                        broadcast_changed(&self.app);
                        IpcResponse::ok(
                            serde_json::to_value(&item).unwrap_or(serde_json::Value::Null),
                        )
                    }
                    Err(e) => IpcResponse::from_error(&e),
                }
            }

            // CLI 在 GUI 未运行时直写过的库，GUI 收到通知后刷新（验收 12）
            IpcRequest::Refresh => {
                broadcast_changed(&self.app);
                IpcResponse::ok(json!({ "refreshed": true }))
            }

            // 通知点击 / 定位条目：打开主窗口并让前端导航
            IpcRequest::Reveal { id } => {
                crate::show_main(&self.app);
                let _ = self.app.emit("reveal-item", json!({ "id": id }));
                IpcResponse::ok(json!({ "revealed": id }))
            }
        }
    }
}

fn broadcast_changed(app: &tauri::AppHandle) {
    // 发给所有窗口；空 payload，各视图自行决定重新拉取
    let _ = app.emit("data-changed", json!({}));
}

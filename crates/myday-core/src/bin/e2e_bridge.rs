//! E2E / agent 桥（测试基建，随 GUI 发布无害）：把 [`Store`] 的全部读写
//! 以「一条 HTTP POST = 一个命令」的方式暴露给浏览器端 E2E（Playwright mock
//! `__TAURI_INTERNALS__.invoke` → 本桥），使 UI 测试跑在真实 core + 真实
//! SQLite 上。零新增依赖：手写 HTTP 解析（Content-Length + body）。
//!
//! 启动：`myday_core_e2e_bridge <port>`（MYDAY_DATA_DIR 等环境变量照常生效）。
//! 请求：`POST / {"cmd":"add_item","args":{...}}`；响应：`{"ok":true,"data":...}`
//! 或 `{"ok":false,"error":{"code","message"}}`——与 CLI 信封一致。
//!
//! 命令面与 apps/desktop 的 Tauri commands 一一对应（camelCase 参数在本层
//! 与 snake_case 间做映射）。

use std::io::{Read, Write};
use std::sync::Arc;

use myday_core::model::*;
use myday_core::store::{ListFilter, Store, TaskView};

fn arg_str(args: &serde_json::Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| args.get(*n).and_then(|v| v.as_str()))
        .map(str::to_string)
}

fn arg_opt_time(args: &serde_json::Value, name: &str) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    match args.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(s) => s
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&chrono::Utc))
            .map(Some)
            .ok_or_else(|| format!("bad time for {name}: {s}")),
    }
}

fn arg_time(args: &serde_json::Value, name: &str) -> Result<chrono::DateTime<chrono::Utc>, String> {
    arg_opt_time(args, name)?.ok_or_else(|| format!("missing {name}"))
}

fn type_of(s: &str) -> Option<ItemType> {
    ItemType::parse(s)
}

fn handle(store: &Store, cmd: &str, args: &serde_json::Value) -> Result<serde_json::Value, String> {
    let v = |k: &str| args.get(k);
    match cmd {
        "list_items" => {
            let filter: ListFilter =
                serde_json::from_value(v("filter").cloned().unwrap_or(serde_json::json!({})))
                    .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(store.list_items(&filter).map_err(e10s)?).unwrap())
        }
        "list_items_window" => Ok(serde_json::to_value(
            store
                .list_items_window(
                    arg_time(args, "from")?,
                    arg_time(args, "to")?,
                    v("item_type").and_then(|t| t.as_str()).and_then(type_of),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "tasks_view" => {
            let view = match v("view").and_then(|s| s.as_str()) {
                Some("today") => TaskView::Today,
                Some("upcoming") => TaskView::Upcoming,
                Some("all") => TaskView::All,
                _ => TaskView::Done,
            };
            Ok(serde_json::to_value(store.tasks_view(view, None).map_err(e10s)?).unwrap())
        }
        "get_item" => Ok(serde_json::to_value(store.get_item(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?).unwrap()),
        "add_item" => {
            let new: NewItem = serde_json::from_value(v("new").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(store.add_item(new).map_err(e10s)?).unwrap())
        }
        "update_item" => {
            let patch: ItemPatch =
                serde_json::from_value(v("patch").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(store.update_item(&arg_str(args, &["id"]).ok_or("missing id")?, patch).map_err(e10s)?).unwrap())
        }
        "delete_item" => Ok(serde_json::to_value(store.delete_item(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?).unwrap()),
        "complete_task" => Ok(serde_json::to_value(store.complete_task(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?).unwrap()),
        "snooze" => Ok(serde_json::to_value(
            store
                .snooze(&arg_str(args, &["id"]).ok_or("missing id")?, arg_time(args, "until")?)
                .map_err(e10s)?,
        )
        .unwrap()),
        "search_items" => Ok(serde_json::to_value(
            store
                .search(&arg_str(args, &["query"]).unwrap_or_default(), v("itemType").and_then(|s| s.as_str()).and_then(type_of))
                .map_err(e10s)?,
        )
        .unwrap()),
        "list_templates" => Ok(serde_json::to_value(store.list_templates().map_err(e10s)?).unwrap()),
        "add_template" => Ok(serde_json::to_value(
            store
                .add_template(
                    &arg_str(args, &["name"]).ok_or("missing name")?,
                    v("tag").and_then(|s| s.as_str()),
                    v("icon").and_then(|s| s.as_str()),
                    v("itemType").and_then(|s| s.as_str()).and_then(type_of).unwrap_or(ItemType::Log),
                    v("defaults").unwrap_or(&serde_json::json!({})),
                    v("fields").unwrap_or(&serde_json::json!([])),
                    v("note").and_then(|s| s.as_str()),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "update_template" => Ok(serde_json::to_value(
            store
                .update_template(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    &arg_str(args, &["name"]).ok_or("missing name")?,
                    v("tag").and_then(|s| s.as_str()),
                    v("icon").and_then(|s| s.as_str()),
                    v("itemType").and_then(|s| s.as_str()).and_then(type_of).unwrap_or(ItemType::Log),
                    v("defaults").unwrap_or(&serde_json::json!({})),
                    v("fields").unwrap_or(&serde_json::json!([])),
                    v("note").and_then(|s| s.as_str()),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "delete_template" => Ok(serde_json::to_value(store.delete_template(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?).unwrap()),
        "move_template" => {
            store
                .move_template(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    v("up").and_then(|b| b.as_bool()).unwrap_or(false),
                )
                .map_err(e10s)?;
            Ok(serde_json::json!(null))
        }
        "set_template_pinned" => {
            store
                .set_template_pinned(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    v("pinned").and_then(|b| b.as_bool()).unwrap_or(true),
                )
                .map_err(e10s)?;
            Ok(serde_json::json!(null))
        }
        "list_field_defs" => Ok(serde_json::to_value(
            store
                .list_field_defs(v("scope").and_then(|s| s.as_str()).and_then(type_of))
                .map_err(e10s)?,
        )
        .unwrap()),
        "add_field_def" => {
            let kind = FieldKind::parse(&arg_str(args, &["kind"]).unwrap_or_default())
                .ok_or("bad kind")?;
            Ok(serde_json::to_value(
                store
                    .add_field_def(
                        &arg_str(args, &["name"]).ok_or("missing name")?,
                        kind,
                        v("options").unwrap_or(&serde_json::json!({})),
                        v("scope").and_then(|s| s.as_str()).and_then(type_of),
                    )
                    .map_err(e10s)?,
            )
            .unwrap())
        }
        "update_field_def" => {
            let name = v("name").and_then(|s| s.as_str());
            let options = v("options");
            Ok(serde_json::to_value(
                store
                    .update_field_def(
                        &arg_str(args, &["id"]).ok_or("missing id")?,
                        name,
                        options,
                        v("sort").and_then(|n| n.as_i64()),
                    )
                    .map_err(e10s)?,
            )
            .unwrap())
        }
        "delete_field_def" => Ok(serde_json::to_value(store.delete_field_def(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?).unwrap()),
        "count_items_with_field" => Ok(serde_json::json!(
            store.count_items_with_field(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?
        )),
        "list_deleted_field_defs" => Ok(serde_json::to_value(store.list_deleted_field_defs().map_err(e10s)?).unwrap()),
        "purge_deleted_field_defs" => Ok(serde_json::json!(
            store.purge_deleted_field_defs().map_err(e10s)?
        )),
        "add_attachment_b64" => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(arg_str(args, &["dataB64", "data_b64"]).unwrap_or_default().as_bytes())
                .map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(
                store
                    .add_attachment_bytes(
                        &arg_str(args, &["itemId", "item_id"]).ok_or("missing itemId")?,
                        &bytes,
                        &arg_str(args, &["ext"]).unwrap_or_else(|| "png".into()),
                    )
                    .map_err(e10s)?,
            )
            .unwrap())
        }
        "get_setting" => Ok(serde_json::to_value(store.get_setting(&arg_str(args, &["key"]).ok_or("missing key")?).map_err(e10s)?).unwrap()),
        "set_setting" => {
            store
                .set_setting(
                    &arg_str(args, &["key"]).ok_or("missing key")?,
                    &arg_str(args, &["value"]).unwrap_or_default(),
                )
                .map_err(e10s)?;
            Ok(serde_json::json!(null))
        }
        "check_conflict" => Ok(serde_json::to_value(
            store
                .conflicting_events(
                    arg_time(args, "start")?,
                    arg_time(args, "end")?,
                    arg_str(args, &["excludeId", "exclude_id"]).as_deref(),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "stats_summary" => Ok(serde_json::to_value(store.stats_summary(365).map_err(e10s)?).unwrap()),
        // 视图模型（FILTER-SPEC §12）：camelCase 参数与 Tauri 命令一一对应
        "view_list" => {
            let panel = v("panel").and_then(|s| s.as_str()).and_then(myday_core::view::Panel::parse);
            Ok(serde_json::to_value(store.list_views(panel).map_err(e10s)?).unwrap())
        }
        "view_get" => Ok(serde_json::to_value(
            store.get_view(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?,
        )
        .unwrap()),
        "view_create" => {
            let panel = myday_core::view::Panel::parse(&arg_str(args, &["panel"]).unwrap_or_default())
                .ok_or("bad panel")?;
            Ok(serde_json::to_value(
                store
                    .create_view(
                        &arg_str(args, &["name"]).ok_or("missing name")?,
                        panel,
                        v("config").unwrap_or(&serde_json::json!({})),
                    )
                    .map_err(e10s)?,
            )
            .unwrap())
        }
        "view_save" => Ok(serde_json::to_value(
            store
                .save_view(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    v("name").and_then(|s| s.as_str()),
                    v("config").unwrap_or(&serde_json::json!({})),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "view_delete" => Ok(serde_json::to_value(
            store.delete_view(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?,
        )
        .unwrap()),
        "view_reset" => Ok(serde_json::to_value(
            store.reset_view(&arg_str(args, &["id"]).ok_or("missing id")?).map_err(e10s)?,
        )
        .unwrap()),
        "view_duplicate" => Ok(serde_json::to_value(
            store
                .duplicate_view(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    &arg_str(args, &["name"]).ok_or("missing name")?,
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "query_view" => Ok(serde_json::to_value(
            store
                .query_view(
                    &arg_str(args, &["id"]).ok_or("missing id")?,
                    v("keyword").and_then(|s| s.as_str()),
                    v("limit").and_then(|n| n.as_i64()),
                )
                .map_err(e10s)?,
        )
        .unwrap()),
        "query_stats_page" => Ok(serde_json::to_value(
            store.query_stats_page().map_err(e10s)?,
        )
        .unwrap()),
        "stats_restore_defaults" => Ok(serde_json::to_value(
            store.restore_stats_defaults().map_err(e10s)?,
        )
        .unwrap()),
        "app_info" => Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "data_root": store.data_root().to_string_lossy(),
            "socket_path": "e2e-bridge",
            "backend": "x11"
        })),
        // 今日悬浮窗（OVERLAY-SPEC）：真实 core 求值（本地日 = 桥进程的今天）
        "overlay_today" => Ok(serde_json::to_value(
            myday_core::overlay::today_overlay(&store, chrono::Local::now().date_naive())
                .map_err(e10s)?,
        )
        .unwrap()),
        "open_quick_add" => Ok(serde_json::json!(null)), // 前端 mock 落为页面内面板
        other => Err(format!("unknown cmd: {other}")),
    }
}

fn e10s(e: myday_core::MyDayError) -> String {
    // 与 apps/desktop err_string 保持一致：展示层拿 Display 原文，不加内部码前缀
    e.to_string()
}

fn main() {
    let port: u16 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1429);
    let store = match myday_core::Store::open_default() {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("e2e-bridge: 打开数据库失败: {e}");
            std::process::exit(1);
        }
    };
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("bind");
    eprintln!("e2e-bridge: listening on 127.0.0.1:{port}");
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let store = store.clone();
        std::thread::spawn(move || {
            // 读头部拿 Content-Length，再读 body
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let mut header_end = None;
            loop {
                let Ok(n) = stream.read(&mut tmp) else { break };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&tmp[..n]);
                if header_end.is_none() {
                    if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                    }
                }
                if let Some(hs) = header_end {
                    let head = String::from_utf8_lossy(&buf[..hs]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if buf.len() >= hs + len {
                        // CORS：页面源（vite preview）与本桥端口不同，需放行预检与响应头
                        const CORS: &str = "Access-Control-Allow-Origin: *\r\n\
                            Access-Control-Allow-Methods: POST, OPTIONS\r\n\
                            Access-Control-Allow-Headers: Content-Type\r\n";
                        let resp = if head.starts_with("options") {
                            format!(
                                "HTTP/1.1 204 No Content\r\n{CORS}Content-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                        } else {
                            let body = String::from_utf8_lossy(&buf[hs..hs + len]).to_string();
                            let out = route(&store, &body);
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n{CORS}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                                out.len(),
                                out
                            )
                        };
                        let _ = stream.write_all(resp.as_bytes());
                        let _ = stream.flush();
                        return;
                    }
                }
            }
        });
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn route(store: &Arc<Store>, body: &str) -> String {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return envelope(Err(format!("bad json: {e}"))),
    };
    let cmd = parsed.get("cmd").and_then(|c| c.as_str()).unwrap_or("").to_string();
    let args = parsed.get("args").cloned().unwrap_or(serde_json::json!({}));
    let is_mutation = matches!(
        cmd.as_str(),
        "add_item" | "update_item" | "delete_item" | "complete_task" | "snooze"
            | "add_template" | "update_template" | "delete_template" | "move_template"
            | "set_template_pinned" | "add_field_def" | "update_field_def" | "delete_field_def"
            | "purge_deleted_field_defs"
            | "set_setting" | "add_attachment_b64"
    );
    let out = handle(store, &cmd, &args).map(|mut data| {
        if is_mutation {
            // 与 GUI 行为一致：变更后标记，前端 mock 会据此触发 data-changed 刷新
            if let Some(obj) = data.as_object_mut() {
                obj.insert("__changed".into(), serde_json::json!(true));
            }
        }
        data
    });
    envelope(out)
}

fn envelope(res: Result<serde_json::Value, String>) -> String {
    match res {
        Ok(data) => serde_json::json!({ "ok": true, "data": data }).to_string(),
        Err(message) => {
            let (code, message) = match message.split_once(']') {
                Some((c, m)) if c.starts_with('[') => (c[1..].to_string(), m.trim().to_string()),
                _ => ("INTERNAL".to_string(), message),
            };
            serde_json::json!({ "ok": false, "error": { "code": code, "message": message } }).to_string()
        }
    }
}

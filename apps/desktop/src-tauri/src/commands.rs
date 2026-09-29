//! Tauri 前端可调用的命令。全部直接委托给 [`myday_core::Store`]，
//! 保证 GUI / CLI / agent 三端行为一致（需求 §9.3）。
//! 命令错误统一为 `[CODE] message` 字符串，前端提示展示。

use base64::Engine;
use std::path::{Path, PathBuf};
#[cfg(all(desktop, target_os = "windows"))]
use std::process::Command;
#[cfg(all(desktop, not(target_os = "windows")))]
use std::process::{Command, Stdio};
#[cfg(desktop)]
use tauri::Manager;
use tauri::State;

use myday_core::model::*;
#[cfg(desktop)]
use myday_core::store::Store;
use myday_core::store::{ListFilter, TaskView};

use crate::AppState;

fn err_string(e: myday_core::MyDayError) -> String {
    e.to_string()
}

/// GUI 本进程内的变更统一广播，各窗口视图据此刷新（与 IPC 桥同频道）。
fn notify_changed(app: &tauri::AppHandle) {
    use tauri::Emitter;
    let _ = app.emit("data-changed", serde_json::json!({}));
}

#[tauri::command]
pub fn list_items(
    state: State<'_, AppState>,
    filter: ListFilter,
) -> std::result::Result<Vec<Item>, String> {
    state.store.list_items(&filter).map_err(err_string)
}

/// 窗口查询（日历）：与 [from, to] 时间相关的全部条目（OR 语义，跨天事件两头都算）。
#[tauri::command]
pub fn list_items_window(
    state: State<'_, AppState>,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    item_type: Option<ItemType>,
) -> std::result::Result<Vec<Item>, String> {
    state
        .store
        .list_items_window(from, to, item_type)
        .map_err(err_string)
}

#[tauri::command]
pub fn tasks_view(
    state: State<'_, AppState>,
    view: String,
    today: Option<chrono::NaiveDate>,
) -> std::result::Result<Vec<Item>, String> {
    let view = match view.as_str() {
        "today" => TaskView::Today,
        "upcoming" => TaskView::Upcoming,
        "all" => TaskView::All,
        "done" => TaskView::Done,
        other => return Err(format!("[INVALID] 未知视图: {other}")),
    };
    state.store.tasks_view(view, today).map_err(err_string)
}

#[tauri::command]
pub fn get_item(state: State<'_, AppState>, id: String) -> std::result::Result<Item, String> {
    state.store.get_item(&id).map_err(err_string)
}

#[tauri::command]
pub fn add_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    new: NewItem,
) -> std::result::Result<Item, String> {
    let item = state.store.add_item(new).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn update_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    patch: ItemPatch,
) -> std::result::Result<Item, String> {
    let item = state.store.update_item(&id, patch).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn delete_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.delete_item(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

// ----------------------------------------------------------------------
// 回收站：软删条目的恢复 / 彻底删除 / 清空
// ----------------------------------------------------------------------

#[tauri::command]
pub fn list_trash(state: State<'_, AppState>) -> std::result::Result<Vec<Item>, String> {
    state.store.list_trash().map_err(err_string)
}

#[tauri::command]
pub fn restore_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.restore_item(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

/// 重复条目单次例外：拆某次发生为独立条目（原系列记入例外，展开时跳过该期）。
#[tauri::command]
pub fn detach_occurrence(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    at: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<Item, String> {
    let item = state.store.detach_occurrence(&id, at).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

/// 重复条目单次例外：仅删除某一次发生。
#[tauri::command]
pub fn skip_occurrence(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    at: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<Item, String> {
    let item = state.store.skip_occurrence(&id, at).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

/// 单次例外的撤销：移除例外锚点，该期回到系列正常展开（「只改这一期」的 undo 用）。
#[tauri::command]
pub fn remove_occurrence_exdate(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    at: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<Item, String> {
    let item = state
        .store
        .remove_occurrence_exdate(&id, at)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn purge_item(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.purge_item(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn empty_trash(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<usize, String> {
    let n = state.store.empty_trash().map_err(err_string)?;
    if n > 0 {
        notify_changed(&app);
    }
    Ok(n)
}

/// 导入 ICS 文件（本地一次性导入），返回导入报告。
#[tauri::command]
pub fn import_ics(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> std::result::Result<myday_core::ics::ImportReport, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("[IO] {e}"))?;
    let report = myday_core::ics::import_ics(&state.store, &text).map_err(err_string)?;
    if report.events + report.tasks > 0 {
        notify_changed(&app);
    }
    Ok(report)
}

#[tauri::command]
pub fn complete_task(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.complete_task(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn snooze(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    until: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<Item, String> {
    let item = state.store.snooze(&id, until).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

#[tauri::command]
pub fn search_items(
    state: State<'_, AppState>,
    query: String,
    item_type: Option<ItemType>,
) -> std::result::Result<Vec<SearchHit>, String> {
    state.store.search(&query, item_type).map_err(err_string)
}

#[tauri::command]
pub fn list_templates(state: State<'_, AppState>) -> std::result::Result<Vec<Template>, String> {
    state.store.list_templates().map_err(err_string)
}

/// 新建模板（记录页按钮可增）：名称 + 标签 + 图标 + 归属类型 + 字段默认值 + 说明。
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri 命令参数须平铺，无法收敛为结构体
pub fn add_template(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    tag: Option<String>,
    icon: Option<String>,
    item_type: Option<String>,
    defaults: serde_json::Value,
    fields: serde_json::Value,
    note: Option<String>,
) -> std::result::Result<Template, String> {
    let item_type = item_type
        .as_deref()
        .and_then(ItemType::parse)
        .unwrap_or(ItemType::Log);
    let tpl = state
        .store
        .add_template(
            &name,
            tag.as_deref(),
            icon.as_deref(),
            item_type,
            &defaults,
            &fields,
            note.as_deref(),
        )
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(tpl)
}

/// 更新模板（可编辑字段整体替换）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn update_template(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
    tag: Option<String>,
    icon: Option<String>,
    item_type: Option<String>,
    defaults: serde_json::Value,
    fields: serde_json::Value,
    note: Option<String>,
) -> std::result::Result<Template, String> {
    let item_type = item_type
        .as_deref()
        .and_then(ItemType::parse)
        .unwrap_or(ItemType::Log);
    let tpl = state
        .store
        .update_template(
            &id,
            &name,
            tag.as_deref(),
            icon.as_deref(),
            item_type,
            &defaults,
            &fields,
            note.as_deref(),
        )
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(tpl)
}

/// 设置模板是否在记录页显示为按钮（取消按钮不删除模板）。
#[tauri::command]
pub fn set_template_pinned(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> std::result::Result<(), String> {
    state
        .store
        .set_template_pinned(&id, pinned)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(())
}

/// 上移 / 下移模板（设置页模板管理）。
#[tauri::command]
pub fn move_template(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    up: bool,
) -> std::result::Result<(), String> {
    state.store.move_template(&id, up).map_err(err_string)?;
    notify_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_template(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Template, String> {
    let tpl = state.store.delete_template(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(tpl)
}

/// 字段定义列表：全局 + 指定类型范围。
#[tauri::command]
pub fn list_field_defs(
    state: State<'_, AppState>,
    scope: Option<ItemType>,
) -> std::result::Result<Vec<FieldDef>, String> {
    state.store.list_field_defs(scope).map_err(err_string)
}

#[tauri::command]
pub fn add_field_def(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    kind: String,
    options: serde_json::Value,
    scope: Option<ItemType>,
) -> std::result::Result<FieldDef, String> {
    let kind = FieldKind::parse(&kind).ok_or_else(|| format!("[INVALID] 未知字段类型: {kind}"))?;
    let def = state
        .store
        .add_field_def(&name, kind, &options, scope)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(def)
}

/// 修改字段：改名（条目上的值随键迁移）/ 改 options / 改排序。
#[tauri::command]
pub fn update_field_def(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    options: Option<serde_json::Value>,
    sort: Option<i64>,
) -> std::result::Result<FieldDef, String> {
    let def = state
        .store
        .update_field_def(&id, name.as_deref(), options.as_ref(), sort)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(def)
}

#[tauri::command]
pub fn delete_field_def(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<FieldDef, String> {
    let def = state.store.delete_field_def(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(def)
}

/// 删除字段前的确认提示：多少条目带着这个字段的值（按字段 id）。
#[tauri::command]
pub fn count_items_with_field(
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<i64, String> {
    state.store.count_items_with_field(&id).map_err(err_string)
}

/// 已软删的字段定义（展示层：旧条目带出历史值的字段名）。
#[tauri::command]
pub fn list_deleted_field_defs(
    state: State<'_, AppState>,
) -> std::result::Result<Vec<FieldDef>, String> {
    state.store.list_deleted_field_defs().map_err(err_string)
}

/// 彻底清理全部软删字段（不可逆）。返回清理的个数。
#[tauri::command]
pub fn purge_deleted_field_defs(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<i64, String> {
    let n = state.store.purge_deleted_field_defs().map_err(err_string)?;
    notify_changed(&app);
    Ok(n)
}

/// 粘贴图片附件：前端传 base64（需求 §4）。
#[tauri::command]
pub fn add_attachment_b64(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    item_id: String,
    data_b64: String,
    ext: String,
) -> std::result::Result<Attachment, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| format!("[INVALID] base64 解码失败: {e}"))?;
    let att = state
        .store
        .add_attachment_bytes(&item_id, &bytes, &ext)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(att)
}

/// 附件绝对路径（前端用 convertFileSrc 转为 asset: URL 预览）。
#[tauri::command]
pub fn attachment_abs_path(
    state: State<'_, AppState>,
    rel_path: String,
) -> std::result::Result<String, String> {
    let abs = state.store.data_root().join(rel_path);
    Ok(abs.to_string_lossy().into_owned())
}

/// 统计汇总（SPRINT-SPEC §6 / SPRINT2-SPEC §8）：热力图 + 模板连续 + 数值趋势。
/// `days` = 统计窗口天数（缺省 365，实际生效 7–1095）。
/// P3 起由视图挂件求值实现（FILTER-SPEC §12：固定 seed 配置，输出信封不变）。
#[tauri::command]
pub fn stats_summary(
    state: State<'_, AppState>,
    days: Option<i64>,
) -> std::result::Result<serde_json::Value, String> {
    let s = state
        .store
        .stats_summary(days.unwrap_or(365))
        .map_err(err_string)?;
    serde_json::to_value(s).map_err(|e| format!("[INTERNAL] {e}"))
}

// ----------------------------------------------------------------------
// 视图模型（FILTER-SPEC §12）：CRUD 广播 data-changed，前端按 panel 订阅刷新
// ----------------------------------------------------------------------

#[tauri::command]
pub fn view_list(
    state: State<'_, AppState>,
    panel: Option<String>,
) -> std::result::Result<Vec<myday_core::view::ViewDef>, String> {
    let panel = panel
        .as_deref()
        .map(myday_core::view::Panel::parse)
        .map(|p| {
            p.ok_or_else(|| "[INVALID] 未知面板（可用 logs / tasks / search / stats）".to_string())
        })
        .transpose()?;
    state.store.list_views(panel).map_err(err_string)
}

#[tauri::command]
pub fn view_get(
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    state.store.get_view(&id).map_err(err_string)
}

#[tauri::command]
pub fn view_create(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    name: String,
    panel: String,
    config: serde_json::Value,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    let panel = myday_core::view::Panel::parse(&panel)
        .ok_or_else(|| format!("[INVALID] 未知面板: {panel}"))?;
    let view = state
        .store
        .create_view(&name, panel, &config)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(view)
}

/// 保存：内置 id = 写 config_user（定制）；用户 id = 覆盖 config。可脚本化。
#[tauri::command]
pub fn view_save(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    config: serde_json::Value,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    let view = state
        .store
        .save_view(&id, name.as_deref(), &config)
        .map_err(err_string)?;
    notify_changed(&app);
    Ok(view)
}

#[tauri::command]
pub fn view_delete(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    let view = state.store.delete_view(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(view)
}

/// 重置内置视图 = 清空 config_user（恢复 seed）。
#[tauri::command]
pub fn view_reset(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    let view = state.store.reset_view(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(view)
}

/// 另存为：从任意视图复制生效配置出一张用户视图。
#[tauri::command]
pub fn view_duplicate(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> std::result::Result<myday_core::view::ViewDef, String> {
    let view = state.store.duplicate_view(&id, &name).map_err(err_string)?;
    notify_changed(&app);
    Ok(view)
}

/// 视图求值：与 CLI `item query --view` 同 id 同结果（同一时刻 + 同一本地时区）。
#[tauri::command]
pub fn query_view(
    state: State<'_, AppState>,
    id: String,
    keyword: Option<String>,
    limit: Option<i64>,
) -> std::result::Result<myday_core::view::ViewResult, String> {
    state
        .store
        .query_view(&id, keyword.as_deref(), limit)
        .map_err(err_string)
}

/// 恢复默认统计页：重新铺缺失的预置容器（按当前数据），返回新建容器名。
#[tauri::command]
pub fn stats_restore_defaults(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<Vec<String>, String> {
    let created = state.store.restore_stats_defaults().map_err(err_string)?;
    if !created.is_empty() {
        notify_changed(&app);
    }
    Ok(created)
}

/// 统计页一次求值：全部 stats 容器展开为挂件流（共享扫描）。
#[tauri::command]
pub fn query_stats_page(
    state: State<'_, AppState>,
) -> std::result::Result<myday_core::view::StatsPageResult, String> {
    state.store.query_stats_page().map_err(err_string)
}

/// 提醒中心：最近已处理提醒（SPRINT2-SPEC §5）。
#[tauri::command]
pub fn reminder_history(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> std::result::Result<Vec<ReminderHistoryEntry>, String> {
    state
        .store
        .reminder_history(limit.unwrap_or(50).clamp(1, 200))
        .map_err(err_string)
}

/// 提醒中心未读数（remind_at > reminder_seen_at）。
#[tauri::command]
pub fn reminder_unread(state: State<'_, AppState>) -> std::result::Result<i64, String> {
    let seen = state
        .store
        .get_setting("reminder_seen_at")
        .map_err(err_string)?
        .and_then(|v| {
            chrono::DateTime::parse_from_rfc3339(&v)
                .map(|d| d.with_timezone(&chrono::Utc))
                .ok()
        });
    state.store.reminder_unread_count(seen).map_err(err_string)
}

/// 打开提醒中心即标记已读，返回写入的时刻。
#[tauri::command]
pub fn mark_reminders_seen(state: State<'_, AppState>) -> std::result::Result<String, String> {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    state
        .store
        .set_setting("reminder_seen_at", &now)
        .map_err(err_string)?;
    Ok(now)
}

/// 桌面：闹钟弹窗当前条目集合（弹窗 webview ready 后拉全量，防建窗与事件竞态）。
#[cfg(desktop)]
#[tauri::command]
pub fn alarm_pending(
    app: tauri::AppHandle,
) -> std::result::Result<Vec<crate::alarm_window::AlarmEntry>, String> {
    use tauri::Manager;
    let entries = {
        let state = app.state::<crate::alarm_window::AlarmPending>();
        let guard = state.0.lock().expect("alarm pending poisoned");
        let v = guard.clone();
        v
    };
    Ok(entries)
}

/// 桌面：闹钟弹窗动作回流（complete / snooze / open / dismiss）。
#[cfg(desktop)]
#[tauri::command]
pub fn alarm_dismiss(app: tauri::AppHandle, key: String, action: String) -> std::result::Result<(), String> {
    crate::alarm_window::dispatch(&app, &key, &action);
    Ok(())
}

/// 移动端：当前应移交系统 AlarmManager 的调度集合 + 需取消的 keys。
/// 前端拉取后调用插件 `sync_alarms`，成功再回报 `alarms_synced`。
#[cfg(mobile)]
#[tauri::command]
pub fn alarms_sync_payload(app: tauri::AppHandle) -> std::result::Result<serde_json::Value, String> {
    use tauri::Manager;
    let payload = app
        .state::<crate::reminder_loop::MobileAlarmSync>()
        .payload_snapshot();
    serde_json::to_value(payload).map_err(|e| e.to_string())
}

/// 移动端：前端回报本次成功写入 AlarmManager 的 keys 全集
/// （提醒循环据此判定「系统已接管」，跳过兜底通知）。
#[cfg(mobile)]
#[tauri::command]
pub fn alarms_synced(app: tauri::AppHandle, keys: Vec<String>) -> std::result::Result<(), String> {
    use tauri::Manager;
    app.state::<crate::reminder_loop::MobileAlarmSync>()
        .confirm(keys);
    Ok(())
}

/// 待办转日程（SPRINT2-SPEC §7）：新建日程承接内容，原待办删除。
#[tauri::command]
pub fn convert_task_to_event(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.convert_task_to_event(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

/// 日程生成记录（SPRINT2-SPEC §7）：复制为新记录，原日程保留。
#[tauri::command]
pub fn event_to_log(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> std::result::Result<Item, String> {
    let item = state.store.event_to_log(&id).map_err(err_string)?;
    notify_changed(&app);
    Ok(item)
}

/// 导出 ICS 到数据目录 exports/，返回文件路径（SPRINT2-SPEC §6）。
#[tauri::command]
pub fn export_ics(state: State<'_, AppState>) -> std::result::Result<String, String> {
    let ics = myday_core::ics::export_ics(&state.store).map_err(err_string)?;
    let dir = state.store.data_root().join("exports");
    std::fs::create_dir_all(&dir).map_err(|e| format!("[IO] {e}"))?;
    let name = format!("myday-{}.ics", chrono::Local::now().format("%Y%m%d-%H%M%S"));
    let path = dir.join(name);
    std::fs::write(&path, ics).map_err(|e| format!("[IO] {e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

/// 一键备份 zip（db + attachments，保留最近 7 份），返回文件路径。
#[tauri::command]
pub fn backup_zip(state: State<'_, AppState>) -> std::result::Result<String, String> {
    let path = myday_core::backup::backup_zip(&state.store).map_err(err_string)?;
    Ok(path.to_string_lossy().into_owned())
}

// ----------------------------------------------------------------------
// 节假日 JSON（SPRINT2-SPEC §4）：数据文件在数据目录，用户导入优先于内置
// ----------------------------------------------------------------------

fn holidays_path(state: &AppState) -> PathBuf {
    state.store.data_root().join("holidays.json")
}

/// 读取用户自定义节假日 JSON；无文件返回 None。
#[tauri::command]
pub fn load_holidays_json(
    state: State<'_, AppState>,
) -> std::result::Result<Option<String>, String> {
    match std::fs::read_to_string(holidays_path(&state)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("[IO] {e}")),
    }
}

/// 保存用户自定义节假日 JSON（仅做 JSON 语法校验；结构校验在前端导入入口）。
#[tauri::command]
pub fn save_holidays_json(
    state: State<'_, AppState>,
    text: String,
) -> std::result::Result<(), String> {
    let _: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("[INVALID] JSON 语法错误: {e}"))?;
    std::fs::write(holidays_path(&state), text).map_err(|e| format!("[IO] {e}"))
}

/// 删除用户自定义节假日 JSON（回退内置数据）；文件不存在视为成功。
#[tauri::command]
pub fn reset_holidays_json(state: State<'_, AppState>) -> std::result::Result<(), String> {
    match std::fs::remove_file(holidays_path(&state)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("[IO] {e}")),
    }
}

/// 冲突检测（需求 §5：提示但不阻止）：与 [start, end] 相交的其他日程。
#[tauri::command]
pub fn check_conflict(
    state: State<'_, AppState>,
    start: chrono::DateTime<chrono::Utc>,
    end: chrono::DateTime<chrono::Utc>,
    exclude_id: Option<String>,
) -> std::result::Result<Vec<Item>, String> {
    state
        .store
        .conflicting_events(start, end, exclude_id.as_deref())
        .map_err(err_string)
}

#[tauri::command]
pub fn get_setting(
    state: State<'_, AppState>,
    key: String,
) -> std::result::Result<Option<String>, String> {
    state.store.get_setting(&key).map_err(err_string)
}

#[tauri::command]
pub fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> std::result::Result<(), String> {
    state.store.set_setting(&key, &value).map_err(err_string)
}

/// 主窗口揭幕前把 webview 底色设为主题背景色：GTK 在 webview 首帧呈现前露出的
/// 就是这层底色（默认白），设成主题 bg 后揭幕不再白闪（THEME-SPEC 防闪色）。
/// 只供主窗口调用（quick-add 等透明窗口须保持透明底色，前端已按 label 拦截）。
#[tauri::command]
pub fn set_window_bg(window: tauri::WebviewWindow, color: String) -> std::result::Result<(), String> {
    let hex = color.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return Err(format!("非法颜色 {color}（须为 #rrggbb）"));
    }
    let v = u32::from_str_radix(hex, 16).map_err(|e| format!("非法颜色 {color}: {e}"))?;
    let c = tauri::window::Color(
        ((v >> 16) & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        (v & 0xff) as u8,
        255,
    );
    window.set_background_color(Some(c)).map_err(|e| e.to_string())
}

/// 应用信息（设置页展示）。移动端无 IPC socket / 显示后端概念，只报版本与数据目录。
#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> serde_json::Value {
    #[cfg(desktop)]
    let mut info = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "data_root": state.store.data_root().to_string_lossy(),
    });
    #[cfg(not(desktop))]
    let info = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "data_root": state.store.data_root().to_string_lossy(),
    });
    #[cfg(desktop)]
    {
        info["socket_path"] =
            serde_json::json!(myday_core::socket_endpoint().unwrap_or_else(|e| e.to_string()));
        // 显示后端（OVERLAY-SPEC §2）：wayland = 原生降级（无置顶/定位），设置页提示用
        info["backend"] = serde_json::json!(if crate::platform::native_wayland() {
            "wayland"
        } else {
            "x11"
        });
    }
    info
}

// ----------------------------------------------------------------------
// 文件链接：条目只记路径（extra["文件"]），不复制文件（桌面专属：
// 依赖系统文件管理器 / xdg-open，移动端无此概念）
// ----------------------------------------------------------------------

/// 用系统默认程序打开文件 / 目录。
#[cfg(desktop)]
#[tauri::command]
pub fn open_file_path(path: String) -> std::result::Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("[NOT_FOUND] 路径不存在或已被移动: {path}"));
    }
    open_with_default(p)
}

/// 路径是否为目录（文件链接 chips 据此区分 📂/📎 展示与打开提示）。
/// 不存在 / 无权限时按文件处理（false），不报错——展示决策不值得打断用户。
#[tauri::command]
pub fn path_is_dir(path: String) -> bool {
    Path::new(&path).is_dir()
}

/// 在文件管理器中定位文件（Windows 用 explorer /select；其余走 FileManager1 标准接口，
/// 不可用时回退为打开所在目录）。
#[cfg(desktop)]
#[tauri::command]
pub fn reveal_file_path(path: String) -> std::result::Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("[NOT_FOUND] 路径不存在或已被移动: {path}"));
    }
    reveal_impl(p)
}

/// 打开日志目录（设置 → 数据与 IPC）。目录可能尚不存在（还没写过日志），
/// 先建出来再打开，保证「打开」一定有落点。
#[cfg(desktop)]
#[tauri::command]
pub fn open_log_dir() -> std::result::Result<(), String> {
    let dir = crate::logging::log_dir().ok_or("[INTERNAL] 日志未初始化")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("[IO] {e}"))?;
    open_with_default(&dir)
}

/// 开机自启当前状态（tauri-plugin-autostart：Linux 写 XDG autostart，
/// Windows 写 HKCU Run 键）。
#[cfg(desktop)]
#[tauri::command]
pub fn get_autostart(app: tauri::AppHandle) -> std::result::Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("[INTERNAL] {e}"))
}

#[cfg(desktop)]
#[tauri::command]
pub fn set_autostart(app: tauri::AppHandle, enable: bool) -> std::result::Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    if enable {
        al.enable().map_err(|e| format!("[INTERNAL] {e}"))
    } else {
        al.disable().map_err(|e| format!("[INTERNAL] {e}"))
    }
}

/// 切换界面语言（设置页）：落库 + 托盘文案 / 副窗口标题即时跟随；
/// 前端自己监听结果重渲染（i18n store 在前端）。移动端只落库，重渲染由前端完成。
#[tauri::command]
pub fn set_ui_lang(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    lang: String,
) -> std::result::Result<(), String> {
    let value = match lang.as_str() {
        "zh" | "en" => lang,
        _ => return Err("[INVALID] lang 必须是 zh / en".into()),
    };
    state
        .store
        .set_setting("ui_lang", &value)
        .map_err(err_string)?;
    #[cfg(desktop)]
    crate::apply_language(&state.store, &app);
    #[cfg(not(desktop))]
    let _ = &app;
    Ok(())
}

#[cfg(all(desktop, not(target_os = "windows")))]
fn file_uri(p: &Path) -> String {
    let mut out = String::from("file://");
    for b in p.as_os_str().to_string_lossy().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(all(desktop, target_os = "windows"))]
fn reveal_impl(p: &Path) -> std::result::Result<(), String> {
    // explorer.exe 对 /select 总是返回非零退出码，spawn 成功即视为已打开。
    let win_path = p.as_os_str().to_string_lossy().replace('/', "\\");
    Command::new("explorer")
        .arg(format!("/select,{win_path}"))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("[IO] 定位文件失败: {e}"))
}

#[cfg(all(desktop, target_os = "windows"))]
fn open_with_default(p: &Path) -> std::result::Result<(), String> {
    // `start` 把第一个带引号的参数当窗口标题，必须先补一个空标题。
    let win_path = p.as_os_str().to_string_lossy().replace('/', "\\");
    Command::new("cmd")
        .args(["/c", "start", ""])
        .arg(win_path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("[IO] 打开失败: {e}"))
}

#[cfg(all(desktop, not(target_os = "windows")))]
fn open_with_default(p: &Path) -> std::result::Result<(), String> {
    Command::new("xdg-open")
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("[IO] 打开失败: {e}"))
}

#[cfg(all(desktop, not(target_os = "windows")))]
fn reveal_impl(p: &Path) -> std::result::Result<(), String> {
    let uri = file_uri(p);
    let shown = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.FileManager1",
            "--object-path",
            "/org/freedesktop/FileManager1",
            "--method",
            "org.freedesktop.FileManager1.ShowItems",
            &format!("[\"{uri}\"]"),
            "",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if shown {
        return Ok(());
    }
    let dir = p.parent().unwrap_or(Path::new("/"));
    Command::new("xdg-open")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("[IO] 打开目录失败: {e}"))
}

/// 主窗口内的快速添加入口：弹出快速窗口并预填。
#[cfg(desktop)]
#[tauri::command]
pub fn open_quick_add(
    app: tauri::AppHandle,
    item_type: Option<String>,
    title: Option<String>,
) -> std::result::Result<(), String> {
    let ty = item_type.as_deref().and_then(ItemType::parse);
    crate::ipc_bridge::show_quick_add(&app, ty, title);
    Ok(())
}

// ----------------------------------------------------------------------
// 今日悬浮窗（OVERLAY-SPEC，桌面专属窗口形态，移动端整段不编译）：求值在
// myday_core::overlay，窗口控制走自定义 Rust 命令（§2.2 决策 D4），配置存
// settings KV（key 统一 overlay. 前缀）
// ----------------------------------------------------------------------

/// 悬浮窗自定义位置（逻辑坐标）。
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverlayPos {
    pub x: f64,
    pub y: f64,
}

/// 悬浮窗自定义尺寸（逻辑坐标，OVERLAY-SPEC v1.1 §3）。
#[cfg(desktop)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OverlaySize {
    pub w: f64,
    pub h: f64,
}

/// 尺寸下限（内容可读的最低保障）与上限（防呆）。
#[cfg(desktop)]
const OVERLAY_MIN_W: f64 = 220.0;
#[cfg(desktop)]
const OVERLAY_MIN_H: f64 = 280.0;
#[cfg(desktop)]
const OVERLAY_MAX_W: f64 = 1200.0;
#[cfg(desktop)]
const OVERLAY_MAX_H: f64 = 1600.0;

#[cfg(desktop)]
fn clamp_overlay_size(w: f64, h: f64) -> OverlaySize {
    OverlaySize {
        w: w.clamp(OVERLAY_MIN_W, OVERLAY_MAX_W),
        h: h.clamp(OVERLAY_MIN_H, OVERLAY_MAX_H),
    }
}

/// 悬浮窗配置（整体读写 JSON，§6；缺省值见 [`OverlayConfig::default`]）。
#[cfg(desktop)]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub enabled: bool,
    /// 初始吸附角落："tr" / "tl"
    pub corner: String,
    /// 拖动后的自定义位置（原生 Wayland 降级时不写入）
    pub custom_pos: Option<OverlayPos>,
    /// 拖边调整后的自定义尺寸；None = 声明默认 264×380（v1.1）
    pub size: Option<OverlaySize>,
    /// 0.30–1.00，CSS 透明度
    pub opacity: f64,
    /// 锁定（点击穿透）态
    pub locked: bool,
    /// 显示内容分档："all" / "events" / "tasks"（v1.2；未知值按 all）
    pub show: String,
    /// 展开逾期与未安排成列表（false = 底部摘要只计数，v1.2）
    pub expand_summary: bool,
}

#[cfg(desktop)]
impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            corner: "tr".into(),
            custom_pos: None,
            size: None,
            opacity: 0.90,
            locked: false,
            show: "all".into(),
            expand_summary: false,
        }
    }
}

/// 合法 show 分档白名单。
#[cfg(desktop)]
const OVERLAY_SHOW: [&str; 3] = ["all", "events", "tasks"];

#[cfg(desktop)]
fn normalize_overlay_show(v: &str) -> String {
    if OVERLAY_SHOW.contains(&v) {
        v.to_string()
    } else {
        "all".to_string()
    }
}

#[cfg(desktop)]
pub(crate) fn overlay_config(store: &Store) -> std::result::Result<OverlayConfig, String> {
    let get = |k: &str| store.get_setting(k).map_err(err_string).unwrap_or(None);
    Ok(OverlayConfig {
        enabled: get("overlay.enabled")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        corner: if get("overlay.corner").as_deref() == Some("tl") {
            "tl"
        } else {
            "tr"
        }
        .into(),
        custom_pos: get("overlay.custom_pos").and_then(|v| serde_json::from_str(&v).ok()),
        size: get("overlay.size").and_then(|v| serde_json::from_str(&v).ok()),
        opacity: get("overlay.opacity")
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v.clamp(0.30, 1.00))
            .unwrap_or(0.90),
        locked: get("overlay.locked")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
        show: normalize_overlay_show(get("overlay.show").as_deref().unwrap_or("all")),
        expand_summary: get("overlay.expand_summary")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false),
    })
}

#[cfg(desktop)]
fn save_overlay_config(store: &Store, cfg: &OverlayConfig) -> std::result::Result<(), String> {
    store
        .set_setting("overlay.enabled", &cfg.enabled.to_string())
        .map_err(err_string)?;
    store
        .set_setting(
            "overlay.corner",
            if cfg.corner == "tl" { "tl" } else { "tr" },
        )
        .map_err(err_string)?;
    store
        .set_setting(
            "overlay.opacity",
            &cfg.opacity.clamp(0.30, 1.00).to_string(),
        )
        .map_err(err_string)?;
    store
        .set_setting("overlay.locked", &cfg.locked.to_string())
        .map_err(err_string)?;
    store
        .set_setting("overlay.show", &normalize_overlay_show(&cfg.show))
        .map_err(err_string)?;
    store
        .set_setting("overlay.expand_summary", &cfg.expand_summary.to_string())
        .map_err(err_string)?;
    match cfg.custom_pos {
        Some(p) => {
            let json = serde_json::to_string(&p).map_err(|e| format!("[INTERNAL] {e}"))?;
            store
                .set_setting("overlay.custom_pos", &json)
                .map_err(err_string)?;
        }
        // 清除自定义位置 = 置空串（读取端解析失败按 None 处理）
        None => {
            store
                .set_setting("overlay.custom_pos", "")
                .map_err(err_string)?;
        }
    }
    match cfg.size {
        Some(s) => {
            let json = serde_json::to_string(&clamp_overlay_size(s.w, s.h))
                .map_err(|e| format!("[INTERNAL] {e}"))?;
            store
                .set_setting("overlay.size", &json)
                .map_err(err_string)?;
        }
        None => {
            store.set_setting("overlay.size", "").map_err(err_string)?;
        }
    }
    Ok(())
}

/// 托盘复选项与配置保持同步（设置页改动后托盘勾选状态跟随）。
#[cfg(desktop)]
pub(crate) fn sync_tray(app: &tauri::AppHandle, cfg: &OverlayConfig) {
    if let Some(menu) = app.try_state::<crate::OverlayTrayMenu>() {
        let _ = menu.show.set_checked(cfg.enabled);
        let _ = menu.lock.set_checked(cfg.locked);
    }
}

#[cfg(desktop)]
fn overlay_window(app: &tauri::AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 计算悬浮窗应落的位置（逻辑坐标）：custom_pos 优先，否则按 corner 吸附
/// 工作区（扣任务栏）边缘，边距 12 逻辑像素；任何来源都做边界收敛（§3）。
#[cfg(desktop)]
fn overlay_target_position(
    win: &tauri::WebviewWindow,
    cfg: &OverlayConfig,
) -> std::result::Result<(f64, f64), String> {
    const MARGIN: f64 = 12.0;
    let scale = win.scale_factor().map_err(|e| format!("[INTERNAL] {e}"))?;
    let outer = win.outer_size().map_err(|e| format!("[INTERNAL] {e}"))?;
    // 尺寸以配置为准（刚 set_size 后 GTK 未必立即反映到 outer_size）
    let (w, h) = match cfg.size {
        Some(s) => (s.w, s.h),
        None => (outer.width as f64 / scale, outer.height as f64 / scale),
    };
    let monitor = win
        .current_monitor()
        .map_err(|e| format!("[INTERNAL] {e}"))?
        .or_else(|| {
            win.primary_monitor()
                .map_err(|e| format!("[INTERNAL] {e}"))
                .ok()
                .flatten()
        })
        .ok_or("[NOT_FOUND] 无可用显示器")?;
    let wa = monitor.work_area();
    let to_l = |px: i32| px as f64 / scale;
    let (x0, y0) = (to_l(wa.position.x), to_l(wa.position.y));
    let (x1, y1) = (
        x0 + to_l(wa.size.width as i32),
        y0 + to_l(wa.size.height as i32),
    );

    let (mut x, mut y) = match cfg.custom_pos {
        Some(p) => (p.x, p.y),
        None => match cfg.corner.as_str() {
            "tl" => (x0 + MARGIN, y0 + MARGIN),
            _ => (x1 - w - MARGIN, y0 + MARGIN),
        },
    };
    // 整窗收回当前工作区：分辨率 / 缩放变化后不得留在屏外
    x = x.clamp(x0, (x1 - w).max(x0));
    y = y.clamp(y0, (y1 - h).max(y0));
    Ok((x, y))
}

/// 把 locked / size / corner / custom_pos 落到真实窗口（§7）。原生 Wayland
/// 降级时跳过定位（合成器接管移动，坐标语义不稳），尺寸仍尝试设置。
#[cfg(desktop)]
fn apply_overlay_window_state(
    app: &tauri::AppHandle,
    cfg: &OverlayConfig,
) -> std::result::Result<(), String> {
    let Some(win) = overlay_window(app) else {
        return Err("[NOT_FOUND] 无 overlay 窗口".into());
    };
    win.set_ignore_cursor_events(cfg.locked)
        .map_err(|e| format!("[INTERNAL] {e}"))?;
    if let Some(s) = cfg.size {
        let c = clamp_overlay_size(s.w, s.h);
        win.set_size(tauri::LogicalSize::new(c.w, c.h))
            .map_err(|e| format!("[INTERNAL] {e}"))?;
    }
    if crate::platform::native_wayland() {
        return Ok(());
    }
    let (x, y) = overlay_target_position(&win, cfg)?;
    win.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|e| format!("[INTERNAL] {e}"))
}

/// 配置变更后的共同收尾：托盘同步 + 窗口状态落地 + 前端广播。
#[cfg(desktop)]
fn after_overlay_config_change(
    app: &tauri::AppHandle,
    cfg: &OverlayConfig,
) -> std::result::Result<(), String> {
    use tauri::Emitter;
    sync_tray(app, cfg);
    apply_overlay_window_state(app, cfg)?;
    let _ = app.emit("overlay-config", cfg);
    Ok(())
}

/// 今日悬浮窗数据（OVERLAY-SPEC §4 求值，语义唯一实现在 myday-core）。
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_today(
    state: State<'_, AppState>,
) -> std::result::Result<myday_core::overlay::TodayOverlay, String> {
    let today = chrono::Local::now().date_naive();
    myday_core::overlay::today_overlay(&state.store, today).map_err(err_string)
}

#[cfg(desktop)]
#[tauri::command]
pub fn get_overlay_config(
    state: State<'_, AppState>,
) -> std::result::Result<OverlayConfig, String> {
    overlay_config(&state.store)
}

#[cfg(desktop)]
#[tauri::command]
pub fn set_overlay_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: OverlayConfig,
) -> std::result::Result<(), String> {
    save_overlay_config(&state.store, &config)?;
    after_overlay_config_change(&app, &config)
}

/// 锁定（穿透）开关：托盘「悬浮窗锁定」与设置页共用。
#[cfg(desktop)]
pub(crate) fn set_overlay_locked(
    app: &tauri::AppHandle,
    store: &Store,
    locked: bool,
) -> std::result::Result<(), String> {
    let mut cfg = overlay_config(store)?;
    cfg.locked = locked;
    save_overlay_config(store, &cfg)?;
    after_overlay_config_change(app, &cfg)
}

/// 显示 / 隐藏悬浮窗（托盘与主窗口设置共用）。`persist` = 是否写
/// `overlay.enabled`（托盘开关持久化；启动恢复与 --overlay 唤起不写）。
#[cfg(desktop)]
pub(crate) fn set_overlay_visible(
    app: &tauri::AppHandle,
    store: &Store,
    visible: bool,
    persist: bool,
) -> std::result::Result<(), String> {
    if visible {
        let cfg = overlay_config(store)?;
        // 只 show 不 set_focus：出现 / 刷新永不抢焦点（§3）
        if let Some(win) = overlay_window(app) {
            win.set_ignore_cursor_events(cfg.locked)
                .map_err(|e| format!("[INTERNAL] {e}"))?;
            win.show().map_err(|e| format!("[INTERNAL] {e}"))?;
        }
        // show 之后再定位：窗口 realize 后 outer_size / 工作区才可靠
        apply_overlay_window_state(app, &cfg)?;
    } else if let Some(win) = overlay_window(app) {
        let _ = win.hide();
    }
    if persist {
        let mut cfg = overlay_config(store)?;
        cfg.enabled = visible;
        save_overlay_config(store, &cfg)?;
        after_overlay_config_change(app, &cfg)?;
    }
    Ok(())
}

#[cfg(desktop)]
#[tauri::command]
pub fn overlay_set_visible(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    visible: bool,
) -> std::result::Result<(), String> {
    set_overlay_visible(&app, &state.store, visible, true)
}

/// 拖动结束后持久化当前位置（前端 onMoved 防抖后调用；§3 位置持久化）。
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_save_drag_pos(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<(), String> {
    if crate::platform::native_wayland() {
        return Ok(()); // 原生 Wayland：客户端无法读写坐标，位置不记忆
    }
    let Some(win) = overlay_window(&app) else {
        return Ok(());
    };
    let scale = win.scale_factor().map_err(|e| format!("[INTERNAL] {e}"))?;
    let pos = win
        .outer_position()
        .map_err(|e| format!("[INTERNAL] {e}"))?;
    let mut cfg = overlay_config(&state.store)?;
    cfg.custom_pos = Some(OverlayPos {
        x: pos.x as f64 / scale,
        y: pos.y as f64 / scale,
    });
    save_overlay_config(&state.store, &cfg)?;
    use tauri::Emitter;
    let _ = app.emit("overlay-config", &cfg);
    Ok(())
}

/// 拖边调整尺寸后持久化当前大小（前端 onResized 防抖后调用；v1.1 §3/§6）。
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_save_resize_size(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<(), String> {
    if crate::platform::native_wayland() {
        return Ok(()); // 原生 Wayland：尺寸读写同样不稳，不记忆
    }
    let Some(win) = overlay_window(&app) else {
        return Ok(());
    };
    let scale = win.scale_factor().map_err(|e| format!("[INTERNAL] {e}"))?;
    let size = win.outer_size().map_err(|e| format!("[INTERNAL] {e}"))?;
    let mut cfg = overlay_config(&state.store)?;
    cfg.size = Some(clamp_overlay_size(
        size.width as f64 / scale,
        size.height as f64 / scale,
    ));
    save_overlay_config(&state.store, &cfg)?;
    use tauri::Emitter;
    let _ = app.emit("overlay-config", &cfg);
    Ok(())
}

/// 悬浮窗点击条目 / 摘要行：前置主窗口（P1 不做定位联动，§5.4）。
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_show_main(app: tauri::AppHandle) -> std::result::Result<(), String> {
    crate::show_main(&app);
    Ok(())
}

/// 窗口状态重放（前端配置面板 / 独立调用）：重读配置落到真实窗口。
#[cfg(desktop)]
#[tauri::command]
pub fn overlay_apply_window_state(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> std::result::Result<(), String> {
    let cfg = overlay_config(&state.store)?;
    apply_overlay_window_state(&app, &cfg)
}

/// invoke 命令注册表：桌面 / 移动共用同一张表。
/// 数据面命令两端可用；窗口 / 托盘 / IPC / 文件管理器相关命令在条目上标注
/// `#[cfg(desktop)]`（generate_handler 会把属性转写到 match 分支，移动构建时
/// 整条剔除，不需要维护第二张表）。
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // ---- 条目 CRUD 与视图查询（两端共用）----
        list_items,
        list_items_window,
        tasks_view,
        get_item,
        add_item,
        update_item,
        delete_item,
        list_trash,
        restore_item,
        purge_item,
        empty_trash,
        import_ics,
        detach_occurrence,
        skip_occurrence,
        remove_occurrence_exdate,
        complete_task,
        snooze,
        search_items,
        list_templates,
        add_template,
        update_template,
        delete_template,
        move_template,
        set_template_pinned,
        list_field_defs,
        add_field_def,
        update_field_def,
        delete_field_def,
        count_items_with_field,
        list_deleted_field_defs,
        purge_deleted_field_defs,
        add_attachment_b64,
        attachment_abs_path,
        get_setting,
        set_setting,
        set_window_bg,
        check_conflict,
        stats_summary,
        view_list,
        view_get,
        view_create,
        view_save,
        view_delete,
        view_reset,
        view_duplicate,
        query_view,
        query_stats_page,
        stats_restore_defaults,
        reminder_history,
        reminder_unread,
        mark_reminders_seen,
        #[cfg(desktop)]
        alarm_pending,
        #[cfg(desktop)]
        alarm_dismiss,
        #[cfg(mobile)]
        alarms_sync_payload,
        #[cfg(mobile)]
        alarms_synced,
        convert_task_to_event,
        event_to_log,
        export_ics,
        backup_zip,
        load_holidays_json,
        save_holidays_json,
        reset_holidays_json,
        app_info,
        path_is_dir,
        set_ui_lang,
        // ---- 桌面专属：文件管理器 / 自启 / 快速窗口 / 托盘悬浮窗 ----
        #[cfg(desktop)]
        sync_server_info,
        #[cfg(desktop)]
        sync_server_toggle,
        #[cfg(desktop)]
        sync_server_token_regen,
        #[cfg(mobile)]
        crate::sync_client::sync_run,
        #[cfg(desktop)]
        open_file_path,
        #[cfg(desktop)]
        reveal_file_path,
        #[cfg(desktop)]
        open_log_dir,
        #[cfg(desktop)]
        get_autostart,
        #[cfg(desktop)]
        set_autostart,
        #[cfg(desktop)]
        open_quick_add,
        #[cfg(desktop)]
        overlay_today,
        #[cfg(desktop)]
        get_overlay_config,
        #[cfg(desktop)]
        set_overlay_config,
        #[cfg(desktop)]
        overlay_apply_window_state,
        #[cfg(desktop)]
        overlay_set_visible,
        #[cfg(desktop)]
        overlay_save_drag_pos,
        #[cfg(desktop)]
        overlay_save_resize_size,
        #[cfg(desktop)]
        overlay_show_main,
    ]
}

// ----------------------------------------------------------------------
// 桌面同步服务管理（设置 → 移动端同步；移动端同款合并逻辑见 core::sync）
// ----------------------------------------------------------------------

/// 桌面设置页的同步状态面板（enabled=意图,running=实际监听中）。
#[cfg(desktop)]
#[tauri::command]
pub fn sync_server_info(state: State<'_, AppState>, app: tauri::AppHandle) -> serde_json::Value {
    let get = |k: &str| {
        state
            .store
            .get_setting(k)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    serde_json::json!({
        "enabled": get("sync.enabled") == "true",
        "running": crate::sync_server::server_running(&app),
        "ip": crate::sync_server::primary_lan_ip(),
        "port": myday_core::sync::SYNC_PORT,
        "token": get("sync.token"),
        // 注意:与服务端会话写入的 settings 键一致(core sync LAST_SYNC_KEY)
        "last_sync": get("sync.last_sync_at"),
    })
}

#[cfg(desktop)]
#[tauri::command]
pub async fn sync_server_toggle(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enable: bool,
) -> Result<serde_json::Value, String> {
    let token = {
        let existing = state
            .store
            .get_setting("sync.token")
            .ok()
            .flatten()
            .unwrap_or_default();
        if existing.is_empty() {
            let nt = crate::sync_server::new_token();
            state
                .store
                .set_setting("sync.token", &nt)
                .map_err(err_string)?;
            nt
        } else {
            existing
        }
    };
    if enable {
        crate::sync_server::server_start(&app, state.store.clone(), token).await?;
    } else {
        crate::sync_server::server_stop(&app);
    }
    state
        .store
        .set_setting("sync.enabled", &enable.to_string())
        .map_err(err_string)?;
    Ok(sync_server_info(state, app))
}

/// 重新生成配对码(旧码立即失效;客户端需重填)。
#[cfg(desktop)]
#[tauri::command]
pub async fn sync_server_token_regen(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let nt = crate::sync_server::new_token();
    state
        .store
        .set_setting("sync.token", &nt)
        .map_err(err_string)?;
    // 运行中则热重启,立刻用上新码
    if crate::sync_server::server_running(&app) {
        crate::sync_server::server_start(&app, state.store.clone(), nt.clone()).await?;
    }
    Ok(sync_server_info(state, app))
}

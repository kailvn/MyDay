//! 核心库集成测试（SCHEMA-REDESIGN 定稿验收清单 §9）。
//! 每个用例使用独立临时数据目录（`MYDAY_DATA_DIR`）。

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Duration, SubsecRound, TimeZone, Utc};

use myday_core::ipc::{self, IpcHandler, IpcRequest, IpcResponse};
use myday_core::model::*;
use myday_core::reminder::{tick_once, Notifier};
use myday_core::store::{ListFilter, ListOrder, Store, TaskView};
use rusqlite::params;

struct TempDir(tempfile::TempDir);

// tempfile 仅测试使用
impl TempDir {
    fn new() -> Self {
        Self(tempfile::tempdir().expect("tempdir"))
    }
    fn store(&self) -> Store {
        Store::open(&self.0.path().join("myday.db"), self.0.path()).expect("open store")
    }
    fn db_path(&self) -> std::path::PathBuf {
        self.0.path().join("myday.db")
    }
}

// ----------------------------------------------------------------------
// 种子 / 首次启动
// ----------------------------------------------------------------------

#[test]
fn seeds_priority_field_and_generic_pinned_templates() {
    let t = TempDir::new();
    let store = t.store();
    let defs = store.list_field_defs(Some(ItemType::Task)).unwrap();
    assert!(defs.iter().any(|d| d.id == "fd_priority" && d.builtin), "内置优先级字段");

    let templates = store.list_templates().unwrap();
    for id in ["tpl_water", "tpl_weight"] {
        let tpl = templates.iter().find(|x| x.id == id).expect(id);
        assert!(tpl.pinned, "{id} 种子即钉选");
        assert!(tpl.builtin);
        assert_eq!(tpl.item_type, ItemType::Log);
    }
    // 随模板物化的字段：数字字段带单位
    let log_defs = store.list_field_defs(Some(ItemType::Log)).unwrap();
    for fid in ["fd_water_cup", "fd_weight_kg"] {
        assert!(log_defs.iter().any(|d| d.id == fid), "missing field {fid}");
    }
    let weight = log_defs.iter().find(|d| d.id == "fd_weight_kg").unwrap();
    assert_eq!(weight.options.get("unit"), Some(&serde_json::json!("kg")));

    // 重开不复活（软删内置字段后重开仍在软删态）
    store.delete_field_def(&weight.id).unwrap();
    drop(store);
    let store = Store::open(&t.db_path(), t.0.path()).unwrap();
    assert!(
        !store.list_field_defs(Some(ItemType::Log)).unwrap().iter().any(|d| d.id == "fd_weight_kg"),
        "软删字段不因重开复活"
    );
}

// ----------------------------------------------------------------------
// 字段：删除后同名重建 = 复活原字段（历史值自动重挂）
// ----------------------------------------------------------------------

#[test]
fn recreate_deleted_field_revives_with_history() {
    let t = TempDir::new();
    let store = t.store();
    let def = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","差"]}), None)
        .unwrap();
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("随手记".into()),
            occurred_at: Some(Utc::now().trunc_subsecs(0)),
            extra: [(def.id.clone(), serde_json::json!("好"))].into_iter().collect(),
            ..Default::default()
        })
        .unwrap();

    // 删除 → 历史值仍在 extra 里但定义不可见
    store.delete_field_def(&def.id).unwrap();
    assert!(!store.list_field_defs(None).unwrap().iter().any(|d| d.id == def.id));

    // 同名重建：复活原 id（历史值重挂），kind/options 按新输入更新
    let again = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","一般","差"]}), None)
        .unwrap();
    assert_eq!(again.id, def.id, "重建同名应复活软删字段而不是新建 id");
    assert_eq!(again.options.get("choices").unwrap(), &serde_json::json!(["好", "一般", "差"]));

    let fetched = store.get_item(&item.id).unwrap();
    assert_eq!(fetched.extra.get(&def.id), Some(&serde_json::json!("好")), "历史值自动恢复可见");

    // 活跃同名字段仍拒绝重复创建
    let err = store
        .add_field_def("心情", FieldKind::Text, &serde_json::json!({}), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Conflict);

    // 软删字段不挡新建不同 scope 的同名（scope 不同即不同字段位）
    let scoped = store
        .add_field_def("心情", FieldKind::Text, &serde_json::json!({}), Some(ItemType::Log))
        .unwrap();
    assert_ne!(scoped.id, def.id);
}

#[test]
fn purge_deleted_fields_removes_revive_path() {
    let t = TempDir::new();
    let store = t.store();
    let def = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","差"]}), None)
        .unwrap();
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("随手记".into()),
            occurred_at: Some(Utc::now().trunc_subsecs(0)),
            extra: [(def.id.clone(), serde_json::json!("好"))].into_iter().collect(),
            ..Default::default()
        })
        .unwrap();

    store.delete_field_def(&def.id).unwrap();
    assert_eq!(store.list_deleted_field_defs().unwrap().len(), 1);

    // 清理：彻底删除软删行
    let n = store.purge_deleted_field_defs().unwrap();
    assert_eq!(n, 1);
    assert!(store.list_deleted_field_defs().unwrap().is_empty());

    // 同名重建 = 全新字段（不复活）：旧值仍留在条目上但不再挂到任何活跃字段
    let again = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","差"]}), None)
        .unwrap();
    assert_ne!(again.id, def.id);
    let fetched = store.get_item(&item.id).unwrap();
    assert!(fetched.extra.get(&def.id).is_some(), "清理前的旧值仍在条目 JSON 上");
    assert!(fetched.extra.get(&again.id).is_none(), "但不挂到新字段");

    // 无软删行时清理为空操作
    assert_eq!(store.purge_deleted_field_defs().unwrap(), 0);
}

// ----------------------------------------------------------------------
// 创建：默认值 / 校验 / 自动识别
// ----------------------------------------------------------------------

#[test]
fn event_defaults_and_roundtrip() {
    let t = TempDir::new();
    let store = t.store();
    let start = (Utc::now() + Duration::hours(3)).trunc_subsecs(0);
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("项目评审".into()),
            start_at: Some(start),
            tags: vec!["工作".into(), "会议".into()],
            ..Default::default()
        })
        .unwrap();

    assert!(item.id.starts_with("evt_"), "id = {}", item.id);
    // 默认结束时间 = 开始 + 1h
    assert_eq!(item.end_at, Some(start + Duration::hours(1)));
    // 默认提前 10 分钟提醒：存相对意图 spec，channel = notify
    assert_eq!(item.reminders.len(), 1);
    assert_eq!(item.reminders[0].spec, "@start-10m");
    assert_eq!(item.reminders[0].channel, "notify");
    // event 强制：status / occurred_at / due_at 为 NULL
    assert_eq!(item.status, None);
    assert_eq!(item.occurred_at, None);
    assert_eq!(item.tags, vec!["会议".to_string(), "工作".to_string()]);

    let fetched = store.get_item(&item.id).unwrap();
    assert_eq!(fetched, item);

    // 日程必须带开始时间
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("没有开始".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    // 日程不允许截止时间（同填两者只能是 task）
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("带截止".into()),
            start_at: Some(start),
            due_at: Some(start),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
}

#[test]
fn log_defaults_occurred_at_now_and_rejects_future() {
    let t = TempDir::new();
    let store = t.store();
    let before = Utc::now().trunc_subsecs(0);
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("头痛".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(item.id.starts_with("log_"));
    assert_eq!(item.status, None, "记录没有状态");
    assert!(item.occurred_at.unwrap() >= before, "新建 log 默认带 occurred_at");
    assert!(item.reminders.is_empty(), "记录默认不提醒");
    assert!(
        item.start_at.is_none() && item.due_at.is_none(),
        "记录不再复用 start_at"
    );

    // 未来的"记录"是语义错误
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("穿越记录".into()),
            occurred_at: Some(Utc::now() + Duration::hours(1)),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);

    // log 允许只有 note
    let note_only = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            note: Some("只有备注的记录".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(note_only.title, None);
    assert_eq!(display_title(&note_only), "只有备注的记录");
}

#[test]
fn auto_detect_type_by_content() {
    let t = TempDir::new();
    let store = t.store();

    // 截止 → task（优先于开始）
    let with_both = store
        .add_item(NewItem {
            title: Some("带开始和截止".into()),
            start_at: Some(Utc::now() + Duration::days(1)),
            due_at: Some(Utc::now() + Duration::days(2)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_both.item_type, ItemType::Task);

    // 开始 → event
    let with_start = store
        .add_item(NewItem {
            title: Some("带开始".into()),
            start_at: Some(Utc::now() + Duration::days(1)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_start.item_type, ItemType::Event);

    // log 范围字段值 → log
    let with_log_field = store
        .add_item(NewItem {
            title: Some("称重".into()),
            extra: serde_json::json!({ "fd_weight_kg": 70.5 }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_log_field.item_type, ItemType::Log);

    // 裸文本 → task
    let bare = store
        .add_item(NewItem { title: Some("买牛奶".into()), ..Default::default() })
        .unwrap();
    assert_eq!(bare.item_type, ItemType::Task);
    assert_eq!(bare.status, Some(ItemStatus::Todo), "待办缺省 status = todo");
}

#[test]
fn template_implies_its_item_type() {
    // §5.8 规则 1：模板自带 item_type，未显式指定类型时按模板类型创建
    let t = TempDir::new();
    let store = t.store();
    let item = store
        .add_item(NewItem {
            template_id: Some("tpl_weight".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(item.item_type, ItemType::Log, "记录类模板 → log");
    assert_eq!(item.title.as_deref(), Some("体重"), "列默认 title 生效");
    assert!(item.occurred_at.is_some(), "log 默认 occurred_at");

    // 显式类型仍最优先
    let explicit = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            template_id: Some("tpl_weight".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(explicit.item_type, ItemType::Task);
}

#[test]
fn idempotency_key_returns_existing() {
    let t = TempDir::new();
    let store = t.store();
    let new = NewItem {
        item_type: Some(ItemType::Task),
        title: Some("买牛奶".into()),
        idempotency_key: Some("agent-1".into()),
        ..Default::default()
    };
    let a = store.add_item(new.clone()).unwrap();
    let b = store.add_item(new).unwrap();
    assert_eq!(a.id, b.id, "幂等键命中应返回同一条目");
}

// ----------------------------------------------------------------------
// 类型不可变
// ----------------------------------------------------------------------

#[test]
fn item_type_is_immutable_via_trigger() {
    let t = TempDir::new();
    let store = t.store();
    let task = store
        .add_item(NewItem { title: Some("买牛奶".into()), ..Default::default() })
        .unwrap();

    // 绕过应用层直接 UPDATE 也被 trigger 拒绝
    let conn = store.raw_conn().unwrap();
    let err = conn.execute("UPDATE items SET type = 'log' WHERE id = ?1", params![task.id]);
    assert!(err.is_err(), "type 变更应被 trigger 拒绝");
    drop(conn);
    assert_eq!(store.get_item(&task.id).unwrap().item_type, ItemType::Task);
}

// ----------------------------------------------------------------------
// 待办状态机
// ----------------------------------------------------------------------

#[test]
fn complete_and_reopen_keeps_completed_at_invariant() {
    let t = TempDir::new();
    let store = t.store();
    let task = store
        .add_item(NewItem { title: Some("写周报".into()), ..Default::default() })
        .unwrap();
    let done = store.complete_task(&task.id).unwrap();
    assert_eq!(done.status, Some(ItemStatus::Done));
    assert!(done.completed_at.is_some());

    // 重开：completed_at 置 NULL（双向封闭）
    let reopened = store.uncomplete_task(&task.id).unwrap();
    assert_eq!(reopened.status, Some(ItemStatus::Todo));
    assert!(reopened.completed_at.is_none());

    // 待办与记录分离：完成待办不再生成影子记录
    let logs = store
        .list_items(&ListFilter { item_type: Some(ItemType::Log), ..Default::default() })
        .unwrap();
    assert!(logs.is_empty());

    // 历史遗留的影子记录在打开库时清理
    let conn = store.raw_conn().unwrap();
    conn.execute(
        "INSERT INTO items (id, type, title, note, occurred_at, extra, created_at, updated_at)
         VALUES ('legacy', 'log', '完成：写周报', '来源待办 abc', '2026-01-01T00:00:00Z', '{}',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .unwrap();
    drop(conn);
    let reopened_store = Store::open(&t.db_path(), t.0.path()).unwrap();
    let logs = reopened_store
        .list_items(&ListFilter { item_type: Some(ItemType::Log), ..Default::default() })
        .unwrap();
    assert!(logs.is_empty(), "遗留影子记录应被清理");
}

#[test]
fn task_views_split_by_status_and_date() {
    let t = TempDir::new();
    let store = t.store();
    // Today 视图按本地日求值（tasks_view 以本地今日 23:59:59 为界）：
    // now+1h 在本地 23 点后跑会跨过午夜落进明天，取本地今日正午则任何时刻跑都落在今天
    let due_today = chrono::Local
        .from_local_datetime(
            &chrono::Local::now().date_naive().and_hms_opt(12, 0, 0).unwrap(),
        )
        .single()
        .unwrap()
        .with_timezone(&Utc);
    let today_due = store
        .add_item(NewItem {
            title: Some("今天到期".into()),
            due_at: Some(due_today),
            ..Default::default()
        })
        .unwrap();
    let next_week = store
        .add_item(NewItem {
            title: Some("下周到期".into()),
            due_at: Some(Utc::now() + Duration::days(7)),
            ..Default::default()
        })
        .unwrap();
    let no_due = store
        .add_item(NewItem { title: Some("无截止".into()), ..Default::default() })
        .unwrap();

    let today = store.tasks_view(TaskView::Today, None).unwrap();
    assert!(today.iter().any(|i| i.id == today_due.id));
    assert!(today.iter().any(|i| i.id == no_due.id), "无截止待办进今天视图");

    let upcoming = store.tasks_view(TaskView::Upcoming, None).unwrap();
    assert_eq!(upcoming.len(), 1);
    assert_eq!(upcoming[0].id, next_week.id);

    store.complete_task(&today_due.id).unwrap();

    // Done 视图只收已完成；完成后的待办退出未完成视图
    let done = store.tasks_view(TaskView::Done, None).unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].status, Some(ItemStatus::Done));
    assert!(
        !store.tasks_view(TaskView::All, None).unwrap().iter().any(|i| i.id == today_due.id),
        "已完成不再出现在未完成视图"
    );

    // 重开：回到未完成视图
    store.uncomplete_task(&today_due.id).unwrap();
    assert!(store
        .tasks_view(TaskView::All, None)
        .unwrap()
        .iter()
        .any(|i| i.id == today_due.id));
}

// ----------------------------------------------------------------------
// extra / 字段定义
// ----------------------------------------------------------------------

#[test]
fn extra_strict_validation_by_kind() {
    let t = TempDir::new();
    let store = t.store();

    // select 只收 choices 内的值
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("采购".into()),
            extra: serde_json::json!({ "fd_priority": "紧急" }),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);

    let ok = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("采购".into()),
            extra: serde_json::json!({ "fd_priority": "高" }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(ok.extra["fd_priority"], "高");

    // 未知键拒绝（key 必须是 field_defs.id）
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("无主键值".into()),
            extra: serde_json::json!({ "不存在的字段": 1 }),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);

    // scope 不匹配拒绝（task 字段不能写在 log 上）
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("跨范围".into()),
            extra: serde_json::json!({ "fd_priority": "高" }),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);

    // 数字字段必须可解析为数字
    let pace = store
        .add_field_def("配速", FieldKind::Number, &serde_json::json!({}), Some(ItemType::Log))
        .unwrap();
    let err = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("跑步".into()),
            extra: serde_json::json!({ (pace.id.clone()): "很快" }),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
}

#[test]
fn extra_patch_replaces_whole_object() {
    let t = TempDir::new();
    let store = t.store();
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("采购".into()),
            extra: serde_json::json!({ "fd_priority": "高" }),
            ..Default::default()
        })
        .unwrap();
    let mood2 = store
        .add_field_def("心情", FieldKind::Text, &serde_json::json!({}), Some(ItemType::Task))
        .unwrap();

    let patched = store
        .update_item(
            &item.id,
            ItemPatch {
                extra: Some(serde_json::json!({ (mood2.id.clone()): "平静" })),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(patched.extra.get("fd_priority").is_none(), "extra 为整对象替换而非按键合并");
    assert_eq!(patched.extra[mood2.id.as_str()], "平静");
}

#[test]
fn field_defs_partial_unique_and_soft_delete() {
    let t = TempDir::new();
    let store = t.store();

    let mood = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","差"]}), Some(ItemType::Log))
        .unwrap();
    assert!(mood.id.starts_with("fld_"));
    assert!(!mood.builtin);

    // 活跃同名字段仍被拒（同名全局也不行：同名同 scope 唯一）
    assert_eq!(
        store
            .add_field_def("心情", FieldKind::Text, &serde_json::json!({}), Some(ItemType::Log))
            .unwrap_err()
            .code(),
        myday_core::ErrorCode::Conflict
    );

    // 软删：值保留在 extra（不主动清理），读取忽略
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("今日心情".into()),
            extra: serde_json::json!({ (mood.id.clone()): "好" }),
            ..Default::default()
        })
        .unwrap();
    let deleted = store.delete_field_def(&mood.id).unwrap();
    assert_eq!(deleted.id, mood.id);
    assert!(store.get_field_def(&mood.id).is_err(), "软删后 get 不可见");
    let raw = store.get_item(&item.id).unwrap();
    assert_eq!(raw.extra[mood.id.as_str()], "好", "软删不清理 extra 历史数据");

    // 同名重建 = 复活原字段（同 id，历史值自动重挂；部分唯一索引不覆盖软删行）
    let rebuilt = store
        .add_field_def("心情", FieldKind::Select, &serde_json::json!({"choices":["好","差"]}), Some(ItemType::Log))
        .unwrap();
    assert_eq!(rebuilt.id, mood.id);

    // 重命名零成本：不动条目上的值
    let before = store.get_item(&item.id).unwrap();
    store.update_field_def(&rebuilt.id, Some("情绪"), None, None).unwrap();
    let after = store.get_item(&item.id).unwrap();
    assert_eq!(before.extra, after.extra, "改名不重写条目");
    assert_eq!(store.get_field_def(&rebuilt.id).unwrap().name, "情绪");

    // 计数按 id
    assert_eq!(store.count_items_with_field(&mood.id).unwrap(), 1);
}

// ----------------------------------------------------------------------
// 搜索
// ----------------------------------------------------------------------

#[test]
fn search_hits_title_note_tag_and_field_name_only_on_used_items() {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("客户沟通会".into()),
            note: Some("确认报价".into()),
            start_at: Some(Utc::now() + Duration::days(1)),
            tags: vec!["客户".into()],
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem { title: Some("买牛奶".into()), ..Default::default() })
        .unwrap();
    // 用了优先级字段的条目 + 定义了字段但没用它的条目
    store
        .add_item(NewItem {
            title: Some("交房租".into()),
            extra: serde_json::json!({ "fd_priority": "高" }),
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem { title: Some("无关条目".into()), ..Default::default() })
        .unwrap();

    let hits = store.search("客户", None).unwrap();
    assert_eq!(hits.len(), 1, "只命中相关条目");
    assert!(hits[0].matched_in.contains(&"title".to_string()));

    // 搜字段名「优先级」只命中真的用了该字段的条目（不全表命中）
    let hits = store.search("优先级", None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].item.title.as_deref(), Some("交房租"));
    assert!(hits[0].matched_in.contains(&"field".to_string()));

    // 搜字段值
    let hits = store.search("高", Some(ItemType::Task)).unwrap();
    assert!(hits.iter().all(|h| h.item.title.as_deref() == Some("交房租")));

    let hits = store.search("牛奶", Some(ItemType::Task)).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(store.search("不存在的内容", None).unwrap().is_empty());
}

// ----------------------------------------------------------------------
// 提醒
// ----------------------------------------------------------------------

#[test]
fn reminder_due_once_then_snooze_keeps_old_log() {
    let t = TempDir::new();
    let store = t.store();
    // 绝对 spec = 一次性事实；存过去时刻，创建后立即到期
    let past = (Utc::now() - Duration::minutes(5))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let task = store
        .add_item(NewItem {
            title: Some("吃药".into()),
            due_at: Some(Utc::now() - Duration::minutes(5)),
            reminders: vec![NewReminder { spec: past.clone(), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();
    let reminder_id = task.reminders[0].id;
    let first_remind_at = past;

    struct Counter(AtomicUsize);
    impl Notifier for Counter {
        fn notify(
            &self,
            _r: &Reminder,
            _i: &Item,
            _at: chrono::DateTime<chrono::Utc>,
            _a: &[&'static str],
        ) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let counter = Counter(AtomicUsize::new(0));

    assert_eq!(tick_once(&store, &counter).unwrap(), 1);
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    // 去重：同轮再跑不再发送
    assert_eq!(tick_once(&store, &counter).unwrap(), 0);

    // snooze = 绝对覆盖行：旧日志保留，覆盖行到点重新触发
    store.snooze(&task.id, Utc::now() - Duration::minutes(1)).unwrap();
    {
        let conn = store.raw_conn().unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM reminder_log WHERE reminder_id = ?1 AND remind_at = ?2",
                params![reminder_id, first_remind_at],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "旧日志保留");
    }
    assert_eq!(tick_once(&store, &counter).unwrap(), 1);

    // 1:N：同一待办可再挂提醒（整体替换）
    let r2 = (Utc::now() - Duration::minutes(2)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let r3 = (Utc::now() - Duration::minutes(3)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    store
        .update_item(
            &task.id,
            ItemPatch {
                reminders: Some(vec![
                    NewReminder { spec: r2, channel: "sound".into() },
                    NewReminder { spec: r3, channel: "popup".into() },
                ]),
                ..Default::default()
            },
        )
        .unwrap();
    let item = store.get_item(&task.id).unwrap();
    assert_eq!(item.reminders.len(), 2);
    assert_eq!(tick_once(&store, &counter).unwrap(), 2);
}

#[test]
fn recurrence_complete_advances_and_uncomplete_rewinds() {
    let t = TempDir::new();
    let store = t.store();

    // 重复待办：每周三交周报（add 会截断到秒，以入库返回值为基准）
    let task = store
        .add_item(NewItem {
            title: Some("交周报".into()),
            due_at: Some(Utc::now() + Duration::days(2)),
            recurrence: Some("@weekly:3".into()),
            ..Default::default()
        })
        .unwrap();
    let due_before = task.due_at.unwrap();
    assert_eq!(task.recurrence.as_deref(), Some("@weekly:3"));

    // 完成 = 推进到下一期：状态保持 todo、截止后移、留完成记账
    let done = store.complete_task(&task.id).unwrap();
    assert_eq!(done.status, Some(ItemStatus::Todo), "重复待办完成不置 done");
    assert!(
        done.due_at.unwrap() > due_before,
        "截止推进到下一期：{:?} -> {:?}",
        due_before,
        done.due_at
    );
    assert_eq!(
        done.completed_at, None,
        "推进不写 completed_at（CHECK 封闭：todo 无完成时间）"
    );
    assert_eq!(
        done.extra.get(myday_core::store::RECURRED_DONE_KEY).and_then(|v| v.as_str()),
        Some(due_before.to_rfc3339_opts(chrono::SecondsFormat::Secs, true).as_str()),
        "记账 = 完成前的截止"
    );

    // 取消完成 = 回拨到完成前的那一期，记账清掉
    let back = store.uncomplete_task(&task.id).unwrap();
    assert_eq!(back.due_at, Some(due_before));
    assert!(back.extra.get(myday_core::store::RECURRED_DONE_KEY).is_none());

    // 记录不支持重复；非法 spec 拒绝
    assert!(store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("称重".into()),
            occurred_at: Some(Utc::now()),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .is_err());
    assert!(store
        .add_item(NewItem {
            title: Some("坏规则".into()),
            due_at: Some(Utc::now() + Duration::days(1)),
            recurrence: Some("@weekly:9".into()),
            ..Default::default()
        })
        .is_err());
    assert!(store
        .add_item(NewItem {
            title: Some("缺截止".into()),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .is_err());

    // CHECK 兜底：绕过应用层直接 SQL 写 log+recurrence 也被拒
    {
        let conn = store.raw_conn().unwrap();
        let r = conn.execute(
            "INSERT INTO items (id, type, title, occurred_at, recurrence, created_at, updated_at)
             VALUES ('log_x', 'log', '绕过', '2026-01-01T00:00:00+00:00', '@daily', '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')",
            [],
        );
        assert!(r.is_err(), "DB CHECK 应拒绝 log 挂重复规则");
    }

    // 编辑：整体替换规则 / 清除（修改全部语义）
    let ev = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("站会".into()),
            start_at: Some(Utc::now() + Duration::days(1)),
            end_at: Some(Utc::now() + Duration::days(1) + Duration::minutes(15)),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .unwrap();
    let ev = store
        .update_item(
            &ev.id,
            ItemPatch { recurrence: Some("@monthly:15".into()), ..Default::default() },
        )
        .unwrap();
    assert_eq!(ev.recurrence.as_deref(), Some("@monthly:15"));
    let ev = store
        .update_item(&ev.id, ItemPatch { clear_recurrence: true, ..Default::default() })
        .unwrap();
    assert_eq!(ev.recurrence, None);
}

#[test]
fn stats_summary_counts_heatmap_streaks_and_series() {
    let t = TempDir::new();
    let store = t.store();

    // log 模板（pinned 默认 true；名字避开内置的「喝水」等种子）
    let tpl = store
        .add_template("冥想", Some("健康"), None, ItemType::Log, &serde_json::json!({"title":"冥想"}), &serde_json::json!([]), None)
        .unwrap();
    // number 字段（单位 kg，scope log；名字避开内置的「体重」）
    let fid = store
        .add_field_def("体脂率", FieldKind::Number, &serde_json::json!({"unit":"%"}), Some(ItemType::Log))
        .unwrap()
        .id;

    let now = Utc::now().trunc_subsecs(0);
    for off in [0, 1] {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some("冥想".into()),
                template_id: Some(tpl.id.clone()),
                occurred_at: Some(now - Duration::days(off)),
                ..Default::default()
            })
            .unwrap();
    }
    for (off, v) in [(1, 70.5), (2, 70.2), (4, 69.8)] {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some("称重".into()),
                occurred_at: Some(now - Duration::days(off)),
                extra: serde_json::json!({ fid.clone(): v }),
                ..Default::default()
            })
            .unwrap();
    }

    let s = store.stats_summary(365).unwrap();
    // 热力图 365 天、今天有记录
    assert_eq!(s.heatmap.len(), 365);
    assert_eq!(s.heatmap.last().unwrap().count, 1, "今天 1 条冥想");
    // 冥想连续：今天 + 昨天 → current ≥ 2
    let med = s.streaks.iter().find(|x| x.name == "冥想").unwrap();
    assert!(med.current >= 2, "连续应 ≥ 2：{:?}", med);
    assert!(med.longest >= 2);
    assert!(med.recent >= 2);
    // 体重序列 3 点
    let series = s.series.iter().find(|x| x.field_id == fid).unwrap();
    assert_eq!(series.points.len(), 3);
    assert_eq!(series.unit.as_deref(), Some("%"));
    assert!(series.points.windows(2).all(|w| w[0].0 <= w[1].0), "升序");
}

#[test]
fn conflicting_events_finds_overlaps_and_expands_recurrence() {
    use chrono::TimeZone;
    let t = TempDir::new();
    let store = t.store();
    let day = chrono::Local::now().date_naive();
    let at = |h: u32, m: u32| {
        chrono::Local
            .from_local_datetime(&day.and_hms_opt(h, m, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc)
    };

    // 普通日程 10:00-11:00
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("评审".into()),
            start_at: Some(at(10, 0)),
            end_at: Some(at(11, 0)),
            ..Default::default()
        })
        .unwrap();
    // 重复日程每天 22:00-23:00
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("值班".into()),
            start_at: Some(at(22, 0)),
            end_at: Some(at(23, 0)),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .unwrap();

    // 相交命中；紧邻不命中；排除自身不命中
    let hits = store.conflicting_events(at(10, 30), at(10, 45), None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title.as_deref(), Some("评审"));
    assert!(store
        .conflicting_events(at(11, 0), at(11, 30), None)
        .unwrap()
        .is_empty());
    let hits = store.conflicting_events(at(10, 15), at(10, 40), Some(&hits[0].id)).unwrap();
    assert!(hits.is_empty(), "排除自身后无冲突");

    // 重复日程按展开判定
    let hits = store.conflicting_events(at(22, 15), at(22, 45), None).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].title.as_deref(), Some("值班"));
}

#[test]
fn reminder_catchup_window_splits_fire_and_missed() {
    use std::sync::Mutex;

    let t = TempDir::new();
    let store = t.store();
    store.set_setting("reminder_catchup_minutes", "60").unwrap();

    struct Rec {
        fired: AtomicUsize,
        missed: AtomicUsize,
        missed_titles: Mutex<Vec<String>>,
    }
    impl Notifier for Rec {
        fn notify(&self, _: &Reminder, _: &Item, _: chrono::DateTime<chrono::Utc>, _: &[&'static str]) {
            self.fired.fetch_add(1, Ordering::SeqCst);
        }
        fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
            self.missed.fetch_add(count, Ordering::SeqCst);
            let mut titles = self.missed_titles.lock().unwrap();
            for (title, _) in lines {
                titles.push(title.clone());
            }
        }
    }
    let rec = Rec {
        fired: AtomicUsize::new(0),
        missed: AtomicUsize::new(0),
        missed_titles: Mutex::new(Vec::new()),
    };

    let abs = |mins: i64| {
        (Utc::now() - Duration::minutes(mins)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };
    // 窗口内（5 分钟前）→ 正常补发
    store
        .add_item(NewItem {
            title: Some("新鲜的".into()),
            reminders: vec![NewReminder { spec: abs(5), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();
    // 窗口外（3 小时前 > 60 分钟）→ 静默 + 聚合摘要
    store
        .add_item(NewItem {
            title: Some("很久以前的".into()),
            reminders: vec![NewReminder { spec: abs(180), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();

    assert_eq!(tick_once(&store, &rec).unwrap(), 1);
    assert_eq!(rec.fired.load(Ordering::SeqCst), 1);
    assert_eq!(rec.missed.load(Ordering::SeqCst), 1);
    assert_eq!(rec.missed_titles.lock().unwrap().as_slice(), ["很久以前的".to_string()]);

    // 两桶都已入 reminder_log：第二轮零处理
    assert_eq!(tick_once(&store, &rec).unwrap(), 0);
    assert_eq!(rec.missed.load(Ordering::SeqCst), 1);

    // 窗口 = 0：一切错过只进摘要
    store.set_setting("reminder_catchup_minutes", "0").unwrap();
    store
        .add_item(NewItem {
            title: Some("刚建的".into()),
            reminders: vec![NewReminder { spec: abs(1), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(tick_once(&store, &rec).unwrap(), 0);
    assert_eq!(rec.missed.load(Ordering::SeqCst), 2);
    assert_eq!(rec.fired.load(Ordering::SeqCst), 1);
}

#[test]
fn reminder_minutes_zero_disables_auto_reminder() {
    let t = TempDir::new();
    let store = t.store();

    // 语义更新（INTERACTION §6）：自动默认提醒只锚「开始」，不按截止算。
    // 带开始的待办有默认提醒；仅有截止的待办不再自动补。
    let with_r = store
        .add_item(NewItem {
            title: Some("默认提醒".into()),
            start_at: Some(Utc::now().trunc_subsecs(0) + Duration::hours(5)),
            due_at: Some(Utc::now().trunc_subsecs(0) + Duration::hours(6)),
            ..Default::default()
        })
        .unwrap();
    assert!(!with_r.reminders.is_empty(), "带开始的待办应有默认提醒");

    store.set_setting("default_reminder_minutes", "0").unwrap();
    let no_r = store
        .add_item(NewItem {
            title: Some("静默待办".into()),
            start_at: Some(Utc::now().trunc_subsecs(0) + Duration::hours(5)),
            ..Default::default()
        })
        .unwrap();
    assert!(no_r.reminders.is_empty(), "0 应关闭自动提醒");
}

// ----------------------------------------------------------------------
// 模板
// ----------------------------------------------------------------------

#[test]
fn template_defaults_dual_namespace() {
    let t = TempDir::new();
    let store = t.store();

    // 列白名单（title）+ 字段 id（extra）双命名空间
    let tpl = store
        .add_template(
            "体重打卡",
            Some("健康"),
            Some("⚖"),
            ItemType::Log,
            &serde_json::json!({ "title": "体重", "fd_weight_kg": 70.5 }),
            &serde_json::json!([]),
            Some("早晨空腹"),
        )
        .unwrap();

    // 应用模板：字段值预填成功，列默认写入 title
    let item = store
        .add_item(NewItem {
            template_id: Some(tpl.id.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(item.item_type, ItemType::Log);
    assert_eq!(item.title.as_deref(), Some("体重"));
    assert_eq!(item.extra["fd_weight_kg"], 70.5, "字段默认值走 extra");
    assert!(item.tags.contains(&"健康".to_string()), "模板标签生效");

    // 显式 extra 覆盖模板默认
    let overridden = store
        .add_item(NewItem {
            template_id: Some(tpl.name.clone()),
            extra: serde_json::json!({ "fd_weight_kg": 72 }),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(overridden.extra["fd_weight_kg"], 72);

    // 显式标题 + 模板 title 默认并存：列键必须被吞掉而不是落入 extra
    // （回归：match 守卫不满足时 "title" 曾被塞进 extra，报 [INVALID] 不是字段 id）
    let with_title = store
        .add_item(NewItem {
            template_id: Some(tpl.id.clone()),
            title: Some("早上称重".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_title.title.as_deref(), Some("早上称重"), "显式标题优先");
    assert!(
        with_title.extra.get("title").is_none(),
        "列键不得混入 extra"
    );
}

#[test]
fn template_invalid_defaults_rejected_at_save() {
    let t = TempDir::new();
    let store = t.store();

    // 未知列
    let err = store
        .add_template("坏模板", None, None, ItemType::Log, &serde_json::json!({ "nonexistent_column": 1 }), &serde_json::json!([]), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    // 列对该类型无意义：status 仅 task 模板可用
    let err = store
        .add_template("坏模板", None, None, ItemType::Log, &serde_json::json!({ "status": "todo" }), &serde_json::json!([]), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    // 非字段 id 的键
    let err = store
        .add_template("坏模板", None, None, ItemType::Log, &serde_json::json!({ "咖啡": 1 }), &serde_json::json!([]), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    // scope 不匹配：task 字段用在 log 模板
    let err = store
        .add_template("坏模板", None, None, ItemType::Log, &serde_json::json!({ "fd_priority": "高" }), &serde_json::json!([]), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    // 字段值违反 kind：select 超出 choices
    let err = store
        .add_template("坏模板", None, None, ItemType::Task, &serde_json::json!({ "fd_priority": "紧急" }), &serde_json::json!([]), None)
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
}

#[test]
fn template_crud_pinned_and_item_apply() {
    let t = TempDir::new();
    let store = t.store();

    let tpl = store
        .add_template(
            "饮茶",
            Some("健康"),
            None,
            ItemType::Log,
            &serde_json::json!({ "title": "饮茶" }),
            &serde_json::json!([]),
            Some("每天喝水"),
        )
        .unwrap();
    assert!(tpl.id.starts_with("tpl_"));
    assert!(!tpl.builtin);
    assert!(tpl.pinned, "新模板默认钉选");

    // 同名冲突
    assert!(matches!(
        store.add_template("饮茶", None, None, ItemType::Log, &serde_json::json!({}), &serde_json::json!([]), None),
        Err(myday_core::MyDayError::Conflict(_))
    ));

    // 更新（整体替换可编辑字段）
    let updated = store
        .update_template(
            &tpl.id,
            "饮茶打卡",
            Some("饮食"),
            None,
            ItemType::Log,
            &serde_json::json!({ "title": "饮茶打卡" }),
            &serde_json::json!([]),
            None,
        )
        .unwrap();
    assert_eq!(updated.name, "饮茶打卡");

    // 按 ID 引用模板
    let item = store
        .add_item(NewItem { template_id: Some(tpl.id.clone()), ..Default::default() })
        .unwrap();
    assert_eq!(item.title.as_deref(), Some("饮茶打卡"));
    assert_eq!(item.template_id.as_deref(), Some(tpl.id.as_str()));

    // 钉选开关
    store.set_template_pinned(&tpl.id, false).unwrap();
    assert!(!store.get_template(&tpl.id).unwrap().pinned, "取消按钮不删除模板");

    // 删除：已创建条目的值不受影响，悬挂引用清除
    store.delete_template(&tpl.id).unwrap();
    let after = store.get_item(&item.id).unwrap();
    assert!(after.template_id.is_none(), "删模板后条目不应留悬挂引用");
    assert_eq!(after.title.as_deref(), Some("饮茶打卡"));

    // 排序交换与端点移动
    let a = store.add_template("aa", None, None, ItemType::Log, &serde_json::json!({}), &serde_json::json!([]), None).unwrap();
    let b = store.add_template("bb", None, None, ItemType::Log, &serde_json::json!({}), &serde_json::json!([]), None).unwrap();
    store.move_template(&b.id, true).unwrap();
    let list = store.list_templates().unwrap();
    let pos = |n: &str| list.iter().position(|x| x.name == n).unwrap();
    assert!(pos("bb") < pos("aa"), "bb 上移后应排在 aa 前");
    store.move_template(&a.id, true).unwrap(); // 端点不报错
}

// ----------------------------------------------------------------------
// 列表过滤 / 删除 / 附件
// ----------------------------------------------------------------------

#[test]
fn list_filter_deserializes_partial_json() {
    // 前端（Tauri invoke）按需传参：非 Option 字段必须有 serde default
    let f: ListFilter =
        serde_json::from_str(r#"{"item_type":"log","order":"asc"}"#).expect("partial filter");
    assert_eq!(f.item_type, Some(ItemType::Log));
    assert_eq!(f.offset, 0);
    let f: ListFilter = serde_json::from_str("{}").expect("empty filter");
    assert_eq!(f.item_type, None);
    // changed_on：日期字符串 → NaiveDate
    let f: ListFilter =
        serde_json::from_str(r#"{"changed_on":"2026-09-16"}"#).expect("changed_on filter");
    assert_eq!(f.changed_on, Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()));
}

#[test]
fn log_timeline_orders_by_occurred_at_and_date_range() {
    let t = TempDir::new();
    let store = t.store();
    let yesterday = Utc::now() - Duration::days(1);
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("昨天的记录".into()),
            occurred_at: Some(yesterday),
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem { item_type: Some(ItemType::Log), title: Some("现在的记录".into()), ..Default::default() })
        .unwrap();

    let items = store
        .list_items(&ListFilter { item_type: Some(ItemType::Log), order: ListOrder::Desc, ..Default::default() })
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title.as_deref(), Some("现在的记录"), "按 occurred_at 倒序");

    // from/to 锚点 = occurred_at
    let items = store
        .list_items(&ListFilter {
            item_type: Some(ItemType::Log),
            from: Some(yesterday - Duration::hours(1)),
            to: Some(yesterday + Duration::hours(1)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title.as_deref(), Some("昨天的记录"));
}

#[test]
fn changed_on_filters_by_creation_or_modification_day() {
    let t = TempDir::new();
    let store = t.store();
    let today = chrono::Local::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);

    let log = store
        .add_item(NewItem { title: Some("今天的记录".into()), ..Default::default() })
        .unwrap();
    let ev = store
        .add_item(NewItem {
            title: Some("明天日程".into()),
            start_at: Some((Utc::now() + Duration::days(1)).trunc_subsecs(0)),
            ..Default::default()
        })
        .unwrap();
    let old = store
        .add_item(NewItem { title: Some("昨天建的待办".into()), ..Default::default() })
        .unwrap();
    store
        .update_item(&old.id, ItemPatch { title: Some("昨天建的待办（今天改）".into()), ..Default::default() })
        .unwrap();
    let _ = log;

    let activity = store
        .list_items(&ListFilter { changed_on: Some(today), ..Default::default() })
        .unwrap();
    let ids: Vec<&str> = activity.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&ev.id.as_str()), "明天日程今天创建，应出现在今日活动");
    assert!(ids.contains(&old.id.as_str()), "昨天待办今天修改，应出现在今日活动");

    let yesterday_activity = store
        .list_items(&ListFilter { changed_on: Some(yesterday), ..Default::default() })
        .unwrap();
    assert!(yesterday_activity.is_empty(), "没有昨天创建/修改的条目");
}

#[test]
fn delete_item_removes_attachments_and_rows() {
    let t = TempDir::new();
    let store = t.store();
    let item = store
        .add_item(NewItem {
            title: Some("客户沟通".into()),
            start_at: Some(Utc::now() + Duration::hours(1)),
            ..Default::default()
        })
        .unwrap();
    let att = store
        .add_attachment_bytes(&item.id, b"fake-png-bytes", "png")
        .unwrap();
    let abs = store.attachment_abs_path(&att);
    assert!(abs.exists());
    assert!(att.id > 0);

    store.delete_item(&item.id).unwrap();
    assert!(!abs.exists(), "附件文件应随条目删除");
    assert!(store.get_item(&item.id).is_err());
}

#[test]
fn update_patch_clear_time_fields() {
    let t = TempDir::new();
    let store = t.store();

    let item = store
        .add_item(NewItem {
            title: Some("交报告".into()),
            start_at: Some(Utc::now().trunc_subsecs(0)),
            due_at: Some(Utc::now().trunc_subsecs(0) + Duration::hours(4)),
            ..Default::default()
        })
        .unwrap();

    let cleared = store
        .update_item(&item.id, ItemPatch { clear_due_at: true, ..Default::default() })
        .unwrap();
    assert!(cleared.due_at.is_none());
    assert!(cleared.start_at.is_some(), "未触碰的字段不动");

    // event 不允许清开始时间（CHECK 兜底 + 友好报错）
    let ev = store
        .add_item(NewItem {
            title: Some("日程".into()),
            start_at: Some(Utc::now().trunc_subsecs(0)),
            ..Default::default()
        })
        .unwrap();
    let err = store
        .update_item(&ev.id, ItemPatch { clear_start_at: true, ..Default::default() })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);

    // log 不可改 start_at（发生时间走 occurred_at）
    let log = store
        .add_item(NewItem { item_type: Some(ItemType::Log), title: Some("记录".into()), ..Default::default() })
        .unwrap();
    let err = store
        .update_item(&log.id, ItemPatch { start_at: Some(Utc::now()), ..Default::default() })
        .unwrap_err();
    assert_eq!(err.code(), myday_core::ErrorCode::Invalid);
    let patched = store
        .update_item(
            &log.id,
            ItemPatch { occurred_at: Some((Utc::now() - Duration::hours(1)).trunc_subsecs(0)), ..Default::default() },
        )
        .unwrap();
    assert_eq!(patched.occurred_at, Some((Utc::now() - Duration::hours(1)).trunc_subsecs(0)));
}

// ----------------------------------------------------------------------
// IPC
// ----------------------------------------------------------------------

#[test]
fn ipc_request_roundtrip_and_handler() {
    // 协议序列化稳定性：字段名即 API，禁止随意改动
    let req: IpcRequest =
        serde_json::from_str(r#"{"cmd":"add-item","new":{"item_type":"task","title":"买牛奶"}}"#)
            .unwrap();
    match &req {
        IpcRequest::AddItem { new } => {
            assert_eq!(new.title.as_deref(), Some("买牛奶"));
            assert_eq!(new.item_type, Some(ItemType::Task));
        }
        other => panic!("unexpected {other:?}"),
    }
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains(r#""cmd":"add-item""#));

    let show: IpcRequest =
        serde_json::from_str(r#"{"cmd":"show-quick-add","item_type":"event","title":"临时会议"}"#)
            .unwrap();
    assert!(matches!(show, IpcRequest::ShowQuickAdd { .. }));

    // 响应信封
    let ok = IpcResponse::ok(serde_json::json!({"pong": true}));
    let parsed: IpcResponse = serde_json::from_str(&serde_json::to_string(&ok).unwrap()).unwrap();
    assert!(parsed.ok && parsed.error.is_none());
}

#[test]
fn ipc_server_client_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("t.sock");

    struct Echo;
    impl IpcHandler for Echo {
        fn handle(&self, req: IpcRequest) -> IpcResponse {
            match req {
                IpcRequest::Ping => IpcResponse::ok(serde_json::json!("pong")),
                IpcRequest::Reveal { id } => IpcResponse::ok(serde_json::json!({ "revealed": id })),
                _ => IpcResponse::err(myday_core::ErrorCode::Unsupported, "no"),
            }
        }
    }

    let listener = ipc::bind(&sock).expect("bind");
    std::thread::spawn(move || ipc::serve(listener, Arc::new(Echo)));

    let resp = ipc::send_to(&sock, &IpcRequest::Ping).unwrap();
    assert!(resp.ok);
    assert_eq!(resp.data.unwrap(), serde_json::json!("pong"));

    let resp = ipc::send_to(&sock, &IpcRequest::Reveal { id: "evt_1".into() }).unwrap();
    assert_eq!(resp.data.unwrap()["revealed"], "evt_1");
}

// ----------------------------------------------------------------------
// 旧库处置：备份 + DROP 重建（不考虑历史数据）
// ----------------------------------------------------------------------

#[test]
fn legacy_db_is_backed_up_then_rebuilt() {
    let t = TempDir::new();
    let db = t.db_path();
    {
        // 手工造一个"旧结构"库（无 occurred_at，user_version = 0）
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );
             INSERT INTO items (id, type, title, created_at, updated_at)
             VALUES ('old_1', 'task', '旧数据', '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z');",
        )
        .unwrap();
    }
    let store = Store::open(&db, t.0.path()).unwrap();
    // 旧表被 DROP，新 schema 就位并有种子
    assert!(store.get_item("old_1").is_err(), "旧数据不保留");
    assert!(store.list_templates().unwrap().iter().any(|x| x.id == "tpl_water"));
    let conn = store.raw_conn().unwrap();
    let occurred: bool = conn
        .prepare("SELECT COUNT(*) FROM pragma_table_info('items') WHERE name = 'occurred_at'")
        .unwrap()
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|n| n > 0)
        .unwrap();
    drop(conn);
    assert!(occurred, "新 schema 含 occurred_at");
    // 备份文件存在且含旧数据
    let mut backups = std::fs::read_dir(t.0.path().join("backups")).unwrap();
    let backup = backups.next().expect("备份文件应存在").unwrap();
    let bconn = rusqlite::Connection::open(backup.path()).unwrap();
    let n: i64 = bconn.query_row("SELECT COUNT(*) FROM items", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "备份里保留旧数据");
}

// ----------------------------------------------------------------------
// 迁移链（1.0 数据承诺）：基线及以上只追加迁移，绝不重建
// ----------------------------------------------------------------------

#[test]
fn baseline_db_migrates_in_place_without_rebuild() {
    let t = TempDir::new();
    let db = t.db_path();
    let store = Store::open(&db, t.0.path()).unwrap();
    let id = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("迁移不丢我".into()),
            due_at: Some(Utc::now()),
            ..Default::default()
        })
        .unwrap()
        .id;
    {
        // 把 user_version 降到基线，模拟上一版本库；重开必须走迁移链
        let conn = store.raw_conn().unwrap();
        conn.pragma_update(None, "user_version", 4).unwrap();
    }
    drop(store);
    let store2 = Store::open(&db, t.0.path()).unwrap();
    assert!(store2.get_item(&id).is_ok(), "迁移不得丢数据");
    let version: i64 = {
        let conn = store2.raw_conn().unwrap();
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(version, 5, "迁移链把库升到当前版本");
    let builtin_views: i64 = {
        let conn = store2.raw_conn().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM view_defs WHERE builtin = 1",
            [],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(builtin_views > 0, "v4→v5 迁移补齐视图种子");
}



mod window_and_tokens {
    use super::*;
    use chrono::TimeZone;

    fn day_bounds_utc(y: i32, m: u32, d: u32) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
        // 测试内直接用 UTC 边界写库 / 查询（窗口查询本身只比较时间戳）
        let from = chrono::Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap();
        let to = chrono::Utc
            .with_ymd_and_hms(y, m, d, 23, 59, 59)
            .unwrap();
        (from, to)
    }

    #[test]
    fn window_query_covers_all_builtin_time_columns() {
        let t = TempDir::new();
        let store = t.store();
        let base = chrono::Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap();

        // 1. 普通日程：当天开始
        store.add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("当天日程".into()),
            start_at: Some(base + Duration::hours(10)),
            end_at: Some(base + Duration::hours(11)),
            ..Default::default()
        }).unwrap();

        // 2. 跨天日程：15 号开始 17 号结束 → 16 日窗口按区间相交命中
        store.add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("跨天日程".into()),
            start_at: Some(base - Duration::hours(20)),
            end_at: Some(base + Duration::hours(30)),
            ..Default::default()
        }).unwrap();

        // 3. due-only 待办（无 start_at）：旧日历完全看不见
        store.add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("当天到期".into()),
            due_at: Some(base + Duration::hours(18)),
            ..Default::default()
        }).unwrap();

        // 4. 记录：occurred_at 当天
        store.add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("当天记录".into()),
            occurred_at: Some(base + Duration::hours(8)),
            ..Default::default()
        }).unwrap();

        // 干扰项：17 号的日程 / 15 号的记录
        store.add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("隔天日程".into()),
            start_at: Some(base + Duration::hours(30)),
            end_at: Some(base + Duration::hours(31)),
            ..Default::default()
        }).unwrap();
        store.add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("前一天记录".into()),
            occurred_at: Some(base - Duration::hours(5)),
            ..Default::default()
        }).unwrap();

        let (from, to) = day_bounds_utc(2026, 9, 16);
        let got = store.list_items_window(from, to, None).unwrap();
        let titles: Vec<&str> = got.iter().map(|i| i.title.as_deref().unwrap()).collect();
        assert!(titles.contains(&"当天日程"));
        assert!(titles.contains(&"跨天日程"), "跨天事件按区间相交命中");
        assert!(titles.contains(&"当天到期"), "due-only 待办命中");
        assert!(titles.contains(&"当天记录"));
        assert!(!titles.contains(&"隔天日程"));
        assert!(!titles.contains(&"前一天记录"));

        // 按类型过滤
        let events = store.list_items_window(from, to, Some(ItemType::Event)).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn list_filter_column_ranges() {
        let t = TempDir::new();
        let store = t.store();
        let base = chrono::Utc.with_ymd_and_hms(2026, 9, 16, 0, 0, 0).unwrap();
        store.add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("有开始的任务".into()),
            start_at: Some(base + Duration::hours(9)),
            due_at: Some(base + Duration::hours(18)),
            ..Default::default()
        }).unwrap();
        // start 范围过滤（AND 语义）
        let got = store.list_items(&ListFilter {
            start_from: Some(base + Duration::hours(8)),
            start_to: Some(base + Duration::hours(10)),
            ..Default::default()
        }).unwrap();
        assert_eq!(got.len(), 1);
        let got = store.list_items(&ListFilter {
            start_from: Some(base + Duration::hours(20)),
            ..Default::default()
        }).unwrap();
        assert!(got.is_empty());
    }

    #[test]
    fn template_time_tokens_resolve_at_apply() {
        let t = TempDir::new();
        let store = t.store();

        let tpl = store
            .add_template(
                "晨会",
                None,
                None,
                ItemType::Event,
                &serde_json::json!({
                    "title": "晨会",
                    "start_at": "@d0T09:00",
                    "end_at": "@start+30m"
                }),
                &serde_json::json!([]),
                None,
            )
            .unwrap();

        // 指定锚点日：2026-09-20（本地日，core 解析 day 族用）
        let anchor = chrono::NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        let item = store
            .add_item(NewItem {
                template_id: Some(tpl.id.clone()),
                anchor_day: Some(anchor),
                ..Default::default()
            })
            .unwrap();
        let start = item.start_at.unwrap().with_timezone(&chrono::Local);
        let end = item.end_at.unwrap().with_timezone(&chrono::Local);
        assert_eq!(start.date_naive(), anchor, "当天 9 点 = 锚点日");
        assert_eq!(start.format("%H:%M").to_string(), "09:00");
        assert_eq!(
            (end - start).num_minutes(),
            30,
            "@start+30m = 开始后半小时"
        );

        // 不指定锚点 = 今天
        let today = store
            .add_item(NewItem { template_id: Some(tpl.name.clone()), ..Default::default() })
            .unwrap()
            .start_at
            .unwrap()
            .with_timezone(&chrono::Local)
            .date_naive();
        assert_eq!(today, chrono::Local::now().date_naive());
    }

    #[test]
    fn template_token_validation_rejects_bad_and_future_occurred() {
        let t = TempDir::new();
        let store = t.store();

        assert!(store
            .add_template("坏token", None, None, ItemType::Event,
                &serde_json::json!({ "start_at": "@foo" }), &serde_json::json!([]), None)
            .is_err(), "未知 token 拒绝");
        assert!(store
            .add_template("start基准滥用", None, None, ItemType::Event,
                &serde_json::json!({ "start_at": "@start+30m" }), &serde_json::json!([]), None)
            .is_err(), "@start 仅 end_at 可用");
        assert!(store
            .add_template("未来记录", None, None, ItemType::Log,
                &serde_json::json!({ "occurred_at": "@d+1T09:00" }), &serde_json::json!([]), None)
            .is_err(), "occurred_at 不允许未来方向占位");
        assert!(store
            .add_template("列误用", None, None, ItemType::Log,
                &serde_json::json!({ "occurred_at": "@now-30m" }), &serde_json::json!([]), None)
            .is_ok(), "log 模板允许过去方向 occurred 占位");
        assert!(store
            .add_template("绝对时间默认", None, None, ItemType::Event,
                &serde_json::json!({ "start_at": "2026-09-20T09:00:00Z" }), &serde_json::json!([]), None)
            .is_err(), "时间默认只收 @ 占位（模板存意图，条目存事实）");
    }
}

#[test]
fn task_auto_reminder_never_anchors_on_due() {
    let t = TempDir::new();
    let store = t.store();

    // 只有截止的待办：不自动提醒（截止是期限不是发生时刻）
    let due_only = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("买牛奶".into()),
            due_at: Some(Utc::now() + Duration::hours(30)),
            ..Default::default()
        })
        .unwrap();
    assert!(due_only.reminders.is_empty(), "待办自动提醒不按截止算");

    // 带开始的待办：按开始提前提醒（开始取未来时刻，避免当天时刻造成的波动）
    let with_start = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("专注写作".into()),
            start_at: Some(Utc::now().trunc_subsecs(0) + Duration::hours(5)),
            due_at: Some(Utc::now() + Duration::hours(9)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(with_start.reminders.len(), 1);
    assert_eq!(
        with_start.reminders[0].spec, "@start-10m",
        "锚开始，不锚截止"
    );
}

#[test]
fn relative_reminder_spec_follows_time_changes() {
    use myday_core::reminder::occurrences;
    let t = TempDir::new();
    let store = t.store();

    // @due-1h：截止改动后提醒自动跟随（相对意图的核心收益）
    let item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("交报告".into()),
            due_at: Some(Utc::now() + Duration::hours(48)),
            reminders: vec![NewReminder {
                spec: "@due-1h".into(),
                channel: "notify".into(),
            }],
            ..Default::default()
        })
        .unwrap();
    let occ = occurrences("@due-1h", &item);
    assert_eq!(occ, vec![item.due_at.unwrap() - Duration::hours(1)]);

    // 改截止 → 同一条 spec 展开出新时刻，无需改提醒
    let updated = store
        .update_item(
            &item.id,
            ItemPatch {
                due_at: Some(Utc::now() + Duration::hours(4)),
                ..Default::default()
            },
        )
        .unwrap();
    let occ = occurrences("@due-1h", &updated);
    assert_eq!(occ, vec![updated.due_at.unwrap() - Duration::hours(1)]);

    // 锚点缺失 → 无发生时刻（宁可不响）
    let bare = occurrences("@due-1h", &{
        let mut i = updated.clone();
        i.due_at = None;
        i
    });
    assert!(bare.is_empty());

    // 非法 spec 静默为空 + 入库被拒
    assert!(occurrences("@foo", &updated).is_empty());
    assert!(store
        .add_item(NewItem {
            item_type: Some(ItemType::Task),
            title: Some("坏spec".into()),
            reminders: vec![NewReminder { spec: "@foo".into(), channel: "notify".into() }],
            ..Default::default()
        })
        .is_err());
}

#[test]
fn daily_spec_expands_per_day_and_dedups() {
    use myday_core::reminder::occurrences;
    let t = TempDir::new();
    let store = t.store();

    // 日程跨 4+ 天（相对现在，保证历史侧必有已到期时刻）
    let event = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("出差".into()),
            start_at: Some(Utc::now() - Duration::days(3)),
            end_at: Some(Utc::now() + Duration::days(3)),
            reminders: vec![NewReminder {
                spec: "@dailyT09:00".into(),
                channel: "notify".into(),
            }],
            ..Default::default()
        })
        .unwrap();

    let occ = occurrences("@dailyT09:00", &event);
    assert!((4..=7).contains(&occ.len()), "7 天期间应展开 5~7 个时刻，实际 {}", occ.len());
    assert!(occ.windows(2).all(|w| w[0] < w[1]), "升序且无重复");

    // 第一轮全部到期时刻要么补发（窗口内）要么聚合（窗口外错过）；第二轮双零（reminder_log 去重）
    struct Tally(AtomicUsize, AtomicUsize);
    impl myday_core::reminder::Notifier for Tally {
        fn notify(&self, _: &Reminder, _: &Item, _: chrono::DateTime<chrono::Utc>, _: &[&'static str]) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn notify_missed(&self, count: usize, _lines: &[(String, String)]) {
            self.1.fetch_add(count, Ordering::SeqCst);
        }
    }
    let tally = Tally(AtomicUsize::new(0), AtomicUsize::new(0));
    let first = tick_once(&store, &tally).unwrap();
    let missed = tally.1.load(Ordering::SeqCst);
    assert!(
        first + missed >= 1,
        "到期每日提醒应至少处理一条：sent={first} missed={missed}"
    );
    assert_eq!(tally.0.load(Ordering::SeqCst), first);
    assert_eq!(tick_once(&store, &tally).unwrap(), 0);
    assert_eq!(tally.1.load(Ordering::SeqCst), missed, "错过项只聚合一次");

    // log 无期间：每日提醒不展开
    let log_item = store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("记录".into()),
            occurred_at: Some(Utc::now()),
            ..Default::default()
        })
        .unwrap();
    assert!(occurrences("@dailyT09:00", &log_item).is_empty());
    let _ = store.delete_item(&event.id).unwrap();
}

// ----------------------------------------------------------------------
// SPRINT2：类型转换 / 提醒中心 / 统计范围 / ICS 导出 / 备份 zip
// ----------------------------------------------------------------------

#[test]
fn convert_task_to_event_moves_everything() {
    let t = TempDir::new();
    let store = t.store();
    let due = (Utc::now() + Duration::days(1)).trunc_subsecs(0);
    let task = store
        .add_item(NewItem {
            title: Some("交周报".into()),
            note: Some("带附件与提醒的待办".into()),
            due_at: Some(due),
            tags: vec!["工作".into()],
            extra: serde_json::json!({"fd_priority": "高"}),
            reminders: vec![
                NewReminder { spec: "@due-30m".into(), channel: "notify".into() },
                NewReminder {
                    spec: (Utc::now() + Duration::hours(2)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    channel: "notify".into(),
                },
            ],
            ..Default::default()
        })
        .unwrap();
    store.add_attachment_bytes(&task.id, b"png-bytes", "png").unwrap();

    let ev = store.convert_task_to_event(&task.id).unwrap();
    assert_eq!(ev.item_type, ItemType::Event);
    assert_eq!(ev.start_at, Some(due));
    assert_eq!(ev.end_at, Some(due + Duration::hours(1)));
    assert_eq!(ev.tags, vec!["工作".to_string()]);
    assert_eq!(ev.extra.get("fd_priority"), None, "task 专属字段（优先级）不入日程");
    // 提醒映射：@due-30m → @start-30m；绝对时刻原样
    assert!(ev.reminders.iter().any(|r| r.spec == "@start-30m"));
    assert_eq!(ev.reminders.len(), 2);
    // 附件迁移：行指向新条目、文件在新目录
    assert_eq!(ev.attachments.len(), 1);
    let att = &ev.attachments[0];
    assert_eq!(att.item_id, ev.id);
    assert!(att.rel_path.starts_with(&format!("attachments/{}/", ev.id)));
    assert!(t.0.path().join(&att.rel_path).exists(), "附件文件已迁移");
    // 原待办已删
    assert!(store.get_item(&task.id).is_err());
}

#[test]
fn event_to_log_copies_and_keeps_event() {
    let t = TempDir::new();
    let store = t.store();
    let start = (Utc::now() - Duration::hours(2)).trunc_subsecs(0);
    let ev = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("跑步局".into()),
            start_at: Some(start),
            end_at: Some(start + Duration::hours(1)),
            tags: vec!["运动".into()],
            ..Default::default()
        })
        .unwrap();
    store.add_attachment_bytes(&ev.id, b"img-data", "png").unwrap();
    let ev = store.get_item(&ev.id).unwrap();

    let log = store.event_to_log(&ev.id).unwrap();
    assert_eq!(log.item_type, ItemType::Log);
    assert_eq!(log.occurred_at, Some(start));
    assert_eq!(log.tags, vec!["运动".to_string()]);
    // 附件是独立副本：两行两文件
    assert_eq!(log.attachments.len(), 1);
    assert_ne!(log.attachments[0].rel_path, ev.attachments[0].rel_path);
    assert!(t.0.path().join(&log.attachments[0].rel_path).exists());
    // 原日程保留
    let still = store.get_item(&ev.id).unwrap();
    assert_eq!(still.item_type, ItemType::Event);
    assert_eq!(still.attachments.len(), 1);

    // 未来日程不能生成记录
    let future = store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("未来".into()),
            start_at: Some(Utc::now() + Duration::hours(1)),
            end_at: Some(Utc::now() + Duration::hours(2)),
            ..Default::default()
        })
        .unwrap();
    assert!(store.event_to_log(&future.id).is_err());
}

#[test]
fn reminder_history_and_unread_count() {
    let t = TempDir::new();
    let store = t.store();
    let abs = |mins: i64| {
        (Utc::now() - Duration::minutes(mins)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    };
    store
        .add_item(NewItem {
            title: Some("到点的事".into()),
            reminders: vec![NewReminder { spec: abs(10), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();

    struct Noop;
    impl Notifier for Noop {
        fn notify(&self, _: &Reminder, _: &Item, _: chrono::DateTime<chrono::Utc>, _: &[&'static str]) {}
        fn notify_missed(&self, _: usize, _: &[(String, String)]) {}
    }
    tick_once(&store, &Noop).unwrap();

    let hist = store.reminder_history(10).unwrap();
    assert_eq!(hist.len(), 1, "提醒历史一条");
    assert_eq!(hist[0].item.title.as_deref(), Some("到点的事"));

    // 未读：seen 之前 0 条，seen 早于 remind_at 则 1 条
    assert_eq!(store.reminder_unread_count(Some(Utc::now())).unwrap(), 0);
    assert_eq!(
        store.reminder_unread_count(Some(Utc::now() - Duration::hours(1))).unwrap(),
        1
    );
    assert_eq!(store.reminder_unread_count(None).unwrap(), 0, "首次使用无未读");
}

#[test]
fn stats_summary_respects_days_window() {
    let t = TempDir::new();
    let store = t.store();
    let mk_log = |days_ago: i64, weight: f64| {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some("体重".into()),
                occurred_at: Some(Utc::now() - Duration::days(days_ago)),
                extra: serde_json::json!({"fd_weight_kg": weight}),
                ..Default::default()
            })
            .unwrap()
    };
    mk_log(40, 70.0); // 窗口外
    mk_log(5, 71.5);  // 窗口内
    mk_log(2, 72.0);  // 窗口内
    let s30 = store.stats_summary(30).unwrap();
    assert_eq!(s30.heatmap.len(), 30, "热力图按窗口天数");
    assert_eq!(s30.heatmap.iter().map(|d| d.count).sum::<i64>(), 2);
    let series = s30.series.iter().find(|s| s.field_id == "fd_weight_kg").unwrap();
    assert_eq!(series.points.len(), 2, "40 天前的点不在 30 天窗口");
    let s365 = store.stats_summary(365).unwrap();
    assert_eq!(s365.heatmap.len(), 365);
    let series = s365.series.iter().find(|s| s.field_id == "fd_weight_kg").unwrap();
    assert_eq!(series.points.len(), 3);
    // 下限保护
    assert_eq!(store.stats_summary(1).unwrap().heatmap.len(), 7, "窗口下限 7 天");
}

#[test]
fn export_ics_events_todos_alarms() {
    let t = TempDir::new();
    let store = t.store();
    let base = (Utc::now() + Duration::days(1)).trunc_subsecs(0);
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: Some("评审会;专场,第一幕".into()),
            start_at: Some(base),
            end_at: Some(base + Duration::hours(1)),
            recurrence: Some("@weekly:3".into()),
            reminders: vec![NewReminder { spec: "@start-10m".into(), channel: "notify".into() }],
            tags: vec!["工作".into()],
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem {
            title: Some("交周报".into()),
            due_at: Some(base + Duration::days(2)),
            reminders: vec![NewReminder { spec: "@due-1h".into(), channel: "notify".into() }],
            ..Default::default()
        })
        .unwrap();
    let done = store
        .add_item(NewItem {
            title: Some("已完成的待办".into()),
            due_at: Some(base - Duration::days(1)),
            ..Default::default()
        })
        .unwrap();
    store.complete_task(&done.id).unwrap();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("不该出现的记录".into()),
            occurred_at: Some(Utc::now()),
            ..Default::default()
        })
        .unwrap();

    let ics = myday_core::ics::export_ics(&store).unwrap();
    assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
    assert!(ics.ends_with("END:VCALENDAR\r\n"));
    assert!(ics.contains("BEGIN:VEVENT"));
    assert!(ics.contains("RRULE:FREQ=WEEKLY;BYDAY=WE"));
    assert!(ics.contains("TRIGGER:-PT10M"));
    assert!(ics.contains("SUMMARY:评审会\\;专场\\,第一幕"), "TEXT 转义");
    assert!(ics.contains("myday://item/evt_"), "回链");
    assert!(ics.contains("CATEGORIES:工作"));
    assert!(ics.contains("BEGIN:VTODO"));
    assert!(ics.contains("TRIGGER;RELATED=END:-PT1H"));
    assert!(ics.contains("STATUS:COMPLETED"));
    assert!(!ics.contains("不该出现的记录"), "log 不导出");
    // 折行：任意物理行不超过 75 字节
    for line in ics.split("\r\n") {
        assert!(line.len() <= 76, "行超长: {line}"); // 75 + 续行前导空格
    }
}

#[test]
fn backup_zip_contains_db_and_attachments_and_rotates() {
    let t = TempDir::new();
    let store = t.store();
    let item = store
        .add_item(NewItem {
            title: Some("有附件".into()),
            ..Default::default()
        })
        .unwrap();
    store.add_attachment_bytes(&item.id, b"file-body", "png").unwrap();

    // 预置 7 份旧备份，触发轮换
    let dir = t.0.path().join("backups");
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..7 {
        std::fs::write(dir.join(format!("myday-backup-2020010{i}-000000.zip")), b"old").unwrap();
    }

    let target = myday_core::backup::backup_zip(&store).unwrap();
    assert!(target.exists());
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("zip"))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 7, "轮换后仍为 7 份");

    // 解包校验：含 myday.db 与附件条目
    let f = std::fs::File::open(&target).unwrap();
    let ar = zip::ZipArchive::new(f).unwrap();
    let mut names: Vec<String> = ar.file_names().map(str::to_string).collect();
    names.sort();
    assert!(names.iter().any(|n| n == "myday.db"));
    assert!(names.iter().any(|n| n.starts_with("attachments/")), "附件入包: {names:?}");
}

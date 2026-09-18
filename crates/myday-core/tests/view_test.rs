//! 视图模型引擎测试（FILTER-SPEC §13 P1 完成判据）：
//! 运算符矩阵、日期值配对封闭、种子语义对齐 tasks_view、重复待办母条目模式、
//! 预筛下推 OR 并集、挂件三段管线（聚合 → 结果表 → 派生）、view_defs CRUD、
//! 存储层不变量（重复待办 due 必填 / 取消完成统一写路径）。

use chrono::{Datelike, Duration, TimeZone, Utc};

use myday_core::model::*;
use myday_core::store::{ListFilter, Store, TaskView};
use myday_core::view::{
    self, Condition, Cmp, EvalCtx, FilterNode, FilterValue, Logic, Panel,
    VIEW_LOGS_TIMELINE, VIEW_SEARCH_ALL, VIEW_STATS_STREAKS,
    VIEW_TASKS_ALL, VIEW_TASKS_DONE, VIEW_TASKS_TODAY, VIEW_TASKS_UPCOMING,
};

struct TempDir(tempfile::TempDir);

impl TempDir {
    fn new() -> Self {
        Self(tempfile::tempdir().expect("tempdir"))
    }
    fn store(&self) -> Store {
        Store::open(&self.0.path().join("myday.db"), self.0.path()).expect("open store")
    }
}

/// 以当前时刻为基准的 UTC 时刻（今天 + 偏移天数 + 钟点，按本地时区）。
fn at(day_offset: i64, h: u32, m: u32) -> chrono::DateTime<Utc> {
    let today = chrono::Local::now().date_naive();
    let d = today + Duration::days(day_offset);
    chrono::Local
        .with_ymd_and_hms(d.year(), d.month(), d.day(), h, m, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// 恒为过去的「结构锚点日」：该日任意时刻都至少早于现在 12 小时。
/// log 场景（add_item 拒绝未来发生时间）用本助手构造「今天」，跨午夜 / 清晨
/// 运行不再误伤（与 store_test 的 cf90a32 同类时间敏感问题）。
fn past_day(day_shift: i64) -> chrono::NaiveDate {
    (chrono::Local::now() - Duration::hours(36)).date_naive() - Duration::days(day_shift)
}

fn past_at(day_shift: i64, h: u32, m: u32) -> chrono::DateTime<Utc> {
    let d = past_day(day_shift);
    chrono::Local
        .with_ymd_and_hms(d.year(), d.month(), d.day(), h, m, 0)
        .single()
        .unwrap()
        .with_timezone(&Utc)
}

/// 「今天或昨天」（连续计数的两种起算都成立）且恒为过去的时刻
fn recent_log_at(hours_ago: i64) -> chrono::DateTime<Utc> {
    Utc::now() - Duration::hours(hours_ago)
}

fn cond(field: &str, cmp: Cmp, value: Option<FilterValue>) -> FilterNode {
    FilterNode::Cond(Condition { field: field.into(), cmp, value })
}

fn and(children: Vec<FilterNode>) -> FilterNode {
    FilterNode::Group { op: Logic::And, children }
}

fn or(children: Vec<FilterNode>) -> FilterNode {
    FilterNode::Group { op: Logic::Or, children }
}

fn user_view_config(filter: FilterNode) -> serde_json::Value {
    serde_json::json!({
        "dataset": {
            "item_type": "task",
            "filter": filter,
            "sort": [{ "field": "col:anchor", "dir": "asc" }],
            "limit": 100
        },
        "layout": "list"
    })
}

// ----------------------------------------------------------------------
// 种子视图：语义逐分支对齐 tasks_view（§9 / §13 P1 判据）
// ----------------------------------------------------------------------

#[test]
fn builtin_task_views_align_with_tasks_view() {
    let t = TempDir::new();
    let store = t.store();

    // 有截止只看截止：截止在今天 → 今天；明天 → 即将到期
    let due_today = store
        .add_item(NewItem {
            title: Some("今天截止".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 18, 0)),
            ..Default::default()
        })
        .unwrap();
    let due_tomorrow = store
        .add_item(NewItem {
            title: Some("明天截止".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(1, 9, 0)),
            ..Default::default()
        })
        .unwrap();
    // 逾期三天 → 今天（含逾期）
    let overdue = store
        .add_item(NewItem {
            title: Some("逾期".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(-3, 9, 0)),
            ..Default::default()
        })
        .unwrap();
    // 无截止看开始：开始在今天 → 今天；明天 → 即将到期
    let start_today = store
        .add_item(NewItem {
            title: Some("今天开始".into()),
            item_type: Some(ItemType::Task),
            start_at: Some(at(0, 8, 0)),
            ..Default::default()
        })
        .unwrap();
    let start_tomorrow = store
        .add_item(NewItem {
            title: Some("明天开始".into()),
            item_type: Some(ItemType::Task),
            start_at: Some(at(1, 8, 0)),
            ..Default::default()
        })
        .unwrap();
    // 全无 = 收件箱：进今天、排序最后
    let inbox = store
        .add_item(NewItem { title: Some("收件箱".into()), item_type: Some(ItemType::Task), ..Default::default() })
        .unwrap();
    // 有截止的「开始在过去」不进今天（截止优先，§4.1 有意差异）
    let due_future_start_past = store
        .add_item(NewItem {
            title: Some("截止优先".into()),
            item_type: Some(ItemType::Task),
            start_at: Some(at(-5, 8, 0)),
            due_at: Some(at(5, 9, 0)),
            ..Default::default()
        })
        .unwrap();
    let done = store
        .add_item(NewItem {
            title: Some("已完成".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 10, 0)),
            status: Some(ItemStatus::Done),
            ..Default::default()
        })
        .unwrap();

    for (view_id, tv) in [
        (VIEW_TASKS_TODAY, TaskView::Today),
        (VIEW_TASKS_UPCOMING, TaskView::Upcoming),
        (VIEW_TASKS_ALL, TaskView::All),
        (VIEW_TASKS_DONE, TaskView::Done),
    ] {
        let via_view = store.query_view(view_id, None, None).unwrap();
        let ids: Vec<String> = via_view.items.unwrap().into_iter().map(|i| i.id).collect();
        let via_sql: Vec<String> = store
            .tasks_view(tv, None)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        if view_id == VIEW_TASKS_ALL {
            // All 的种子排序 = anchor asc（§9，start_at 有值不算空），
            // tasks_view 的 SQL 按 due 排——本实现按规格，仅要求母条目集合一致
            let mut a = ids.clone();
            let mut b = via_sql.clone();
            a.sort();
            b.sort();
            assert_eq!(a, b, "视图 {view_id} 与 tasks_view 集合一致");
        } else {
            assert_eq!(ids, via_sql, "视图 {view_id} 与 tasks_view 集合+顺序必须一致");
        }
    }

    // 语义抽查
    let today = store.query_view(VIEW_TASKS_TODAY, None, None).unwrap().items.unwrap();
    let today_ids: Vec<&str> = today.iter().map(|i| i.id.as_str()).collect();
    for id in [&due_today.id, &overdue.id, &start_today.id, &inbox.id] {
        assert!(today_ids.contains(&id.as_str()), "{id} 应在今天");
    }
    for id in [&due_tomorrow.id, &start_tomorrow.id, &due_future_start_past.id, &done.id] {
        assert!(!today_ids.contains(&id.as_str()), "{id} 不应在今天");
    }
    // 收件箱（anchor 空）恒排该级末尾
    assert_eq!(today.last().unwrap().id, inbox.id);

    let upcoming = store.query_view(VIEW_TASKS_UPCOMING, None, None).unwrap().items.unwrap();
    let up_ids: Vec<&str> = upcoming.iter().map(|i| i.id.as_str()).collect();
    assert!(up_ids.contains(&due_tomorrow.id.as_str()));
    assert!(up_ids.contains(&start_tomorrow.id.as_str()));
    assert!(up_ids.contains(&due_future_start_past.id.as_str()));

    let all = store.query_view(VIEW_TASKS_ALL, None, None).unwrap().items.unwrap();
    assert_eq!(all.len(), 7, "All = 全部未完成（不含已完成）");

    let done_view = store.query_view(VIEW_TASKS_DONE, None, None).unwrap().items.unwrap();
    assert_eq!(done_view.len(), 1);
    assert_eq!(done_view[0].id, done.id);
}

#[test]
fn recurring_task_stays_single_row_mother_mode() {
    let t = TempDir::new();
    let store = t.store();
    // 重复待办：due 恒为当前期（行内单行），进今天/即将到期都只出现一次
    store
        .add_item(NewItem {
            title: Some("每周报告".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 17, 0)),
            recurrence: Some("@weekly:1".into()),
            ..Default::default()
        })
        .unwrap();
    let today = store.query_view(VIEW_TASKS_TODAY, None, None).unwrap().items.unwrap();
    assert_eq!(today.iter().filter(|i| i.title.as_deref() == Some("每周报告")).count(), 1);
    let sql = store.tasks_view(TaskView::Today, None).unwrap();
    assert_eq!(today.len(), sql.len(), "母条目集合与 tasks_view 一致");
}

#[test]
fn logs_timeline_groups_desc_with_null_last() {
    let t = TempDir::new();
    let store = t.store();
    for (off, h) in [(0i64, 9u32), (0, 8), (1, 20), (2, 10)] {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some(format!("记录 {off}/{h}")),
                occurred_at: Some(past_at(off, h, 0)),
                ..Default::default()
            })
            .unwrap();
    }
    let r = store.query_view(VIEW_LOGS_TIMELINE, None, None).unwrap();
    let groups = r.groups.expect("种子视图带 day 分组");
    assert_eq!(groups.len(), 3, "按本地日倒序分三组");
    let keys: Vec<&str> = groups.iter().filter_map(|g| g.key.as_deref()).collect();
    assert!(keys[0] > keys[1] && keys[1] > keys[2], "组键倒序: {keys:?}");
    // 组内按 sort（occurred desc）
    let first_day: Vec<String> = groups[0].items.iter().map(|i| i.title.clone().unwrap()).collect();
    assert_eq!(first_day, vec!["记录 0/9", "记录 0/8"]);

    // 时间线视图不包含 task / event
    let total = r.total;
    assert_eq!(total, 4);
}

#[test]
fn search_view_keyword_compiles_into_ast() {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            title: Some("客户沟通会".into()),
            item_type: Some(ItemType::Event),
            start_at: Some(at(0, 10, 0)),
            end_at: Some(at(0, 11, 0)),
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem {
            title: Some("买菜".into()),
            note: Some("要买西红柿".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(1, 9, 0)),
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("喝水".into()),
            tags: vec!["健康".into()],
            occurred_at: Some(past_at(0, 8, 0)),
            extra: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
    // 打了体重字段值的记录（字段值命中）
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("晨重".into()),
            occurred_at: Some(past_at(0, 7, 30)),
            extra: serde_json::json!({ "fd_weight_kg": 72.5 }),
            ..Default::default()
        })
        .unwrap();

    let r = store
        .query_view(VIEW_SEARCH_ALL, Some("西红柿"), None)
        .unwrap();
    let items = r.items.unwrap();
    assert_eq!(items.len(), 1, "备注命中");
    let r = store.query_view(VIEW_SEARCH_ALL, Some("健康"), None).unwrap();
    assert_eq!(r.items.unwrap().len(), 1, "标签命中");
    let r = store.query_view(VIEW_SEARCH_ALL, Some("72.5"), None).unwrap();
    assert_eq!(r.items.unwrap().len(), 1, "字段值命中");
    let r = store.query_view(VIEW_SEARCH_ALL, Some("客户"), None).unwrap();
    let items = r.items.unwrap();
    assert_eq!(items.len(), 1);
    let matched = r.matched.unwrap();
    assert_eq!(matched.get(items[0].id.as_str()), Some(&vec!["title".to_string()]));

    // 排序 = updated desc（种子），后创建的「晨重」在前
    let r = store.query_view(VIEW_SEARCH_ALL, Some("喝水"), None).unwrap();
    assert_eq!(r.items.unwrap().len(), 1);
}

// ----------------------------------------------------------------------
// 运算符矩阵（§4.3）：文本 / 数字 / 单选 / 多选 / 布尔 / 日期
// ----------------------------------------------------------------------

fn eval_one(store: &Store, filter: FilterNode) -> usize {
    let cfg = user_view_config(and(vec![filter]));
    let v = store.create_view("临时", Panel::Tasks, &cfg).unwrap();
    let r = store.query_view(&v.id, None, None).unwrap();
    let _ = store.delete_view(&v.id);
    r.items.unwrap().len()
}

fn matrix_setup() -> (TempDir, Store) {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_field_def("数值", FieldKind::Number, &serde_json::json!({}), None)
        .unwrap();
    (t, store)
}

#[test]
fn operator_matrix_text_number_select_multiselect_bool() {
    let (_t, store) = matrix_setup();
    let defs = store.list_field_defs(None).unwrap();
    let num = defs.iter().find(|d| d.kind == FieldKind::Number && d.scope.is_none()).unwrap().id.clone();
    store
        .add_field_def("等级", FieldKind::Select, &serde_json::json!({"choices":["低","高"]}), None)
        .unwrap();
    store
        .add_field_def("标签组", FieldKind::MultiSelect, &serde_json::json!({"choices":["a","b","c"]}), None)
        .unwrap();
    store.add_field_def("开关", FieldKind::Bool, &serde_json::json!({}), None).unwrap();
    store.add_field_def("网址", FieldKind::Url, &serde_json::json!({}), None).unwrap();
    let defs = store.list_field_defs(None).unwrap();
    let sel = defs.iter().find(|d| d.kind == FieldKind::Select).unwrap().id.clone();
    let multi = defs.iter().find(|d| d.kind == FieldKind::MultiSelect).unwrap().id.clone();
    let flag = defs.iter().find(|d| d.kind == FieldKind::Bool).unwrap().id.clone();
    let url = defs.iter().find(|d| d.kind == FieldKind::Url).unwrap().id.clone();

    let mk = |extra: serde_json::Value, title: &str| {
        store
            .add_item(NewItem {
                title: Some(title.into()),
                item_type: Some(ItemType::Task),
                due_at: Some(at(0, 12, 0)),
                extra,
                ..Default::default()
            })
            .unwrap();
    };
    mk(
        serde_json::json!({ num.clone(): 42, sel.clone(): "高", multi.clone(): ["a","b"], flag.clone(): true, url.clone(): "https://example.com" }),
        "全有",
    );
    mk(serde_json::json!({ num.clone(): 7, sel.clone(): "低", multi.clone(): ["c"], flag.clone(): false }), "半有");
    mk(serde_json::json!({}), "全无");

    // 文本：contains / not_contains / empty / not_empty / eq（大小写不敏感）
    assert_eq!(eval_one(&store, cond("col:title", Cmp::Contains, Some(FilterValue::Text("全".into())))), 2);
    assert_eq!(eval_one(&store, cond("col:title", Cmp::NotContains, Some(FilterValue::Text("全".into())))), 1);
    assert_eq!(eval_one(&store, cond(&url, Cmp::NotEmpty, None)), 1);
    assert_eq!(eval_one(&store, cond(&url, Cmp::Empty, None)), 2);
    // 数字
    assert_eq!(eval_one(&store, cond(&num, Cmp::Gt, Some(FilterValue::Number(10.0)))), 1);
    assert_eq!(eval_one(&store, cond(&num, Cmp::Lte, Some(FilterValue::Number(42.0)))), 2);
    assert_eq!(
        eval_one(&store, cond(&num, Cmp::Between, Some(FilterValue::NumPair(serde_json::from_value(serde_json::json!({"from":5,"to":10})).unwrap())))),
        1
    );
    assert_eq!(eval_one(&store, cond(&num, Cmp::Empty, None)), 1);
    assert_eq!(eval_one(&store, cond(&num, Cmp::Eq, Some(FilterValue::Number(42.0)))), 1);
    // 单选
    assert_eq!(eval_one(&store, cond(&sel, Cmp::Eq, Some(FilterValue::Text("高".into())))), 1);
    assert_eq!(eval_one(&store, cond(&sel, Cmp::Neq, Some(FilterValue::Text("高".into())))), 1, "neq 不含空值行（空值恒不命中 eq/neq）");
    // 多选
    assert_eq!(
        eval_one(&store, cond(&multi, Cmp::Any, Some(FilterValue::StrList(vec!["a".into(), "c".into()])))),
        2
    );
    assert_eq!(
        eval_one(&store, cond(&multi, Cmp::All, Some(FilterValue::StrList(vec!["a".into(), "b".into()])))),
        1
    );
    assert_eq!(
        eval_one(&store, cond(&multi, Cmp::HasNone, Some(FilterValue::StrList(vec!["a".into()])))),
        2
    );
    // 布尔：未填 ≠ false
    assert_eq!(eval_one(&store, cond(&flag, Cmp::IsTrue, None)), 1);
    assert_eq!(eval_one(&store, cond(&flag, Cmp::IsFalse, None)), 1);
    assert_eq!(eval_one(&store, cond(&flag, Cmp::Empty, None)), 1);
    // 标签列（多值语义一致）
    store
        .add_item(NewItem {
            title: Some("带标签".into()),
            item_type: Some(ItemType::Task),
            tags: vec!["工作".into(), "紧急".into()],
            due_at: Some(at(0, 13, 0)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        eval_one(&store, cond("col:tags", Cmp::Any, Some(FilterValue::StrList(vec!["紧急".into()])))),
        1
    );
    assert_eq!(
        eval_one(&store, cond("col:tags", Cmp::Contains, Some(FilterValue::Text("紧".into())))),
        1
    );
    // 日期列 + 相对值：due before tomorrow（矩阵条目全带 due）
    assert_eq!(
        eval_one(&store, cond("col:due_at", Cmp::Before, Some(FilterValue::Date(view::DateValue::RelPoint(view::RelDay::Tomorrow))))),
        4
    );
    // 「今天或之前」写 before tomorrow
    assert_eq!(
        eval_one(&store, cond("col:anchor", Cmp::Before, Some(FilterValue::Date(view::DateValue::RelPoint(view::RelDay::Tomorrow))))),
        4
    );
}

#[test]
fn date_value_pairing_is_closed() {
    let t = TempDir::new();
    let store = t.store();
    // 点值配 within / 范围值配其余日期运算符 = 参数错误
    let fields = store.list_field_defs(None).unwrap();
    let bad1 = serde_json::json!({
        "dataset": { "item_type": "all", "filter": { "op": "and", "children": [
            { "field": "col:due_at", "cmp": "within", "value": { "day": "2026-09-17" } }
        ] } }
    });
    assert!(myday_core::view::validate_view_config(&bad1, &fields).is_err());
    let bad2 = serde_json::json!({
        "dataset": { "item_type": "all", "filter": { "op": "and", "children": [
            { "field": "col:due_at", "cmp": "on", "value": { "rel": "this_week" } }
        ] } }
    });
    assert!(myday_core::view::validate_view_config(&bad2, &fields).is_err());
    // last_days:0 = 参数错误
    assert!(view::parse_date_value(&serde_json::json!({"rel":"last_days:0"})).is_err());
    assert!(view::parse_date_value(&serde_json::json!({"rel":"last_days:7"})).is_ok());
    // 混合端点不放宽 between：within + after 组合才合法
    let ok = serde_json::json!({
        "dataset": { "item_type": "all", "filter": { "op": "and", "children": [
            { "field": "col:due_at", "cmp": "after", "value": { "day": "2026-09-01" } },
            { "field": "col:due_at", "cmp": "within", "value": { "rel": "this_week" } }
        ] } }
    });
    assert!(myday_core::view::validate_view_config(&ok, &fields).is_ok());
}

#[test]
fn within_and_between_hit_expected_days() {
    let (_t, store) = matrix_setup();
    let defs = store.list_field_defs(None).unwrap();
    let num = defs.iter().find(|d| d.kind == FieldKind::Number && d.scope.is_none()).unwrap().id.clone();
    // due = 明天的待办：within this_week（若明天在本周）与 between 今天~明天
    store
        .add_item(NewItem {
            title: Some("A".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(1, 10, 0)),
            extra: serde_json::json!({ num.clone(): 5 }),
            ..Default::default()
        })
        .unwrap();
    let cfg = user_view_config(and(vec![cond("col:due_at", Cmp::Between, Some(FilterValue::DatePair(view::DatePair {
        from: view::DateValue::RelPoint(view::RelDay::Today),
        to: view::DateValue::RelPoint(view::RelDay::Tomorrow),
    })))]));
    let v = store.create_view("btw", Panel::Tasks, &cfg).unwrap();
    assert_eq!(store.query_view(&v.id, None, None).unwrap().items.unwrap().len(), 1);
    let _ = store.delete_view(&v.id);

    // number between
    let cfg = user_view_config(and(vec![cond(&num, Cmp::Between, Some(FilterValue::NumPair(view::NumPair { from: 1.0, to: 9.0 })))]));
    let v = store.create_view("num-btw", Panel::Tasks, &cfg).unwrap();
    assert_eq!(store.query_view(&v.id, None, None).unwrap().items.unwrap().len(), 1);
    let _ = store.delete_view(&v.id);
}

// ----------------------------------------------------------------------
// 预筛下推：OR 并集不漏分支（§10.2 硬规则）
// ----------------------------------------------------------------------

#[test]
fn pushdown_or_union_keeps_both_branches() {
    let t = TempDir::new();
    let store = t.store();
    // 分支 1：due before 明天（预筛走 due 列窗）
    let a = store
        .add_item(NewItem {
            title: Some("今天到期".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 10, 0)),
            ..Default::default()
        })
        .unwrap();
    // 分支 2：status eq done（due 不在未来窗口内；若按交集下推会漏掉）
    let b = store
        .add_item(NewItem {
            title: Some("已完成早截止".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(-30, 10, 0)),
            status: Some(ItemStatus::Done),
            ..Default::default()
        })
        .unwrap();
    // 都不命中的对照
    store
        .add_item(NewItem {
            title: Some("未来".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(30, 10, 0)),
            ..Default::default()
        })
        .unwrap();

    let filter = or(vec![
        cond("col:due_at", Cmp::Before, Some(FilterValue::Date(view::DateValue::RelPoint(view::RelDay::Tomorrow)))),
        cond("col:status", Cmp::Eq, Some(FilterValue::Text("done".into()))),
    ]);
    let cfg = user_view_config(filter);
    let v = store.create_view("or-test", Panel::Tasks, &cfg).unwrap();
    let r = store.query_view(&v.id, None, None).unwrap().items.unwrap();
    let ids: Vec<&str> = r.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids.len(), 2, "两个 OR 分支都必须命中: {ids:?}");
    assert!(ids.contains(&a.id.as_str()) && ids.contains(&b.id.as_str()));
}

// ----------------------------------------------------------------------
// view_defs CRUD 与定制语义（§7）
// ----------------------------------------------------------------------

#[test]
fn view_crud_customize_reset_delete() {
    let t = TempDir::new();
    let store = t.store();

    // 种子齐全：六内置面板视图 + 三个预置统计容器（普通行，非 builtin）
    let views = store.list_views(None).unwrap();
    assert!(views.iter().any(|v| v.id == VIEW_TASKS_TODAY));
    assert_eq!(views.len(), 9, "六面板视图 + 三统计容器");
    assert_eq!(views.iter().filter(|v| v.builtin).count(), 6, "仅面板视图是内置");

    // 内置编辑写 config_user；生效配置随之变化；customized 标志翻转
    let today = store.get_view(VIEW_TASKS_TODAY).unwrap();
    assert!(!today.customized());
    let mut cfg = today.effective_config().clone();
    cfg["dataset"]["filter"]["children"].as_array_mut().unwrap().push(
        serde_json::json!({ "field": "col:anchor", "cmp": "empty" }),
    );
    store.save_view(VIEW_TASKS_TODAY, None, &cfg).unwrap();
    let customized = store.get_view(VIEW_TASKS_TODAY).unwrap();
    assert!(customized.customized());
    assert_eq!(customized.effective_config(), &cfg);

    // 重置 = 清空 config_user，恢复 seed
    let reset = store.reset_view(VIEW_TASKS_TODAY).unwrap();
    assert!(!reset.customized());
    assert_eq!(reset.effective_config(), &today.config);

    // 内置不可删
    assert!(store.delete_view(VIEW_TASKS_TODAY).is_err());

    // 用户视图：新建 → 查询 → 删除
    let v = store
        .create_view("高优先级未完成", Panel::Tasks, &user_view_config(and(vec![
            cond("col:status", Cmp::Eq, Some(FilterValue::Text("todo".into()))),
        ])))
        .unwrap();
    assert!(v.id.starts_with("view_") && !v.builtin);
    store.query_view(&v.id, None, None).unwrap();
    store.delete_view(&v.id).unwrap();
    assert!(store.get_view(&v.id).is_err());

    // 另存为 = 从生效配置复制出用户视图
    let dup = store.duplicate_view(VIEW_TASKS_UPCOMING, "我的即将").unwrap();
    assert!(!dup.builtin);
    assert_eq!(dup.effective_config(), &store.get_view(VIEW_TASKS_UPCOMING).unwrap().config);
}

#[test]
fn save_rejects_invalid_configs() {
    let t = TempDir::new();
    let store = t.store();
    let fields = store.list_field_defs(None).unwrap();

    // 根不是组
    let bad_root = serde_json::json!({ "dataset": { "item_type": "all", "filter": { "field": "col:status", "cmp": "eq", "value": "todo" } } });
    assert!(myday_core::view::validate_view_config(&bad_root, &fields).is_err());
    // 三层嵌套
    let deep = serde_json::json!({ "dataset": { "item_type": "all", "filter": { "op": "and", "children": [
        { "op": "or", "children": [ { "op": "and", "children": [] } ] }
    ] } } });
    assert!(myday_core::view::validate_view_config(&deep, &fields).is_err());
    // status 只支持 eq
    let bad_cmp = serde_json::json!({ "dataset": { "item_type": "task", "filter": { "op": "and", "children": [
        { "field": "col:status", "cmp": "neq", "value": "done" }
    ] } } });
    assert!(myday_core::view::validate_view_config(&bad_cmp, &fields).is_err());
    // recurrence 只支持 empty/not_empty
    let bad_rec = serde_json::json!({ "dataset": { "item_type": "task", "filter": { "op": "and", "children": [
        { "field": "col:recurrence", "cmp": "eq", "value": "@daily" }
    ] } } });
    assert!(myday_core::view::validate_view_config(&bad_rec, &fields).is_err());
    // 未知内置列
    let bad_col = serde_json::json!({ "dataset": { "item_type": "all", "filter": { "op": "and", "children": [
        { "field": "col:id", "cmp": "eq", "value": "x" }
    ] } } });
    assert!(myday_core::view::validate_view_config(&bad_col, &fields).is_err());

    // 挂件：values 仅 log 且禁 group；dataset 禁 limit
    let bad_values = serde_json::json!({ "kind": "widget", "dataset": { "item_type": "task", "filter": { "op": "and", "children": [] } },
        "agg": { "group": null, "metric": { "fn": "values", "field": "fd_water_cup" } }, "render": "line", "options": {} });
    assert!(myday_core::view::validate_widget_config(&bad_values, &fields).is_err());
    let bad_limit = serde_json::json!({ "kind": "widget", "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] }, "limit": 10 },
        "render": "bar", "options": {} });
    assert!(myday_core::view::validate_widget_config(&bad_limit, &fields).is_err());
    // window.days 必须 ≥ 1
    let bad_window = serde_json::json!({ "kind": "widget", "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] } },
        "window": { "days": 0 }, "render": "bar", "options": {} });
    assert!(myday_core::view::validate_widget_config(&bad_window, &fields).is_err());
}

// ----------------------------------------------------------------------
// 挂件三段管线（§8）
// ----------------------------------------------------------------------

#[test]
fn widget_streak_and_heatmap_and_bar_same_source() {
    let t = TempDir::new();
    let store = t.store();
    // 用种子模板 tpl_water 记录：今昨连续 2 天 + 3 天前断档
    //（now-2h 与 now-26h 恒为相邻两日，current=2；now-74h 恒再隔一日截断）
    for hours_ago in [2i64, 26, 74] {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some("喝水".into()),
                template_id: Some("tpl_water".into()),
                occurred_at: Some(recent_log_at(hours_ago)),
                ..Default::default()
            })
            .unwrap();
    }
    let ctx = EvalCtx::now();
    let fields = store.list_field_defs(None).unwrap();
    let templates = store
        .list_templates()
        .unwrap()
        .into_iter()
        .map(|t| view::TplInfo { id: t.id, name: t.name, icon: t.icon })
        .collect::<Vec<_>>();
    let pool = store.list_items_unbounded(&ListFilter::default()).unwrap();

    let cfg = serde_json::from_value::<myday_core::view::WidgetConfig>(serde_json::json!({
        "kind": "widget",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [
            { "field": "col:template_id", "cmp": "eq", "value": "tpl_water" }
        ] } },
        "window": { "all": true },
        "agg": { "group": { "by": "time", "bucket": "day", "time_field": "col:occurred_at" }, "metric": { "fn": "count" } },
        "derived": { "kind": "streak", "goal": { "daily": 1 } },
        "render": "card",
        "options": { "presence": false, "top_n": 8 }
    }))
    .unwrap();
    let r = myday_core::view::eval_widget(
        "test",
        "tpl_water",
        &cfg,
        &pool,
        &ctx,
        None,
        &fields,
        &templates,
    )
    .unwrap();
    let streak = r.derived.streak.expect("streak 派生");
    assert_eq!(streak.current, 2, "今天+昨天连续（-2 断档截断）");
    assert_eq!(streak.longest, 2, "最长 = 今天起往前 2 天");
    assert_eq!(streak.recent, 3, "window=all → recent = 全部 3 条");
    // day 桶：全历史窗口从最早记录补零到今天
    let buckets = r.buckets.unwrap();
    assert_eq!(buckets.last().unwrap().key, ctx.today.format("%Y-%m-%d").to_string());
    assert!(buckets.iter().any(|b| b.value == 0.0), "断档日补零");

    // 同数据即时换渲染器：bar / heatmap 不需要改数据配置（§8 抽象）
    let mut bar_cfg = cfg.clone();
    bar_cfg.render = myday_core::view::Render::Bar;
    let r = myday_core::view::eval_widget("test", "k", &bar_cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    assert!(r.error.is_none());

    // 渲染器不兼容标注：时间桶上画饼图
    let mut pie_cfg = cfg.clone();
    pie_cfg.render = myday_core::view::Render::Pie;
    let r = myday_core::view::eval_widget("test", "k", &pie_cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    assert!(r.error.is_some(), "饼图需要字段分组");
}

#[test]
fn widget_field_grouping_counts_each_value_and_templates_get_labels() {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("晨跑".into()),
            template_id: Some("tpl_water".into()),
            tags: vec!["运动".into(), "户外".into()],
            occurred_at: Some(past_at(0, 7, 0)),
            ..Default::default()
        })
        .unwrap();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("喝水".into()),
            template_id: Some("tpl_water".into()),
            occurred_at: Some(past_at(0, 8, 0)),
            ..Default::default()
        })
        .unwrap();
    let ctx = EvalCtx::now();
    let fields = store.list_field_defs(None).unwrap();
    let templates = store
        .list_templates()
        .unwrap()
        .into_iter()
        .map(|t| view::TplInfo { id: t.id, name: t.name, icon: t.icon })
        .collect::<Vec<_>>();
    let pool = store.list_items_unbounded(&ListFilter::default()).unwrap();

    // col:tags 字段分组：一条带两标签 → 两个桶各计一次
    let cfg = serde_json::from_value::<myday_core::view::WidgetConfig>(serde_json::json!({
        "kind": "widget",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] } },
        "agg": { "group": { "by": "field", "field": "col:tags" }, "metric": { "fn": "count" } },
        "render": "pie",
        "options": { "top_n": 8 }
    }))
    .unwrap();
    let r = myday_core::view::eval_widget("test", "k", &cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    let bs = r.buckets.unwrap();
    assert_eq!(bs.len(), 2);
    assert_eq!(bs[0].value, 1.0, "按 value 降序");

    // col:template_id 分组带 labels
    let cfg = serde_json::from_value::<myday_core::view::WidgetConfig>(serde_json::json!({
        "kind": "widget",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] } },
        "agg": { "group": { "by": "field", "field": "col:template_id" }, "metric": { "fn": "count" } },
        "render": "pie",
        "options": { "top_n": 8 }
    }))
    .unwrap();
    let r = myday_core::view::eval_widget("test", "k", &cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    assert_eq!(r.labels.get("tpl_water").map(String::as_str), Some("喝水"));
}

#[test]
fn widget_values_points_and_month_avg() {
    let t = TempDir::new();
    let store = t.store();
    // 锚点日两条（恒同月）+ 35 天前一条（跨月差恒大于 31 天，恒另一月）
    for (off, h, v) in [(35i64, 8u32, 70.0), (0, 8, 71.5), (0, 9, 72.0)] {
        store
            .add_item(NewItem {
                item_type: Some(ItemType::Log),
                title: Some("体重".into()),
                occurred_at: Some(past_at(off, h, 0)),
                extra: serde_json::json!({ "fd_weight_kg": v }),
                ..Default::default()
            })
            .unwrap();
    }
    let ctx = EvalCtx::now();
    let fields = store.list_field_defs(None).unwrap();
    let templates: Vec<view::TplInfo> = Vec::new();
    let pool = store.list_items_unbounded(&ListFilter::default()).unwrap();

    // values 点列（升序）+ unit 缺省取字段定义
    let cfg = serde_json::from_value::<myday_core::view::WidgetConfig>(serde_json::json!({
        "kind": "widget",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [
            { "field": "fd_weight_kg", "cmp": "not_empty" }
        ] } },
        "agg": { "group": null, "metric": { "fn": "values", "field": "fd_weight_kg" } },
        "render": "line",
        "options": {}
    }))
    .unwrap();
    let r = myday_core::view::eval_widget("test", "k", &cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    let pts = r.points.unwrap();
    assert_eq!(pts.len(), 3);
    assert!(pts[0].t < pts[2].t);
    assert_eq!(r.unit.as_deref(), Some("kg"));

    // 月桶 avg + bar：单月单值 → 该月 avg = 值
    let cfg = serde_json::from_value::<myday_core::view::WidgetConfig>(serde_json::json!({
        "kind": "widget",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] } },
        "window": { "days": 90 },
        "agg": { "group": { "by": "time", "bucket": "month", "time_field": "col:occurred_at" }, "metric": { "fn": "avg", "field": "fd_weight_kg" } },
        "render": "bar",
        "options": {}
    }))
    .unwrap();
    let r = myday_core::view::eval_widget("test", "k", &cfg, &pool, &ctx, None, &fields, &templates).unwrap();
    let bs = r.buckets.unwrap();
    let nonzero: Vec<&myday_core::view::Bucket> = bs.iter().filter(|b| b.value > 0.0).collect();
    assert_eq!(nonzero.len(), 2, "两个月各有数据: {bs:?}");
}

#[test]
fn stats_preset_containers_are_ordinary() {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("喝水".into()),
            template_id: Some("tpl_water".into()),
            occurred_at: Some(recent_log_at(2)),
            ..Default::default()
        })
        .unwrap();

    // 预置容器 = 开库铺好的普通容器（无 builtin / 隐藏 / 动态概念）
    let page = store.query_stats_page().unwrap();
    assert_eq!(page.containers.len(), 3);
    let streaks = page.containers.iter().find(|c| c.view_id == VIEW_STATS_STREAKS).unwrap();
    assert_eq!(streaks.layout, "horizontal");
    // 预置挂件按当前数据物化：种子两个 pinned log 模板各一张卡，全部显式
    let cards: Vec<_> = page.widgets.iter().filter(|w| w.view_id == VIEW_STATS_STREAKS).collect();
    assert_eq!(cards.len(), 2);
    assert!(cards.iter().all(|w| w.render == myday_core::view::Render::Card));
    let row = store.get_view(VIEW_STATS_STREAKS).unwrap();
    assert!(!row.builtin, "预置容器与用户容器同权");
    let cards_in_config = row.config.get("widgets").unwrap().as_array().unwrap().len();
    assert_eq!(cards_in_config, 2);

    // 编辑 = 普通整配置替换；× 移除卡 = 从 widgets 里删除（永久，可恢复默认找回）
    let mut cfg = row.config.clone();
    cfg["widgets"].as_array_mut().unwrap().remove(0);
    store.save_view(VIEW_STATS_STREAKS, None, &cfg).unwrap();
    let page = store.query_stats_page().unwrap();
    assert_eq!(page.widgets.iter().filter(|w| w.view_id == VIEW_STATS_STREAKS).count(), 1);

    // 删除预置容器 = 物理删除（与用户容器同权）
    store.delete_view(VIEW_STATS_STREAKS).unwrap();
    assert!(store.get_view(VIEW_STATS_STREAKS).is_err());

    // 恢复默认统计页 = 完全重置：清掉全部容器（含用户改过的），铺回三预设
    let created = store.restore_stats_defaults().unwrap();
    assert_eq!(created.len(), 3, "三个预设全部重铺");
    let page = store.query_stats_page().unwrap();
    let cards: Vec<_> = page.widgets.iter().filter(|w| w.view_id == VIEW_STATS_STREAKS).collect();
    assert_eq!(cards.len(), 2, "按当前 pinned 模板重新铺卡");
    assert_eq!(page.containers.len(), 3);

    // 用户容器同一条路径：＋容器 → ＋挂件
    let cv = store
        .create_view("我的面板", Panel::Stats, &serde_json::json!({
            "kind": "container",
            "layout": "horizontal",
            "widgets": [
                { "kind": "widget", "title": "跑步连续", "dataset": { "item_type": "log", "filter": { "op": "and", "children": [
                    { "field": "col:tags", "cmp": "any", "value": ["跑步"] } ] } },
                    "agg": { "group": { "by": "time", "bucket": "day", "time_field": "col:occurred_at" }, "metric": { "fn": "count" } },
                    "derived": { "kind": "streak", "goal": { "daily": 1 } },
                    "render": "card",
                    "options": {}
                }
            ]
        }))
        .unwrap();
    let page = store.query_stats_page().unwrap();
    assert!(page.widgets.iter().any(|w| w.view_id == cv.id && w.render == myday_core::view::Render::Card));
    assert!(store.delete_view(&cv.id).is_ok());
}

#[test]
fn stats_summary_independent_of_view_rows() {
    // CLI 契约：固定按预设定义（代码常量）求值——统计页行全删也不受影响
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("喝水".into()),
            template_id: Some("tpl_water".into()),
            occurred_at: Some(recent_log_at(2)),
            ..Default::default()
        })
        .unwrap();
    for row in store.list_views(Some(Panel::Stats)).unwrap() {
        store.delete_view(&row.id).unwrap();
    }
    let s = store.stats_summary(30).unwrap();
    assert_eq!(s.heatmap.len(), 30);
    assert_eq!(s.streaks.len(), 2, "pinned log 模板全部出卡（预设定义）");
    assert!(s.streaks.iter().any(|x| x.template_id == "tpl_water" && x.current == 1));
}

#[test]
fn stats_summary_envelope_stays_compatible() {
    let t = TempDir::new();
    let store = t.store();
    store
        .add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some("喝水".into()),
            template_id: Some("tpl_water".into()),
            occurred_at: Some(recent_log_at(2)),
            ..Default::default()
        })
        .unwrap();
    let s = store.stats_summary(365).unwrap();
    assert_eq!(s.heatmap.len(), 365);
    assert_eq!(s.streaks.len(), 2, "pinned log 模板全部出卡（固定 seed，忽略 hidden）");
    let water = s.streaks.iter().find(|x| x.template_id == "tpl_water").unwrap();
    assert_eq!(water.current, 1);
    assert_eq!(water.recent, 1);
    // 页面定制不影响 CLI 契约（预设定义在代码里，不读 view_defs 的统计行）
    store.delete_view(VIEW_STATS_STREAKS).unwrap();
    let s2 = store.stats_summary(30).unwrap();
    assert_eq!(s2.streaks.len(), 2);
    assert_eq!(s2.heatmap.len(), 30);
}

// ----------------------------------------------------------------------
// 存储层不变量（FILTER-SPEC §3 / v1.6 ②）
// ----------------------------------------------------------------------

#[test]
fn invariant_recurring_task_requires_due() {
    let t = TempDir::new();
    let store = t.store();
    // add：重复待办必须带截止
    assert!(store
        .add_item(NewItem {
            title: Some("无截止重复".into()),
            item_type: Some(ItemType::Task),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .is_err());
    let task = store
        .add_item(NewItem {
            title: Some("重复待办".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 18, 0)),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .unwrap();
    // update：清掉截止（保留规则）= 破坏不变量
    let patch = ItemPatch { clear_due_at: true, ..Default::default() };
    assert!(store.update_item(&task.id, patch).is_err());
    // 同一补丁清规则 + 清截止 = 合法（不再重复）
    let patch = ItemPatch { clear_due_at: true, clear_recurrence: true, ..Default::default() };
    store.update_item(&task.id, patch).unwrap();
}

#[test]
fn invariant_uncomplete_via_update_item_rolls_back() {
    let t = TempDir::new();
    let store = t.store();
    let task = store
        .add_item(NewItem {
            title: Some("每天喝水".into()),
            item_type: Some(ItemType::Task),
            due_at: Some(at(0, 20, 0)),
            recurrence: Some("@daily".into()),
            ..Default::default()
        })
        .unwrap();
    // 完成 = 推进到下一期（当前期 +1 天），记账
    let advanced = store.complete_task(&task.id).unwrap();
    assert_eq!(advanced.status, Some(ItemStatus::Todo));
    let advanced_due = advanced.due_at.unwrap();
    assert_eq!(advanced_due, at(1, 20, 0));
    // 用户又把它标记成 done（update 直改），随后取消完成 —— 取消旁路
    store
        .update_item(&task.id, ItemPatch { status: Some(ItemStatus::Done), ..Default::default() })
        .unwrap();
    // 经 update_item 直改 todo = 统一回拨（v1.6 ②：对齐 uncomplete_task）
    let rolled = store
        .update_item(&task.id, ItemPatch { status: Some(ItemStatus::Todo), ..Default::default() })
        .unwrap();
    assert_eq!(rolled.due_at, Some(at(0, 20, 0)), "回拨到最近一次推进前的存储值");
    let item = store.get_item(&task.id).unwrap();
    assert!(
        item.extra.get(myday_core::store::RECURRED_DONE_KEY).is_none(),
        "记账键消费即清"
    );
}

// ----------------------------------------------------------------------
// 结果信封（§10.6）
// ----------------------------------------------------------------------

#[test]
fn result_envelope_carries_evaluated_at_and_tz() {
    let t = TempDir::new();
    let store = t.store();
    let r = store.query_view(VIEW_TASKS_TODAY, None, None).unwrap();
    assert!(chrono::DateTime::parse_from_rfc3339(&r.evaluated_at).is_ok());
    assert!(!r.tz.is_empty());
    assert_eq!(r.view_id, VIEW_TASKS_TODAY);
    assert_eq!(r.panel, myday_core::view::Panel::Tasks);
    // 生效配置回显
    assert!(r.config.get("dataset").is_some());
    let _ = myday_core::view::Window::Days { days: 7 };
}

//! 今日悬浮窗求值（OVERLAY-SPEC §4）。
//!
//! 「今日未完成」的唯一语义实现：本地日语义、逾期边界、无 due 排除、
//! 重复日程按窗口展开（复用 [`crate::recurrence::occurrences_between`]）。
//! 求值独立于 Tauri 可单测；命令层只做序列化。
//!
//! 注：条目为硬删除（items 无软删列），无需排除 deleted_at；
//! 软删仅存在于字段定义（field_defs.deleted_at），与本求值无关。

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::error::Result;
use crate::model::{display_title, Item, ItemStatus, ItemType};
use crate::recurrence::occurrences_between;
use crate::store::{ListFilter, Store};

/// 单条悬浮窗条目（只读展示所需的最小面）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverlayEntry {
    pub id: String,
    pub kind: ItemType,
    pub title: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub due_at: Option<DateTime<Utc>>,
    /// 全天日程（不显示时刻）
    pub all_day: bool,
    /// 全天到期待办（不显示时刻）
    pub due_all_day: bool,
    /// 日程进行中（求值时刻 ∈ [start, end)），供左条高亮
    pub in_progress: bool,
    /// 同刻稳定排序锚（OVERLAY-SPEC §4.2），前端可忽略
    pub created_at: DateTime<Utc>,
}

/// `overlay_today` 的输出信封（OVERLAY-SPEC §7）。
#[derive(Debug, Clone, Serialize)]
pub struct TodayOverlay {
    /// 本地日（YYYY-MM-DD）
    pub date: NaiveDate,
    /// 今日日程（start ∈ [本地 00:00, 次日 00:00)，start 升序）
    pub events: Vec<OverlayEntry>,
    /// 今日待办（task + todo + due 同窗，due 升序）
    pub tasks: Vec<OverlayEntry>,
    /// 今日已完成（done 且 due 在今日）
    pub done_count: i64,
    /// 逾期 = todo 且 due < 今日 00:00（只计数不成列）
    pub overdue_count: i64,
    /// 未安排 = todo 且无 due（只计数不成列）
    pub unscheduled_count: i64,
}

/// `day` 的本地 00:00 → UTC（DST 缺失时刻回退为标准时区零点）。
fn local_midnight_utc(day: NaiveDate) -> DateTime<Utc> {
    chrono::Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0).expect("00:00 恒合法"))
        .earliest()
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|| Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap()))
}

fn entry(item: &Item, kind: ItemType, start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>, now: DateTime<Utc>) -> OverlayEntry {
    OverlayEntry {
        id: item.id.clone(),
        kind,
        title: display_title(item),
        start_at: start,
        end_at: end,
        due_at: item.due_at,
        all_day: item.all_day,
        due_all_day: item.due_all_day,
        in_progress: matches!((start, end), (Some(s), Some(e)) if now >= s && now < e),
        created_at: item.created_at,
    }
}

/// 求值 `day`（本地日）的今日悬浮窗数据（OVERLAY-SPEC §4.1–§4.3）。
pub fn today_overlay(store: &Store, day: NaiveDate) -> Result<TodayOverlay> {
    let day0 = local_midnight_utc(day);
    let day1 = local_midnight_utc(day.succ_opt().expect("NaiveDate 无尽头"));
    let now = Utc::now();

    // 一次全量扫描后内存分桶：与 view.rs 求值路径同款取数方式，
    // 重复日程展开需要窗口外的系列锚点条目，按窗口预筛反而取不全。
    let all = store.list_items_unbounded(&ListFilter::default())?;

    let mut events: Vec<OverlayEntry> = Vec::new();
    let mut tasks: Vec<OverlayEntry> = Vec::new();
    let mut done_count = 0i64;
    let mut overdue_count = 0i64;
    let mut unscheduled_count = 0i64;

    for it in &all {
        match it.item_type {
            ItemType::Log => {}
            ItemType::Event => {
                if it.recurrence.is_some() {
                    // 重复日程：展开落在当日的发生（归属按开始，跨天两头不算）
                    for occ in occurrences_between(it, day0, day1) {
                        let start = occ.start.unwrap_or(day0);
                        if start < day0 || start >= day1 {
                            continue;
                        }
                        events.push(entry(it, ItemType::Event, Some(start), occ.end, now));
                    }
                } else if let Some(s) = it.start_at {
                    if s >= day0 && s < day1 {
                        events.push(entry(it, ItemType::Event, Some(s), it.end_at, now));
                    }
                }
            }
            ItemType::Task => {
                let open = it.status == Some(ItemStatus::Todo);
                match it.due_at {
                    // 重复待办的 due_at 即当前期（完成推进），窗口语义与普通待办一致
                    Some(d) if d >= day0 && d < day1 => {
                        if open {
                            tasks.push(entry(it, ItemType::Task, it.start_at, None, now));
                        } else if it.status == Some(ItemStatus::Done) {
                            done_count += 1;
                        }
                    }
                    Some(d) if open && d < day0 => overdue_count += 1,
                    None if open => unscheduled_count += 1,
                    _ => {}
                }
            }
        }
    }

    // §4.2：时间升序，同刻按 created_at 稳定排序
    events.sort_by(|a, b| a.start_at.cmp(&b.start_at).then(a.created_at.cmp(&b.created_at)));
    tasks.sort_by(|a, b| a.due_at.cmp(&b.due_at).then(a.created_at.cmp(&b.created_at)));

    Ok(TodayOverlay {
        date: day,
        events,
        tasks,
        done_count,
        overdue_count,
        unscheduled_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NewItem;
    use chrono::{Duration, TimeZone};

    struct TempDir(tempfile::TempDir);
    impl TempDir {
        fn store(&self) -> Store {
            Store::open(&self.0.path().join("myday.db"), self.0.path()).expect("open store")
        }
    }

    /// 本地时区构造 UTC 时刻（不依赖机器时区）
    fn local(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        chrono::Local
            .with_ymd_and_hms(y, m, d, h, mi, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    fn add(store: &Store, new: NewItem) -> Item {
        store.add_item(new).expect("add_item")
    }

    /// 普通日程（同日 start → end）
    fn event(store: &Store, title: &str, day: (i32, u32, u32), h: u32, mi: u32, end_h: u32) -> Item {
        let (y, m, d) = day;
        add(
            store,
            NewItem {
                item_type: Some(ItemType::Event),
                title: Some(title.into()),
                start_at: Some(local(y, m, d, h, mi)),
                end_at: Some(local(y, m, d, end_h, mi)),
                ..Default::default()
            },
        )
    }

    fn task_due(store: &Store, title: &str, due: Option<DateTime<Utc>>, status: Option<ItemStatus>) -> Item {
        add(
            store,
            NewItem {
                item_type: Some(ItemType::Task),
                title: Some(title.into()),
                due_at: due,
                due_all_day: false,
                status,
                ..Default::default()
            },
        )
    }

    fn eval(store: &Store, day: (i32, u32, u32)) -> TodayOverlay {
        let (y, m, d) = day;
        today_overlay(store, NaiveDate::from_ymd_opt(y, m, d).unwrap()).expect("today_overlay")
    }

    #[test]
    fn local_day_boundaries_and_done_count() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        // 今日 00:00 起点与 23:59 终点都算今日；昨日 / 明日不算
        event(&store, "零点日程", (2026, 9, 17), 0, 0, 1);
        // 跨午夜日程（23:59 → 次日 00:10）：归属按开始日，只算 9/17 一天
        add(
            &store,
            NewItem {
                item_type: Some(ItemType::Event),
                title: Some("跨午夜日程".into()),
                start_at: Some(local(2026, 9, 17, 23, 59)),
                end_at: Some(local(2026, 9, 18, 0, 10)),
                ..Default::default()
            },
        );
        event(&store, "昨日日程", (2026, 9, 16), 12, 0, 13);
        event(&store, "明日日程", (2026, 9, 18), 12, 0, 13);

        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.events.len(), 2, "{:?}", out.events);
        assert!(out.events.windows(2).all(|w| w[0].start_at <= w[1].start_at), "start 升序");
        assert_eq!(out.events[0].title, "零点日程");
        // 9/18 视角：跨午夜日程不重复计入次日
        let out18 = eval(&store, (2026, 9, 18));
        assert_eq!(out18.events.len(), 1);
        assert_eq!(out18.events[0].title, "明日日程");

        // done 计数：今日到期已完成 +1；昨日到期已完成不计
        task_due(&store, "今日完成", Some(local(2026, 9, 17, 9, 0)), Some(ItemStatus::Done));
        task_due(&store, "昨日完成", Some(local(2026, 9, 16, 9, 0)), Some(ItemStatus::Done));
        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.done_count, 1);
    }

    #[test]
    fn overdue_boundary_at_midnight_and_unscheduled() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        // 昨日 23:59:59 与今日 00:00 只差一秒：一条逾期、一条今日
        let today0 = local(2026, 9, 17, 0, 0);
        task_due(&store, "卡点今日", Some(today0), None);
        task_due(&store, "差一秒逾期", Some(today0 - Duration::seconds(1)), None);
        task_due(&store, "无截止", None, None);

        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.tasks.len(), 1);
        assert_eq!(out.tasks[0].title, "卡点今日");
        assert_eq!(out.overdue_count, 1, "due < 今日00:00 即逾期");
        assert_eq!(out.unscheduled_count, 1, "无 due 计入未安排");
        assert_eq!(out.done_count, 0);
    }

    #[test]
    fn explicit_day_snapshot_across_midnight() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        event(&store, "周四日程", (2026, 9, 17), 10, 0, 11);
        task_due(&store, "周四待办", Some(local(2026, 9, 17, 18, 0)), None);

        // 同一份数据，对不同本地日求值互不串窗
        let thu = eval(&store, (2026, 9, 17));
        let fri = eval(&store, (2026, 9, 18));
        assert_eq!(thu.events.len(), 1);
        assert_eq!(thu.tasks.len(), 1);
        assert_eq!(fri.events.len(), 0);
        assert_eq!(fri.tasks.len(), 0);
        // 周五视角：周四待办即逾期
        assert_eq!(fri.overdue_count, 1);
    }

    #[test]
    fn recurring_event_expands_into_day() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        // 系列锚点在 9/10，@daily 应在 9/17 当日展开一次；9/17 恰是锚点也应只有一条
        add(
            &store,
            NewItem {
                item_type: Some(ItemType::Event),
                title: Some("每日站会".into()),
                start_at: Some(local(2026, 9, 10, 9, 0)),
                end_at: Some(local(2026, 9, 10, 9, 15)),
                recurrence: Some("@daily".into()),
                ..Default::default()
            },
        );
        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.events.len(), 1, "{:?}", out.events);
        assert_eq!(out.events[0].start_at, Some(local(2026, 9, 17, 9, 0)));
        let out = eval(&store, (2026, 9, 18));
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.events[0].start_at, Some(local(2026, 9, 18, 9, 0)));
    }

    #[test]
    fn recurring_overdue_task_counts_overdue_not_today() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        // 每周三待办，锚点 9/16（周三）未完成：due 停在错过的那期 → 逾期而非今日
        add(
            &store,
            NewItem {
                item_type: Some(ItemType::Task),
                title: Some("周报".into()),
                due_at: Some(local(2026, 9, 16, 18, 0)),
                status: Some(ItemStatus::Todo),
                recurrence: Some("@weekly:3".into()),
                ..Default::default()
            },
        );
        let out = eval(&store, (2026, 9, 17));
        assert!(out.tasks.is_empty());
        assert_eq!(out.overdue_count, 1);
    }

    #[test]
    fn ties_break_by_created_at_stable() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        let a = task_due(&store, "先建", Some(local(2026, 9, 17, 9, 0)), None);
        let b = task_due(&store, "后建", Some(local(2026, 9, 17, 9, 0)), None);
        // 两条同刻；把 a 的 created_at 抬早一天，断言同刻按 created_at 升序
        {
            let conn = store.raw_conn().unwrap();
            conn.execute(
                "UPDATE items SET created_at = ?1 WHERE id = ?2",
                rusqlite::params![dt(local(2026, 9, 16, 9, 0)), a.id],
            )
            .unwrap();
        }
        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.tasks.len(), 2);
        assert_eq!(out.tasks[0].id, a.id);
        assert_eq!(out.tasks[1].id, b.id);
    }

    #[test]
    fn all_day_task_flags_and_log_ignored() {
        let t = TempDir(tempfile::tempdir().unwrap());
        let store = t.store();
        add(
            &store,
            NewItem {
                item_type: Some(ItemType::Task),
                title: Some("交房租".into()),
                due_at: Some(local(2026, 9, 17, 0, 0)),
                due_all_day: true,
                ..Default::default()
            },
        );
        add(
            &store,
            NewItem {
                item_type: Some(ItemType::Log),
                title: Some("喝水".into()),
                occurred_at: Some(local(2026, 9, 17, 8, 0)),
                ..Default::default()
            },
        );
        let out = eval(&store, (2026, 9, 17));
        assert_eq!(out.tasks.len(), 1);
        assert!(out.tasks[0].due_all_day, "全天待办带标记，前端不显时刻");
        assert!(out.events.is_empty(), "log 不进悬浮窗");
    }

    fn dt(t: DateTime<Utc>) -> String {
        t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }
}

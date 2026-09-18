//! 提醒调度（INTERACTION §6）。
//!
//! 提醒存「意图」（spec）而非时刻，发生时刻运行时展开：
//! - `@start±off` / `@due±off`：锚定条目开始/截止，**条目改时间自动跟随**；
//! - `@dailyT<clock>`：按条目期间逐日展开（event [start,end]、
//!   task [start ?? created_at, due]、log 无期间）；
//! - 绝对 RFC3339（不带 @）：一次性。
//!
//! 去重靠 `reminder_log(reminder_id, 解析时刻)` 复合键：同一 spec 因时间变动
//! 产生新时刻自然重新触发，历史时刻不重发。
//!
//! 核心库不直接发系统通知，而是通过 [`Notifier`] 抽象：
//! GUI 注入 notify-rust 实现；测试注入收集器。
//! 调用方（GUI 常驻线程）每 30 秒执行一次 [`tick_once`]。

use chrono::{DateTime, Duration, Utc};
use crate::error::Result;
use crate::model::{display_title, Item, ItemStatus, ItemType, Reminder};
use crate::store::Store;
use crate::tpltime::{self, Spec};

/// 通知发送抽象。只传**结构化数据**（条目 + 发生时刻 + 动作 ID），文案由
/// 实现方按界面语言渲染（core 保持 locale 中立）。GNOME 通知 action 受限时，
/// 实现方自行降级为打开定位。
pub trait Notifier: Send + Sync {
    /// `at` = spec 展开的发生时刻；`actions` 为动作 ID（complete/snooze/open）。
    fn notify(&self, reminder: &Reminder, item: &Item, at: DateTime<Utc>, actions: &[&'static str]);

    /// 补发窗口之外的错过提醒聚合摘要（SPRINT-SPEC §1.1）：一条通知代替 N 条轰炸。
    /// `lines` = (标题, 原定时刻文本)。默认空实现（测试收集器通常只关心逐条 notify）。
    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let _ = (count, lines);
    }
}

/// 空通知器（CLI / 测试用，仅打日志）。
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn notify(&self, _reminder: &Reminder, item: &Item, _at: DateTime<Utc>, _actions: &[&'static str]) {
        eprintln!("myday reminder: {} ({})", display_title(item), item.id);
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        eprintln!("myday reminder: 错过 {count} 条提醒（{lines:?}）");
    }
}

/// 一条待发提醒：spec 展开出的发生时刻 + 所属条目。
#[derive(Debug, Clone)]
pub struct DueOccurrence {
    pub reminder: Reminder,
    pub at: DateTime<Utc>,
    pub item: Item,
}

/// 提醒 spec → 发生时刻序列（升序、去重）。
/// spec 非法或锚点缺失返回空（宁可不响，不猜测）。
pub fn occurrences(spec: &str, item: &Item) -> Vec<DateTime<Utc>> {
    if !tpltime::is_token(spec) {
        // 绝对时刻：一次性事实
        return crate::store::parse_dt(spec).into_iter().collect();
    }
    let Ok(parsed) = tpltime::parse_spec(spec, "reminder") else {
        return vec![];
    };
    let mut out = match parsed {
        Spec::Start(off) => item.start_at.map(|s| s + off).into_iter().collect(),
        Spec::Due(off) => item
            .due_at
            .map(|d| d + off.unwrap_or(Duration::zero()))
            .into_iter()
            .collect(),
        Spec::Daily(clock) => daily_occurrences(item, clock),
        // now 族禁止作提醒 spec（随评估漂移）；d 基准是模板锚点日语义，提醒不展开
        _ => vec![],
    };
    out.sort();
    out.dedup();
    out
}

/// 期间逐日本地时刻（上限 400 个防御异常区间）。
fn daily_occurrences(item: &Item, clock: (u32, u32)) -> Vec<DateTime<Utc>> {
    let period: Option<(DateTime<Utc>, DateTime<Utc>)> = match item.item_type {
        ItemType::Event => item.start_at.and_then(|s| item.end_at.map(|e| (s, e))),
        ItemType::Task => item
            .due_at
            .map(|d| (item.start_at.unwrap_or(item.created_at), d)),
        ItemType::Log => None,
    };
    let Some((from, to)) = period else {
        return vec![];
    };
    let mut day = from.with_timezone(&chrono::Local).date_naive();
    let end_day = to.with_timezone(&chrono::Local).date_naive();
    let mut out = Vec::new();
    while day <= end_day && out.len() < 400 {
        if let Some(n) = day.and_hms_opt(clock.0, clock.1, 0) {
            out.push(tpltime::local_to_utc(n));
        }
        day += Duration::days(1);
    }
    out
}

/// 补发窗口（设置键 `reminder_catchup_minutes`，默认 120，范围 0–1440）：
/// 窗口内错过的提醒照常补发；窗口外的一律静默记日志并聚合成一条摘要，
/// 防止休眠 / 重启后对历史时刻通知轰炸（SPRINT-SPEC §1.1）。
fn catchup_window(store: &Store) -> Duration {
    store
        .get_setting("reminder_catchup_minutes")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .map(|m| Duration::minutes(m.clamp(0, 1440)))
        .unwrap_or_else(|| Duration::hours(2))
}

/// 执行一轮提醒检查，返回本轮补发的提醒数（错过聚合不计入）。
pub fn tick_once(store: &Store, notifier: &dyn Notifier) -> Result<usize> {
    let now = Utc::now();
    let window = catchup_window(store);
    let due = store.due_reminder_occurrences(now)?;
    let mut sent = 0;
    let mut missed: Vec<DueOccurrence> = Vec::new();
    for occ in due {
        if now - occ.at > window {
            // 窗口外错过：静默入 reminder_log，稍后一条摘要带过
            store.mark_reminded(occ.reminder.id, occ.at)?;
            missed.push(occ);
            continue;
        }
        if occ.item.item_type == ItemType::Task && occ.item.status == Some(ItemStatus::Done) {
            // 已完成的待办不再提醒，直接标记
            store.mark_reminded(occ.reminder.id, occ.at)?;
            continue;
        }
        let actions: &[&'static str] = match occ.item.item_type {
            ItemType::Task => &["complete", "snooze"],
            ItemType::Event | ItemType::Log => &["open", "snooze"],
        };
        notifier.notify(&occ.reminder, &occ.item, occ.at, actions);
        store.mark_reminded(occ.reminder.id, occ.at)?;
        sent += 1;
    }
    if !missed.is_empty() {
        let lines: Vec<(String, String)> = missed
            .iter()
            .take(3)
            .map(|o| {
                (
                    display_title(&o.item),
                    o.at
                        .with_timezone(&chrono::Local)
                        .format("%m-%d %H:%M")
                        .to_string(),
                )
            })
            .collect();
        notifier.notify_missed(missed.len(), &lines);
    }
    Ok(sent)
}

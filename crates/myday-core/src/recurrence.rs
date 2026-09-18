//! 重复规则（SPRINT-SPEC §2）。
//!
//! spec 文法（独立命名空间，不复用 tpltime 的时刻 token）：
//! - `@daily`       每天（同一本地时刻）
//! - `@weekly:<n>`  每周，n = 1..7（1 = 周一 … 7 = 周日）
//! - `@monthly:<d>` 每月，d = 1..31（当月天数不足取月末，如 31 → 2 月 28/29）
//!
//! 「修改全部」语义：条目只存一条规则（`items.recurrence`），不物化实例；
//! 编辑锚点时间 = 平移整个系列；渲染按窗口展开（TS 镜像 `lib/recurrence.ts`
//! 与 golden 用例两侧对齐）。重复待办「完成 = 推进到下一期」见
//! [`crate::store::Store::complete_task`]（`extra.recurred_done_at` 记账供回拨）。

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};

use crate::error::{MyDayError, Result};
use crate::model::{Item, ItemType};

/// 单次发生（该类型用到的字段有值，其余为 None）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Occurrence {
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub due: Option<DateTime<Utc>>,
}

/// 单条目单窗口展开上限（防御异常区间，与提醒环同款）。
const MAX_OCCURRENCES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recurrence {
    Daily,
    Weekly(u32),
    Monthly(u32),
}

impl Recurrence {
    /// 严格解析（宁报错不猜测）：空值 / 未知 token / 越界参数一律 [INVALID]。
    pub fn parse(s: &str) -> Result<Self> {
        let invalid = || {
            MyDayError::Invalid(format!(
                "未知重复规则: {s}（支持 @daily / @weekly:1-7 / @monthly:1-31）"
            ))
        };
        if s == "@daily" {
            return Ok(Self::Daily);
        }
        if let Some(n) = s.strip_prefix("@weekly:") {
            let n: u32 = n.parse().map_err(|_| invalid())?;
            if (1..=7).contains(&n) {
                return Ok(Self::Weekly(n));
            }
            return Err(invalid());
        }
        if let Some(d) = s.strip_prefix("@monthly:") {
            let d: u32 = d.parse().map_err(|_| invalid())?;
            if (1..=31).contains(&d) {
                return Ok(Self::Monthly(d));
            }
            return Err(invalid());
        }
        Err(invalid())
    }

    pub fn as_str(self) -> String {
        match self {
            Self::Daily => "@daily".into(),
            Self::Weekly(n) => format!("@weekly:{n}"),
            Self::Monthly(d) => format!("@monthly:{d}"),
        }
    }

    /// 严格晚于 `after` 的下一次发生时刻（与 anchor 同一本地钟点）。
    /// DST 缺失时刻当天顺延到下一期；扫描均有界，异常情形返回 None。
    pub fn next_after(self, anchor: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local = anchor.with_timezone(&chrono::Local);
        let (h, mi, se) = (local.hour(), local.minute(), local.second());
        let base = local.date_naive();
        let at = |d: chrono::NaiveDate| -> Option<DateTime<Utc>> {
            d.and_hms_opt(h, mi, se).map(crate::tpltime::local_to_utc)
        };
        match self {
            Self::Daily => {
                let after_date = after.with_timezone(&chrono::Local).date_naive();
                let min_k = (after_date - base).num_days() - 1;
                let mut k = min_k.max(0);
                while k <= min_k + (MAX_OCCURRENCES as i64) + 2 {
                    if let Some(t) = at(base + Duration::days(k)) {
                        if t > after {
                            return Some(t);
                        }
                    }
                    k += 1;
                }
                None
            }
            Self::Weekly(w) => {
                let base_wd = base.weekday().number_from_monday() as i64;
                let mut d = base + Duration::days((w as i64 - base_wd).rem_euclid(7));
                for _ in 0..520 {
                    if let Some(t) = at(d) {
                        if t > after {
                            return Some(t);
                        }
                    }
                    d += Duration::weeks(1);
                }
                None
            }
            Self::Monthly(day) => {
                let (y0, m0) = (base.year(), base.month() as i64);
                for j in 0..1200 {
                    let total = m0 - 1 + j;
                    let cy = y0 + total.div_euclid(12) as i32;
                    let cm = total.rem_euclid(12) as u32 + 1;
                    let cd = day.min(days_in_month(cy, cm));
                    if let Some(t) = at(chrono::NaiveDate::from_ymd_opt(cy, cm, cd)?) {
                        if t > after {
                            return Some(t);
                        }
                    }
                }
                None
            }
        }
    }
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    (chrono::NaiveDate::from_ymd_opt(ny, nm, 1)
        .expect("首日必然合法")
        - Duration::days(1))
    .day()
}

/// 展开条目在窗口内的发生：日程按区间相交（跨天两头都算），待办按 due 落窗。
/// 无规则 / 规则非法 / 锚点缺失 → 空（宁可不渲染，不猜测）。
pub fn occurrences_between(item: &Item, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
    let Some(spec) = item.recurrence.as_deref() else {
        return Vec::new();
    };
    let Ok(rec) = Recurrence::parse(spec) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match item.item_type {
        ItemType::Event => {
            let Some(anchor) = item.start_at else {
                return out;
            };
            let end = item.end_at.unwrap_or(anchor);
            let dur = end - anchor;
            let mut t = anchor;
            loop {
                if t > to {
                    break;
                }
                let e = t + dur;
                if e >= from {
                    out.push(Occurrence { start: Some(t), end: Some(e), due: None });
                    if out.len() >= MAX_OCCURRENCES {
                        break;
                    }
                }
                let Some(n) = rec.next_after(anchor, t) else {
                    break;
                };
                t = n;
            }
        }
        ItemType::Task => {
            let Some(due) = item.due_at else {
                return out;
            };
            let shift = item.start_at.map(|s| due - s);
            let mut t = due;
            loop {
                if t > to {
                    break;
                }
                if t >= from {
                    out.push(Occurrence {
                        start: shift.map(|d| t - d),
                        end: None,
                        due: Some(t),
                    });
                    if out.len() >= MAX_OCCURRENCES {
                        break;
                    }
                }
                let Some(n) = rec.next_after(due, t) else {
                    break;
                };
                t = n;
            }
        }
        ItemType::Log => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        // 用本地时区构造再转 UTC，保证与 next_after 的本地钟点语义一致
        chrono::Local
            .with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn parse_is_strict() {
        assert_eq!(Recurrence::parse("@daily").unwrap(), Recurrence::Daily);
        assert_eq!(Recurrence::parse("@weekly:1").unwrap(), Recurrence::Weekly(1));
        assert_eq!(Recurrence::parse("@weekly:7").unwrap(), Recurrence::Weekly(7));
        assert_eq!(Recurrence::parse("@monthly:31").unwrap(), Recurrence::Monthly(31));
        for bad in ["", "@", "@week", "@weekly:0", "@weekly:8", "@monthly:0", "@monthly:32", "@daily:", "@now", "@d+7"] {
            assert!(Recurrence::parse(bad).is_err(), "{bad} 应拒绝");
        }
    }

    #[test]
    fn daily_weekly_monthly_next() {
        let anchor = utc(2026, 9, 16, 9); // 周三 09:00 本地
        // 每天：严格晚于当天 09:00 的下一次 = 次日
        let next = Recurrence::Daily.next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 17, 9));
        // 每周一：从周三起到下周一
        let next = Recurrence::Weekly(1).next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 21, 9));
        // 每周三是自身锚点：下一个是下周三
        let next = Recurrence::Weekly(3).next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 23, 9));
        // 每月 31 号：9 月之后是 10 月 31，然后 11 月 30（月末钳制），12 月 31
        let jan31 = utc(2026, 1, 31, 20);
        let feb = Recurrence::Monthly(31).next_after(jan31, jan31).unwrap();
        assert_eq!(feb, utc(2026, 2, 28, 20));
        let mar = Recurrence::Monthly(31).next_after(jan31, feb).unwrap();
        assert_eq!(mar, utc(2026, 3, 31, 20));
        let _ = anchor;
    }

    #[test]
    fn monthly_clamps_to_month_end() {
        let jan31 = utc(2026, 1, 31, 8);
        let mut t = jan31;
        let months = [
            (2026, 2, 28),
            (2026, 3, 31),
            (2026, 4, 30),
            (2026, 5, 31),
            (2026, 6, 30),
        ];
        for (y, m, d) in months {
            t = Recurrence::Monthly(31).next_after(jan31, t).unwrap();
            let local = t.with_timezone(&chrono::Local);
            assert_eq!((local.year(), local.month(), local.day()), (y, m, d));
        }
    }

    #[test]
    fn occurrences_window_for_event_and_task() {
        // 构造条目直接复用（不进库）；时间用本地构造转 UTC，不依赖机器时区
        let mut item = serde_json::from_str::<Item>(
            r#"{"id":"evt_x","type":"event","title":"站会","start_at":null,"end_at":null,"all_day":false,"due_at":null,"due_all_day":false,"occurred_at":null,"status":null,"completed_at":null,"template_id":null,"reminders":[],"tags":[],"attachments":[],"idempotency_key":null,"created_at":"2026-09-16T00:00:00Z","updated_at":"2026-09-16T00:00:00Z","extra":{}}"#,
        )
        .unwrap();
        let anchor = utc(2026, 9, 16, 9); // 周三本地 09:00
        item.recurrence = Some("@daily".into());
        item.start_at = Some(anchor);
        item.end_at = Some(anchor + Duration::minutes(15));
        // 窗口 = 9/17 当天（本地），站会每天一次
        let from = utc(2026, 9, 17, 0);
        let to = utc(2026, 9, 17, 23);
        let occ = occurrences_between(&item, from, to);
        assert_eq!(occ.len(), 1, "窗口内恰好一次：{:?}", occ);
        assert_eq!(occ[0].end.unwrap() - occ[0].start.unwrap(), Duration::minutes(15));

        // 待办：每周三截止，9/17 窗口为空、9/23（周三）窗口恰好一次
        item.item_type = ItemType::Task;
        item.start_at = None;
        item.end_at = None;
        item.due_at = Some(anchor);
        item.recurrence = Some("@weekly:3".into());
        let occ = occurrences_between(&item, from, to);
        assert!(occ.is_empty(), "9/17 不是周三：{:?}", occ);
        let occ = occurrences_between(&item, utc(2026, 9, 23, 0), utc(2026, 9, 23, 23));
        assert_eq!(occ.len(), 1);
        assert_eq!(occ[0].due, Some(utc(2026, 9, 23, 9)));
        assert_eq!(occ[0].start, None, "无开始的待办 occurrence 不带 start");
    }
}

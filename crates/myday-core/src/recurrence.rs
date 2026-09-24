//! 重复规则（SPRINT-SPEC §2）。
//!
//! spec 文法（独立命名空间，不复用 tpltime 的时刻 token）：
//! - `@daily`       每天（同一本地时刻）
//! - `@weekly:<n>`  每周，n = 1..7（1 = 周一 … 7 = 周日）
//! - `@monthly:<d>` 每月，d = 1..31（当月天数不足取月末，如 31 → 2 月 28/29）
//!
//! 结束条件（可选，`;` 追加一段，二选一互斥）：
//! - `;until=YYYY-MM-DD` 含当天的本地最后日期（该日的发生仍是最后一期）
//! - `;count=N`          含锚点在内的总次数上限
//! 例：`@weekly:3;until=2026-12-31`、`@daily;count=10`。
//!
//! 单次例外：`items.recurrence_exdates`（JSON 数组，RFC3339 时刻）列出被剔除的
//! 发生锚点——展开时跳过该期；「拆为单次 / 仅删除这一期」见
//! [`crate::store::Store::detach_occurrence`] / [`crate::store::Store::skip_occurrence`]。
//!
//! 「修改全部」语义：条目只存一条规则（`items.recurrence`），不物化实例；
//! 编辑锚点时间 = 平移整个系列；渲染按窗口展开（TS 镜像 `lib/recurrence.ts`
//! 与 golden 用例两侧对齐）。重复待办「完成 = 推进到下一期」见
//! [`crate::store::Store::complete_task`]（`extra.recurred_done_at` 记账供回拨）。

use chrono::{DateTime, Datelike, Duration, NaiveDate, Timelike, Utc};

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

/// 重复基础规则（不含结束条件）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecurKind {
    Daily,
    Weekly(u32),
    Monthly(u32),
}

/// 重复规则 = 基础规则 + 可选结束条件（until 与 count 互斥，parse 拒绝双写）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recurrence {
    pub kind: RecurKind,
    /// 含当天的本地最后日期
    pub until: Option<NaiveDate>,
    /// 含锚点在内的总次数上限
    pub count: Option<u32>,
}

impl Recurrence {
    /// 仅基础规则（无结束条件）。
    pub fn bare(kind: RecurKind) -> Self {
        Self { kind, until: None, count: None }
    }

    /// 严格解析（宁报错不猜测）：空值 / 未知 token / 越界参数 / until+count 双写
    /// 一律 [INVALID]。
    pub fn parse(s: &str) -> Result<Self> {
        let invalid = || {
            MyDayError::Invalid(format!(
                "未知重复规则: {s}（支持 @daily / @weekly:1-7 / @monthly:1-31，\
                 结束条件 ;until=YYYY-MM-DD 或 ;count=N，二者不可同用）"
            ))
        };
        let mut segs = s.split(';');
        let base = segs.next().unwrap_or("");
        let kind = if base == "@daily" {
            RecurKind::Daily
        } else if let Some(n) = base.strip_prefix("@weekly:") {
            let n: u32 = n.parse().map_err(|_| invalid())?;
            if !(1..=7).contains(&n) {
                return Err(invalid());
            }
            RecurKind::Weekly(n)
        } else if let Some(d) = base.strip_prefix("@monthly:") {
            let d: u32 = d.parse().map_err(|_| invalid())?;
            if !(1..=31).contains(&d) {
                return Err(invalid());
            }
            RecurKind::Monthly(d)
        } else {
            return Err(invalid());
        };
        let mut until = None;
        let mut count = None;
        for seg in segs {
            if let Some(v) = seg.strip_prefix("until=") {
                if until.is_some() || count.is_some() {
                    return Err(invalid());
                }
                until = Some(NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| invalid())?);
            } else if let Some(v) = seg.strip_prefix("count=") {
                if until.is_some() || count.is_some() {
                    return Err(invalid());
                }
                let n: u32 = v.parse().map_err(|_| invalid())?;
                if n == 0 {
                    return Err(invalid());
                }
                count = Some(n);
            } else {
                return Err(invalid());
            }
        }
        Ok(Self { kind, until, count })
    }

    /// 规范化字符串（round-trip；字段次序固定：kind;until;count）。
    pub fn as_str(self) -> String {
        let mut out = match self.kind {
            RecurKind::Daily => "@daily".to_string(),
            RecurKind::Weekly(n) => format!("@weekly:{n}"),
            RecurKind::Monthly(d) => format!("@monthly:{d}"),
        };
        if let Some(u) = self.until {
            out.push_str(&format!(";until={u}"));
        }
        if let Some(c) = self.count {
            out.push_str(&format!(";count={c}"));
        }
        out
    }

    /// 严格晚于 `after` 的下一次发生时刻（与 anchor 同一本地钟点）。
    /// 结束条件生效：until 之后 / 达到 count 次数上限 → None。
    /// DST 缺失时刻当天顺延到下一期（该期不计入 count）；扫描均有界。
    pub fn next_after(self, anchor: DateTime<Utc>, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let local = anchor.with_timezone(&chrono::Local);
        let (h, mi, se) = (local.hour(), local.minute(), local.second());
        let base = local.date_naive();
        let after_local = after.with_timezone(&chrono::Local).date_naive();
        let at = |d: NaiveDate| -> Option<DateTime<Utc>> {
            d.and_hms_opt(h, mi, se).map(crate::tpltime::local_to_utc)
        };
        // k = 距 anchor 的期数（0 = anchor 本期），供 count 上限判断；
        // 起点略早于 after 的保守估计，循环内再精判
        let (k0, date_of): (i64, Box<dyn Fn(i64) -> Option<NaiveDate>>) = match self.kind {
            RecurKind::Daily => (
                (after_local - base).num_days() - 1,
                Box::new(move |k: i64| Some(base + Duration::days(k))),
            ),
            RecurKind::Weekly(w) => {
                let base_wd = base.weekday().number_from_monday() as i64;
                let aligned = base + Duration::days((w as i64 - base_wd).rem_euclid(7));
                let span = (after_local - aligned).num_days().max(0);
                ((span / 7) - 1, Box::new(move |k: i64| Some(aligned + Duration::days(7 * k))))
            }
            RecurKind::Monthly(day) => {
                let month_idx = |d: NaiveDate| d.year() as i64 * 12 + d.month() as i64 - 1;
                let base_m = month_idx(base);
                let k0 = (month_idx(after_local) - base_m - 1).max(-1);
                (
                    k0,
                    Box::new(move |k: i64| {
                        let total = base_m + k;
                        let cy = (total.div_euclid(12)) as i32;
                        let cm = total.rem_euclid(12) as u32 + 1;
                        NaiveDate::from_ymd_opt(cy, cm, day.min(days_in_month(cy, cm)))
                    }),
                )
            }
        };
        let hard_max = self
            .count
            .map(|c| c as i64)
            .unwrap_or(MAX_OCCURRENCES as i64 + 2);
        let mut k = k0.max(0);
        while k < hard_max {
            let Some(d) = date_of(k) else {
                k += 1;
                continue;
            };
            if let Some(u) = self.until {
                if d > u {
                    return None; // 单调递增：越过 until 即终止
                }
            }
            if let Some(t) = at(d) {
                if t > after {
                    return Some(t);
                }
            }
            k += 1;
        }
        None
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
/// 单次例外（`recurrence_exdates` 中的锚点期）被跳过。
/// 无规则 / 规则非法 / 锚点缺失 → 空（宁可不渲染，不猜测）。
pub fn occurrences_between(item: &Item, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
    let Some(spec) = item.recurrence.as_deref() else {
        return Vec::new();
    };
    let Ok(rec) = Recurrence::parse(spec) else {
        return Vec::new();
    };
    let exdates: std::collections::HashSet<i64> = item
        .recurrence_exdates
        .iter()
        .map(|t| t.timestamp())
        .collect();
    let is_excluded = |t: DateTime<Utc>| exdates.contains(&t.timestamp());
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
                if e >= from && !is_excluded(t) {
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
                if t >= from && !is_excluded(t) {
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
        assert_eq!(Recurrence::parse("@daily").unwrap().kind, RecurKind::Daily);
        assert_eq!(Recurrence::parse("@weekly:1").unwrap().kind, RecurKind::Weekly(1));
        assert_eq!(Recurrence::parse("@weekly:7").unwrap().kind, RecurKind::Weekly(7));
        assert_eq!(Recurrence::parse("@monthly:31").unwrap().kind, RecurKind::Monthly(31));
        assert!(Recurrence::parse("@daily").unwrap().until.is_none());
        for bad in ["", "@", "@week", "@weekly:0", "@weekly:8", "@monthly:0", "@monthly:32", "@daily:", "@now", "@d+7"] {
            assert!(Recurrence::parse(bad).is_err(), "{bad} 应拒绝");
        }
    }

    #[test]
    fn parse_end_conditions_and_roundtrip() {
        use chrono::TimeZone;
        let u = Recurrence::parse("@daily;until=2026-12-31").unwrap();
        assert_eq!(u.until, Some(chrono::NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()));
        assert_eq!(u.as_str(), "@daily;until=2026-12-31");
        let c = Recurrence::parse("@weekly:3;count=12").unwrap();
        assert_eq!(c.count, Some(12));
        assert_eq!(c.as_str(), "@weekly:3;count=12");
        for bad in [
            "@daily;until=2026-13-01",   // 非法日期
            "@daily;until=2026-12-31;count=3", // 双写
            "@daily;count=0",            // 零次
            "@daily;count=abc",          // 非数字
            "@daily;freq=x",             // 未知段
            "@daily;until",              // 残缺
        ] {
            assert!(Recurrence::parse(bad).is_err(), "{bad} 应拒绝");
        }
    }

    #[test]
    fn until_stops_expansion() {
        // 每天，至 2026-09-20（含）：20 日仍有下一期，21 日没有
        let rec = Recurrence::parse("@daily;until=2026-09-20").unwrap();
        let a = utc(2026, 9, 18, 9);
        assert_eq!(rec.next_after(a, a).unwrap(), utc(2026, 9, 19, 9));
        let last = utc(2026, 9, 20, 9);
        assert_eq!(rec.next_after(a, last), None, "越过 until 无下一期");
    }

    #[test]
    fn count_limits_occurrence_index() {
        // 每天 × 3 次：锚点 + 2 天内两期，第 3 期之后没有
        let rec = Recurrence::parse("@daily;count=3").unwrap();
        let a = utc(2026, 9, 16, 9);
        assert_eq!(rec.next_after(a, a).unwrap(), utc(2026, 9, 17, 9));
        let second = utc(2026, 9, 17, 9);
        assert_eq!(rec.next_after(a, second).unwrap(), utc(2026, 9, 18, 9));
        assert_eq!(rec.next_after(a, utc(2026, 9, 18, 9)), None, "3 次已用尽");
        // 从很晚的 after 反推：k0 直接越过上限，同样 None
        assert_eq!(rec.next_after(a, utc(2027, 9, 18, 9)), None);
    }

    #[test]
    fn exdate_skips_occurrence() {
        use chrono::TimeZone;
        let mut item = serde_json::from_str::<Item>(
            r#"{"id":"evt_x","type":"event","title":"站会","start_at":null,"end_at":null,"all_day":false,"due_at":null,"due_all_day":false,"occurred_at":null,"status":null,"completed_at":null,"recurrence":"@daily","recurrence_exdates":[],"template_id":null,"reminders":[],"tags":[],"attachments":[],"idempotency_key":null,"created_at":"2026-09-16T00:00:00Z","updated_at":"2026-09-16T00:00:00Z","extra":{}}"#,
        )
        .unwrap();
        let a = utc(2026, 9, 16, 9);
        item.start_at = Some(a);
        item.end_at = Some(a + Duration::minutes(15));
        // 剔除 9/17 这一期：窗口内 9/17 不再出现，9/18 照常
        item.recurrence_exdates = vec![utc(2026, 9, 17, 9)];
        let occ = occurrences_between(&item, utc(2026, 9, 17, 0), utc(2026, 9, 17, 23));
        assert!(occ.is_empty(), "被剔除的期不应展开：{:?}", occ);
        let occ = occurrences_between(&item, utc(2026, 9, 18, 0), utc(2026, 9, 18, 23));
        assert_eq!(occ.len(), 1);
    }

    #[test]
    fn daily_weekly_monthly_next() {
        let anchor = utc(2026, 9, 16, 9); // 周三 09:00 本地
        // 每天：严格晚于当天 09:00 的下一次 = 次日
        let next = Recurrence::bare(RecurKind::Daily).next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 17, 9));
        // 每周一：从周三起到下周一
        let next = Recurrence::bare(RecurKind::Weekly(1)).next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 21, 9));
        // 每周三是自身锚点：下一个是下周三
        let next = Recurrence::bare(RecurKind::Weekly(3)).next_after(anchor, anchor).unwrap();
        assert_eq!(next, utc(2026, 9, 23, 9));
        // 每月 31 号：9 月之后是 10 月 31，然后 11 月 30（月末钳制），12 月 31
        let jan31 = utc(2026, 1, 31, 20);
        let feb = Recurrence::bare(RecurKind::Monthly(31)).next_after(jan31, jan31).unwrap();
        assert_eq!(feb, utc(2026, 2, 28, 20));
        let mar = Recurrence::bare(RecurKind::Monthly(31)).next_after(jan31, feb).unwrap();
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
            t = Recurrence::bare(RecurKind::Monthly(31)).next_after(jan31, t).unwrap();
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


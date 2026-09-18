//! ICS 导出（需求 P1.3 / SPRINT2-SPEC §6）。
//!
//! - 日程 → VEVENT（DTSTART/DTEND UTC；all_day 用 VALUE=DATE；重复规则映射 RRULE；
//!   提醒映射 VALARM：`@start±` → 相对 DTSTART、绝对时刻 → VALUE=DATE-TIME）。
//! - 带截止的待办 → VTODO（DUE；`@due±` 提醒 → RELATED=END 的 VALARM）。
//! - 记录不导出；`@dailyT<clock>` 期间提醒无 ICS 对应物，跳过。
//! - DESCRIPTION 末尾附 `myday://item/<id>` 回链；CRLF + 75 字节折行（RFC 5545）。

use chrono::{DateTime, Duration, Utc};

use crate::error::Result;
use crate::model::{Item, ItemType};
use crate::store::{parse_dt, ListFilter, Store};
use crate::tpltime::{self, Spec};

/// 导出全部日程与带截止的待办为 ICS 文本。
pub fn export_ics(store: &Store) -> Result<String> {
    let items = store.list_items(&ListFilter {
        order: crate::store::ListOrder::Asc,
        ..Default::default()
    })?;
    let now = Utc::now();
    let mut lines: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//MyDay//Schedule//CN".into(),
        "CALSCALE:GREGORIAN".into(),
        "METHOD:PUBLISH".into(),
    ];
    for item in items {
        match item.item_type {
            ItemType::Event => event_block(&item, now, &mut lines),
            ItemType::Task if item.due_at.is_some() => todo_block(&item, now, &mut lines),
            _ => {}
        }
    }
    lines.push("END:VCALENDAR".into());
    Ok(lines.iter().map(|l| fold_line(l)).collect::<Vec<_>>().join("\r\n") + "\r\n")
}

fn event_block(item: &Item, now: DateTime<Utc>, out: &mut Vec<String>) {
    let Some(start) = item.start_at else { return };
    let end = item.end_at.unwrap_or(start + Duration::hours(1));
    out.push("BEGIN:VEVENT".into());
    out.push(format!("UID:{}@myday", item.id));
    out.push(format!("DTSTAMP:{}", fmt_utc(now)));
    if item.all_day {
        let sd = start.with_timezone(&chrono::Local).date_naive();
        let ed = end.with_timezone(&chrono::Local).date_naive() + Duration::days(1);
        out.push(format!("DTSTART;VALUE=DATE:{}", sd.format("%Y%m%d")));
        out.push(format!("DTEND;VALUE=DATE:{}", ed.format("%Y%m%d")));
    } else {
        out.push(format!("DTSTART:{}", fmt_utc(start)));
        out.push(format!("DTEND:{}", fmt_utc(end)));
    }
    push_common(item, out);
    if let Some(spec) = item.recurrence.as_deref() {
        if let Some(rrule) = rrule_of(spec) {
            out.push(format!("RRULE:{rrule}"));
        }
    }
    for r in &item.reminders {
        if let Some(prop) = valarm_prop(&r.spec, false) {
            out.push("BEGIN:VALARM".into());
            out.push("ACTION:DISPLAY".into());
            out.push(format!("DESCRIPTION:{}", escape(&crate::model::display_title(item))));
            out.push(prop);
            out.push("END:VALARM".into());
        }
    }
    out.push("END:VEVENT".into());
}

fn todo_block(item: &Item, now: DateTime<Utc>, out: &mut Vec<String>) {
    let Some(due) = item.due_at else { return };
    out.push("BEGIN:VTODO".into());
    out.push(format!("UID:{}@myday", item.id));
    out.push(format!("DTSTAMP:{}", fmt_utc(now)));
    if item.due_all_day {
        out.push(format!("DUE;VALUE=DATE:{}", due.with_timezone(&chrono::Local).date_naive().format("%Y%m%d")));
    } else {
        out.push(format!("DUE:{}", fmt_utc(due)));
    }
    out.push(format!(
        "STATUS:{}",
        if item.status == Some(crate::model::ItemStatus::Done) { "COMPLETED" } else { "NEEDS-ACTION" }
    ));
    if let Some(done) = item.completed_at {
        out.push(format!("COMPLETED:{}", fmt_utc(done)));
    }
    push_common(item, out);
    if let Some(spec) = item.recurrence.as_deref() {
        if let Some(rrule) = rrule_of(spec) {
            out.push(format!("RRULE:{rrule}"));
        }
    }
    for r in &item.reminders {
        if let Some(prop) = valarm_prop(&r.spec, true) {
            out.push("BEGIN:VALARM".into());
            out.push("ACTION:DISPLAY".into());
            out.push(format!("DESCRIPTION:{}", escape(&crate::model::display_title(item))));
            out.push(prop);
            out.push("END:VALARM".into());
        }
    }
    out.push("END:VTODO".into());
}

/// SUMMARY / DESCRIPTION / CATEGORIES / URL 公共行。
fn push_common(item: &Item, out: &mut Vec<String>) {
    out.push(format!("SUMMARY:{}", escape(&crate::model::display_title(item))));
    let mut desc = item.note.clone().unwrap_or_default();
    desc.push_str(&format!("\nmyday://item/{}", item.id));
    out.push(format!("DESCRIPTION:{}", escape(&desc)));
    if !item.tags.is_empty() {
        out.push(format!("CATEGORIES:{}", item.tags.iter().map(|t| escape(t)).collect::<Vec<_>>().join(",")));
    }
}

/// 重复规则 → RRULE（weekly 的 n 为 1..=7 周一始）。
fn rrule_of(spec: &str) -> Option<String> {
    use crate::recurrence::Recurrence;
    match Recurrence::parse(spec).ok()? {
        Recurrence::Daily => Some("FREQ=DAILY".into()),
        Recurrence::Weekly(n) => {
            const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
            Some(format!("FREQ=WEEKLY;BYDAY={}", DAYS[(n - 1) as usize]))
        }
        Recurrence::Monthly(d) => Some(format!("FREQ=MONTHLY;BYMONTHDAY={d}")),
    }
}

/// 提醒 spec → VALARM 的 TRIGGER 行。task（due 基准）用 RELATED=END。
fn valarm_prop(spec: &str, is_task: bool) -> Option<String> {
    if !tpltime::is_token(spec) {
        let at = parse_dt(spec)?;
        return Some(format!("TRIGGER;VALUE=DATE-TIME:{}", fmt_utc(at)));
    }
    let parsed = tpltime::parse_spec(spec, "reminder").ok()?;
    match parsed {
        Spec::Start(off) => Some(format!("TRIGGER:{}", ics_duration(off))),
        Spec::Due(off) => {
            let d = off.unwrap_or(Duration::zero());
            let related = if is_task { ";RELATED=END" } else { "" };
            Some(format!("TRIGGER{related}:{}", ics_duration(d)))
        }
        // 期间每日提醒无 ICS 对应物；其余（now/d 族）不是合法提醒 spec
        _ => None,
    }
}

/// Duration → ICS 时长（负号表示提前）。
fn ics_duration(d: Duration) -> String {
    let neg = if d < Duration::zero() { "-" } else { "" };
    let abs = d.abs();
    let days = abs.num_days();
    let hours = abs.num_hours() % 24;
    let mins = abs.num_minutes() % 60;
    let mut out = format!("{neg}P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if hours > 0 || mins > 0 {
        out.push('T');
        if hours > 0 {
            out.push_str(&format!("{hours}H"));
        }
        if mins > 0 {
            out.push_str(&format!("{mins}M"));
        }
    }
    if days == 0 && hours == 0 && mins == 0 {
        out.push_str("T0M");
    }
    out
}

fn fmt_utc(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// RFC 5545 TEXT 转义。
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}

/// 75 字节折行（UTF-8 安全：按字符累积字节长度）。
fn fold_line(line: &str) -> String {
    const LIMIT: usize = 75;
    let mut out = String::new();
    let mut cur = 0usize;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if cur + len > LIMIT {
            out.push_str("\r\n ");
            cur = 1; // 续行前导空格计入
        }
        out.push(ch);
        cur += len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_format() {
        assert_eq!(ics_duration(Duration::minutes(-10)), "-PT10M");
        assert_eq!(ics_duration(Duration::minutes(-60)), "-PT1H");
        assert_eq!(ics_duration(Duration::days(-1)), "-P1D");
        assert_eq!(ics_duration(Duration::minutes(90)), "PT1H30M");
        assert_eq!(ics_duration(Duration::zero()), "PT0M");
    }

    #[test]
    fn escape_and_fold() {
        assert_eq!(escape("a;b,c\nd\\"), "a\\;b\\,c\\nd\\\\");
        let long = "SUMMARY:".to_string() + &"很长的标题".repeat(30);
        let folded = fold_line(&long);
        for seg in folded.split("\r\n ") {
            assert!(seg.len() <= 75);
        }
    }

    #[test]
    fn rrule_mapping() {
        assert_eq!(rrule_of("@daily").as_deref(), Some("FREQ=DAILY"));
        assert_eq!(rrule_of("@weekly:3").as_deref(), Some("FREQ=WEEKLY;BYDAY=WE"));
        assert_eq!(rrule_of("@weekly:7").as_deref(), Some("FREQ=WEEKLY;BYDAY=SU"));
        assert_eq!(rrule_of("@monthly:15").as_deref(), Some("FREQ=MONTHLY;BYMONTHDAY=15"));
        assert_eq!(rrule_of("bogus"), None);
    }
}

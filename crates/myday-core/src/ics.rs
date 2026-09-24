//! ICS 导出（需求 P1.3 / SPRINT2-SPEC §6）。
//!
//! - 日程 → VEVENT（DTSTART/DTEND UTC；all_day 用 VALUE=DATE；重复规则映射 RRULE；
//!   提醒映射 VALARM：`@start±` → 相对 DTSTART、绝对时刻 → VALUE=DATE-TIME）。
//! - 带截止的待办 → VTODO（DUE；`@due±` 提醒 → RELATED=END 的 VALARM）。
//! - 记录不导出；`@dailyT<clock>` 期间提醒无 ICS 对应物，跳过。
//! - DESCRIPTION 末尾附 `myday://item/<id>` 回链；CRLF + 75 字节折行（RFC 5545）。
//!
//! 导入（`import_ics`）：VEVENT → 日程、VTODO → 待办（含无截止待办 = 未排期池）。
//! 尽力而为：无法映射的部分（结束条件 / 多日 BYDAY / YEARLY 等）降级为告警，
//! 单个组件失败不中断整体；重复导入靠 myday 回链与 `ics-<UID>` 幂等键双重防护。

use chrono::{DateTime, Datelike, Duration, Utc};

use crate::error::Result;
use crate::model::{Item, ItemStatus, ItemType, NewItem, NewReminder};
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
    Ok(lines
        .iter()
        .map(|l| fold_line(l))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n")
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
        if let Some(rrule) = rrule_of(spec, item.all_day) {
            out.push(format!("RRULE:{rrule}"));
        }
    }
    if !item.recurrence_exdates.is_empty() {
        // 单次例外 → EXDATE（UTC 时刻，逗号分隔；RFC 5545）
        let list = item
            .recurrence_exdates
            .iter()
            .map(|t| fmt_utc(*t))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("EXDATE:{list}"));
    }
    for r in &item.reminders {
        if let Some(prop) = valarm_prop(&r.spec, false) {
            out.push("BEGIN:VALARM".into());
            out.push("ACTION:DISPLAY".into());
            out.push(format!(
                "DESCRIPTION:{}",
                escape(&crate::model::display_title(item))
            ));
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
        out.push(format!(
            "DUE;VALUE=DATE:{}",
            due.with_timezone(&chrono::Local)
                .date_naive()
                .format("%Y%m%d")
        ));
    } else {
        out.push(format!("DUE:{}", fmt_utc(due)));
    }
    out.push(format!(
        "STATUS:{}",
        if item.status == Some(crate::model::ItemStatus::Done) {
            "COMPLETED"
        } else {
            "NEEDS-ACTION"
        }
    ));
    if let Some(done) = item.completed_at {
        out.push(format!("COMPLETED:{}", fmt_utc(done)));
    }
    push_common(item, out);
    if let Some(spec) = item.recurrence.as_deref() {
        if let Some(rrule) = rrule_of(spec, item.due_all_day) {
            out.push(format!("RRULE:{rrule}"));
        }
    }
    if !item.recurrence_exdates.is_empty() {
        let list = item
            .recurrence_exdates
            .iter()
            .map(|t| fmt_utc(*t))
            .collect::<Vec<_>>()
            .join(",");
        out.push(format!("EXDATE:{list}"));
    }
    for r in &item.reminders {
        if let Some(prop) = valarm_prop(&r.spec, true) {
            out.push("BEGIN:VALARM".into());
            out.push("ACTION:DISPLAY".into());
            out.push(format!(
                "DESCRIPTION:{}",
                escape(&crate::model::display_title(item))
            ));
            out.push(prop);
            out.push("END:VALARM".into());
        }
    }
    out.push("END:VTODO".into());
}

/// SUMMARY / DESCRIPTION / CATEGORIES / URL 公共行。
fn push_common(item: &Item, out: &mut Vec<String>) {
    out.push(format!(
        "SUMMARY:{}",
        escape(&crate::model::display_title(item))
    ));
    let mut desc = item.note.clone().unwrap_or_default();
    desc.push_str(&format!("\nmyday://item/{}", item.id));
    out.push(format!("DESCRIPTION:{}", escape(&desc)));
    if !item.tags.is_empty() {
        out.push(format!(
            "CATEGORIES:{}",
            item.tags
                .iter()
                .map(|t| escape(t))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
}

/// 重复规则 → RRULE（weekly 的 n 为 1..=7 周一始）；结束条件映射 UNTIL/COUNT。
fn rrule_of(spec: &str, all_day: bool) -> Option<String> {
    use crate::recurrence::{RecurKind, Recurrence};
    let rec = Recurrence::parse(spec).ok()?;
    let mut out = match rec.kind {
        RecurKind::Daily => "FREQ=DAILY".to_string(),
        RecurKind::Weekly(n) => {
            const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
            format!("FREQ=WEEKLY;BYDAY={}", DAYS[(n - 1) as usize])
        }
        RecurKind::Monthly(d) => format!("FREQ=MONTHLY;BYMONTHDAY={d}"),
    };
    if let Some(u) = rec.until {
        // 含当天的本地最后日。RFC 5545：UNTIL 值类型须与 DTSTART 一致——
        // 全天锚点用 DATE 形式，其余用该日本地 23:59:59 → UTC 的 DATE-TIME
        if all_day {
            out.push_str(&format!(";UNTIL={}", u.format("%Y%m%d")));
        } else {
            let local_end =
                crate::tpltime::local_to_utc(u.and_hms_opt(23, 59, 59).expect("valid clock"));
            out.push_str(&format!(";UNTIL={}", local_end.format("%Y%m%dT%H%M%SZ")));
        }
    }
    if let Some(c) = rec.count {
        out.push_str(&format!(";COUNT={c}"));
    }
    Some(out)
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

// ----------------------------------------------------------------------
// 导入（SPRINT2-SPEC §6 扩展：本地 .ics 文件一次性导入）
// ----------------------------------------------------------------------

/// 导入报告（CLI `myday import ics` / 设置页共用）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportReport {
    pub events: usize,
    pub tasks: usize,
    /// 随条目导入的提醒条数
    pub reminders: usize,
    /// myday 回链命中库内已有条目而跳过的组件数（重复导入保护）
    pub skipped_duplicates: usize,
    /// 无法映射而降级 / 跳过的告警（组件摘要 + 原因）
    pub warnings: Vec<String>,
}

/// 单个属性：`NAME;PARAM=VAL;PARAM=VAL:value`（RFC 5545 content line）。
struct Prop {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Prop {
    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// 折行还原：`\r\n ` / `\n\t` 开头的续行拼回上一行（RFC 5545 §3.1）。
fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.lines() {
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !out.is_empty() {
            let last = out.last_mut().expect("non-empty");
            last.push_str(&raw[1..]);
        } else {
            out.push(raw.to_string());
        }
    }
    out
}

/// 拆 name / params / value：首个不在双引号内的 `:` 之前为名字与参数段。
fn parse_prop(line: &str) -> Option<Prop> {
    let mut in_quotes = false;
    let mut split_at = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                split_at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let i = split_at?;
    let (head, value) = (&line[..i], &line[i + 1..]);
    let mut segs = head.split(';');
    let name = segs.next()?.trim().to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    let mut params = Vec::new();
    for seg in segs {
        if let Some((k, v)) = seg.split_once('=') {
            params.push((
                k.trim().to_ascii_uppercase(),
                v.trim().trim_matches('"').to_string(),
            ));
        }
    }
    Some(Prop {
        name,
        params,
        value: value.to_string(),
    })
}

/// RFC 5545 TEXT 反转义。
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(x) => out.push(x),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// CATEGORIES 拆分：只按未转义的逗号切（转义的 `\,` 属于名字本身）。
fn split_categories(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            cur.push(c); // 前一轮已把反斜杠压入，转义对整体交给 unescape
            escaped = false;
        } else if c == '\\' {
            cur.push(c);
            escaped = true;
        } else if c == ',' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur); // 尾部悬挂反斜杠已在循环内压入
    out.into_iter()
        .map(|s| unescape(s.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// ICS 日期时间值：DATE（VALUE=DATE 或 8 位）→ 本地日；DATE-TIME → UTC / 浮动按本地。
enum IcsDt {
    Date(chrono::NaiveDate),
    DateTime(chrono::DateTime<Utc>),
}

fn parse_ics_dt(p: &Prop) -> Option<IcsDt> {
    let v = p.value.trim();
    let is_date = p
        .param("VALUE")
        .is_some_and(|v| v.eq_ignore_ascii_case("date"))
        || (v.len() == 8 && v.chars().all(|c| c.is_ascii_digit()));
    if is_date {
        return chrono::NaiveDate::parse_from_str(v, "%Y%m%d")
            .ok()
            .map(IcsDt::Date);
    }
    let dt = if let Some(stripped) = v.strip_suffix('Z') {
        chrono::NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S")
            .ok()?
            .and_local_timezone(Utc)
            .single()?
    } else {
        // 浮动时刻（无时区）：按本地时区解释（与 MyDay 的本地优先一致）
        let naive = chrono::NaiveDateTime::parse_from_str(v, "%Y%m%dT%H%M%S").ok()?;
        use chrono::TimeZone;
        chrono::Local
            .from_local_datetime(&naive)
            .earliest()?
            .with_timezone(&Utc)
    };
    Some(IcsDt::DateTime(dt))
}

/// 本地日 → 该日 [start, end] 的 UTC 表示：start = 零点，end = 23:59:59。
fn local_day_range(day: chrono::NaiveDate) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    use chrono::TimeZone;
    let at = |h: u32, m: u32, s: u32| -> chrono::DateTime<Utc> {
        chrono::Local
            .from_local_datetime(&day.and_hms_opt(h, m, s).expect("valid clock"))
            .earliest()
            .unwrap_or_else(|| Utc::now().with_timezone(&chrono::Local))
            .with_timezone(&Utc)
    };
    (at(0, 0, 0), at(23, 59, 59))
}

/// ICS 时长解析（`-PT15M` / `P1D` / `PT2H30M` / `-P1WT1H`）。
fn parse_ics_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix('P')?;
    let mut total: i64 = 0;
    let mut num = String::new();
    let mut in_time = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            unit @ ('W' | 'D' | 'H' | 'M' | 'S') => {
                if !in_time && matches!(unit, 'H' | 'M' | 'S') {
                    return None; // 日期段不允许时分秒
                }
                let n: i64 = num.parse().ok()?;
                let mult = match unit.to_ascii_uppercase() {
                    'W' => 7 * 86_400,
                    'D' => 86_400,
                    'H' => 3_600,
                    'M' => 60,
                    'S' => 1,
                    _ => unreachable!("matched above"),
                };
                total += n.checked_mul(mult)?;
                num.clear();
                any = true;
            }
            _ => return None,
        }
    }
    if !num.is_empty() || !any {
        return None; // 尾部悬挂数字 / 无任何数字段 = 非法
    }
    let d = Duration::seconds(total);
    Some(if neg { -d } else { d })
}

/// 相对提醒 spec 组装：`@start` / `@start-15m`（零偏移省略；分钟溢出按 h / d 归档）。
fn relative_spec(base: &str, d: Duration) -> String {
    let mins = d.num_minutes();
    if mins == 0 {
        return base.to_string();
    }
    let sign = if mins < 0 { "-" } else { "+" };
    let a = mins.abs();
    if a % 1_440 == 0 {
        format!("{base}{sign}{}d", a / 1_440)
    } else if a % 60 == 0 {
        format!("{base}{sign}{}h", a / 60)
    } else {
        format!("{base}{sign}{a}m")
    }
}

/// RRULE → MyDay 重复文法。返回 None 表示放弃（附原因）；
/// UNTIL/COUNT 映射为 `;until=` / `;count=`（RFC 双写非法 → 忽略并告警）；
/// INTERVAL ≠ 1 无对应物：保留基础规则并告警（宁可少一条约束，不丢重复）。
fn rrule_to_spec(rrule: &str, start: chrono::DateTime<Utc>) -> (Option<String>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut freq = "";
    let mut byday: Vec<&str> = Vec::new();
    let mut bymonthday: Option<i64> = None;
    let mut until: Option<chrono::NaiveDate> = None;
    let mut count: Option<u32> = None;
    for part in rrule.split(';') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        match k.to_ascii_uppercase().as_str() {
            "FREQ" => freq = v,
            "BYDAY" => byday.extend(v.split(',').map(str::trim)),
            "BYMONTHDAY" => bymonthday = v.parse().ok(),
            "UNTIL" => {
                // DATE（YYYYMMDD）与 DATE-TIME（…THHMMSSZ，取本地日）都收
                let d = if let Ok(date) = chrono::NaiveDate::parse_from_str(v, "%Y%m%d") {
                    Some(date)
                } else {
                    chrono::NaiveDateTime::parse_from_str(v.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
                        .ok()
                        .map(|n| n.date())
                };
                until = d;
                if d.is_none() {
                    warnings.push(format!("RRULE 的 UNTIL=\"{v}\" 无法解析，已忽略"));
                }
            }
            "COUNT" => match v.parse::<u32>() {
                Ok(n) if n >= 1 => count = Some(n),
                _ => warnings.push(format!("RRULE 的 COUNT=\"{v}\" 非法，已忽略")),
            },
            "INTERVAL" if v != "1" => warnings.push(format!(
                "RRULE 的 INTERVAL={v} 无对应物，已按每 1 {}/次导入",
                if freq.eq_ignore_ascii_case("weekly") {
                    "周"
                } else {
                    "期"
                }
            )),
            _ => {}
        }
    }
    if until.is_some() && count.is_some() {
        warnings.push("RRULE 同时带 UNTIL 与 COUNT（RFC 非法），结束条件已忽略".into());
        until = None;
        count = None;
    }
    let local_start = start.with_timezone(&chrono::Local);
    const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
    let end_suffix = |spec: &mut String| {
        if let Some(u) = until {
            spec.push_str(&format!(";until={u}"));
        }
        if let Some(c) = count {
            spec.push_str(&format!(";count={c}"));
        }
    };
    let mut spec = if freq.eq_ignore_ascii_case("daily") {
        Some("@daily".to_string())
    } else if freq.eq_ignore_ascii_case("weekly") {
        if byday.len() > 1 {
            warnings.push("RRULE 的多 BYDAY 周重复无对应物，已按单日导入".into());
            byday.truncate(1);
        }
        match byday.first() {
            Some(d) => DAYS
                .iter()
                .position(|x| x.eq_ignore_ascii_case(d))
                .map(|i| format!("@weekly:{}", i + 1)),
            // RFC 默认 = DTSTART 的星期几
            None => Some(format!(
                "@weekly:{}",
                ((local_start.weekday().num_days_from_monday()) + 1)
            )),
        }
    } else if freq.eq_ignore_ascii_case("monthly") {
        match bymonthday {
            Some(d) if (1..=31).contains(&d) => Some(format!("@monthly:{d}")),
            Some(d) => {
                warnings.push(format!("RRULE 的 BYMONTHDAY={d} 无对应物，已放弃重复"));
                None
            }
            // RFC 默认 = DTSTART 的「每月第 N 天」
            None => Some(format!("@monthly:{}", local_start.day())),
        }
    } else {
        warnings.push(format!("重复频率 {freq} 无对应物，已按不重复导入"));
        None
    };
    if let Some(s) = spec.as_mut() {
        end_suffix(s);
    }
    (spec, warnings)
}

/// 提醒收集：VALARM TRIGGER 列表 → (NewReminder 列表, 告警)。
/// 相对 TRIGGER：task 锚 @due（RFC：VTODO 无 DTSTART 的默认语义）、event 锚 @start；
/// RELATED=END：task 仍锚 @due、event 折算为相对 start 的总偏移；绝对时刻原样导入。
fn alarms_to_reminders(
    triggers: &[Prop],
    is_task: bool,
    start: Option<chrono::DateTime<Utc>>,
    end: Option<chrono::DateTime<Utc>>,
    label: &str,
) -> (Vec<NewReminder>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    for trigger in triggers {
        let related_end = trigger
            .param("RELATED")
            .is_some_and(|r| r.eq_ignore_ascii_case("END"));
        let value_is_dt = trigger
            .param("VALUE")
            .is_some_and(|v| v.eq_ignore_ascii_case("date-time"))
            || trigger.value.trim().ends_with('Z');
        if value_is_dt {
            match parse_ics_dt(trigger) {
                Some(IcsDt::DateTime(at)) => {
                    out.push(NewReminder {
                        spec: crate::store::dt(at),
                        channel: "notify".into(),
                    });
                }
                _ => warnings.push(format!("{label}: 绝对 TRIGGER 无法解析，已跳过该提醒")),
            }
            continue;
        }
        let Some(off) = parse_ics_duration(&trigger.value) else {
            warnings.push(format!(
                "{label}: TRIGGER 时长 \"{}\" 无法解析，已跳过该提醒",
                trigger.value
            ));
            continue;
        };
        let spec = if related_end && !is_task {
            match (start, end) {
                // event 无 @end 基准：折算为相对 start 的总偏移
                (Some(s), Some(e)) => relative_spec("@start", off + (e - s)),
                _ => {
                    warnings.push(format!("{label}: RELATED=END 缺少结束时间，已跳过该提醒"));
                    continue;
                }
            }
        } else if is_task {
            relative_spec("@due", off)
        } else {
            relative_spec("@start", off)
        };
        out.push(NewReminder {
            spec,
            channel: "notify".into(),
        });
    }
    (out, warnings)
}

/// 摘要 + 组件描述（告警文案用）：`VEVENT「团队周会」`。
fn comp_label(kind: &str, title: Option<&str>) -> String {
    match title {
        Some(t) => format!("{kind}「{}」", t.chars().take(24).collect::<String>()),
        None => format!("未命名{kind}"),
    }
}

/// myday 回链：`myday://item/<id>`（导出时附在 DESCRIPTION 末行）。
fn extract_backlink(note: &str) -> (Option<String>, String) {
    if let Some(pos) = note.find("myday://item/") {
        let id: String = note[pos + "myday://item/".len()..]
            .chars()
            .take_while(|c| !c.is_whitespace())
            .collect();
        if !id.is_empty() {
            let rest = format!(
                "{}{}",
                &note[..pos],
                &note[pos + "myday://item/".len() + id.len()..]
            );
            return (Some(id), rest.trim().to_string());
        }
    }
    (None, note.to_string())
}

#[derive(Default)]
struct Comp {
    is_todo: bool,
    uid: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    categories: Vec<String>,
    dtstart: Option<Prop>,
    dtend: Option<Prop>,
    due: Option<Prop>,
    rrule: Option<String>,
    status: Option<String>,
    completed: Option<Prop>,
    /// EXDATE 原始值（逗号分隔的 DATE / DATE-TIME，可有多个属性）
    exdates: Vec<String>,
    alarms: Vec<Prop>,
}

/// 解析并导入 ICS 文本。逐组件尽力而为：失败只产生 warning。
pub fn import_ics(store: &Store, text: &str) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut cur: Option<Comp> = None;
    let mut in_alarm = false;

    for line in unfold_lines(text) {
        let trimmed = line.trim_end();
        let upper = trimmed.to_ascii_uppercase();
        if let Some(name) = upper.strip_prefix("BEGIN:") {
            match name.trim() {
                "VEVENT" => cur = Some(Comp::default()),
                "VTODO" => {
                    cur = Some(Comp {
                        is_todo: true,
                        ..Comp::default()
                    })
                }
                "VALARM" if cur.is_some() => in_alarm = true,
                _ => {}
            }
            continue;
        }
        if let Some(name) = upper.strip_prefix("END:") {
            match name.trim() {
                "VALARM" => in_alarm = false,
                "VEVENT" | "VTODO" => {
                    if let Some(c) = cur.take() {
                        import_component(store, c, &mut report)?;
                    }
                }
                _ => {}
            }
            continue;
        }
        let Some(p) = parse_prop(trimmed) else {
            continue;
        };
        if in_alarm {
            if let Some(c) = cur.as_mut() {
                if p.name == "TRIGGER" {
                    c.alarms.push(p);
                }
            }
            continue;
        }
        if let Some(c) = cur.as_mut() {
            match p.name.as_str() {
                "UID" => c.uid = Some(p.value.trim().to_string()),
                "SUMMARY" => c.summary = Some(unescape(p.value.trim())),
                "DESCRIPTION" => c.description = Some(unescape(&p.value)),
                "CATEGORIES" => c.categories.extend(split_categories(&p.value)),
                "DTSTART" => c.dtstart = Some(p),
                "DTEND" => c.dtend = Some(p),
                "DUE" => c.due = Some(p),
                "RRULE" => c.rrule = Some(p.value.trim().to_string()),
                "EXDATE" => c.exdates.push(p.value.trim().to_string()),
                "STATUS" => c.status = Some(p.value.trim().to_ascii_uppercase()),
                "COMPLETED" => c.completed = Some(p),
                _ => {}
            }
        }
    }
    Ok(report)
}

/// 单组件 → 条目。数据性失败只记 warning 不中断；存储级错误向上传播。
fn import_component(store: &Store, c: Comp, report: &mut ImportReport) -> Result<()> {
    let kind = if c.is_todo { "VTODO" } else { "VEVENT" };
    let label = comp_label(kind, c.summary.as_deref());

    // 重复导入保护 1：myday 回链命中库内条目
    let note_raw = c.description.clone().unwrap_or_default();
    let (backlink, note) = extract_backlink(&note_raw);
    if let Some(id) = &backlink {
        if store.get_item(id).is_ok() {
            report.skipped_duplicates += 1;
            return Ok(());
        }
    }

    let title = c.summary.clone().filter(|s| !s.trim().is_empty());
    if title.is_none() && note.trim().is_empty() {
        report
            .warnings
            .push(format!("{label}: 无 SUMMARY 与 DESCRIPTION，已跳过"));
        return Ok(());
    }

    // ---- 时间字段（组件的硬前提，缺失/非法 = 跳过组件） --------------------
    let mut new = NewItem {
        item_type: Some(if c.is_todo {
            ItemType::Task
        } else {
            ItemType::Event
        }),
        title,
        note: if note.trim().is_empty() {
            None
        } else {
            Some(note)
        },
        tags: c.categories,
        // 重复导入保护 2：外部 UID 幂等（同文件重导返回已有条目）
        idempotency_key: c.uid.as_deref().map(|u| format!("ics-{}", u)),
        ..Default::default()
    };

    let mut warnings: Vec<String> = Vec::new();

    if c.is_todo {
        if let Some(due) = &c.due {
            match parse_ics_dt(due) {
                Some(IcsDt::Date(d)) => {
                    new.due_all_day = true;
                    new.due_at = Some(local_day_range(d).1);
                }
                Some(IcsDt::DateTime(at)) => new.due_at = Some(at),
                None => warnings.push(format!("{label}: DUE 无法解析，已按无截止导入")),
            }
        }
        if c.status.as_deref() == Some("COMPLETED") {
            new.status = Some(ItemStatus::Done);
        }
    } else {
        let Some(start_prop) = &c.dtstart else {
            report
                .warnings
                .push(format!("{label}: 缺少 DTSTART，已跳过"));
            return Ok(());
        };
        match parse_ics_dt(start_prop) {
            Some(IcsDt::Date(d)) => {
                // 全天日程：DTEND DATE 为排他端点（RFC 5545）
                new.all_day = true;
                let (s, _) = local_day_range(d);
                new.start_at = Some(s);
                let end_day = match &c.dtend {
                    Some(p) => match parse_ics_dt(p) {
                        Some(IcsDt::Date(de)) if de > d => de - chrono::Duration::days(1),
                        _ => d,
                    },
                    None => d,
                };
                new.end_at = Some(local_day_range(end_day).1);
            }
            Some(IcsDt::DateTime(at)) => {
                new.start_at = Some(at);
                if let Some(p) = &c.dtend {
                    if let Some(IcsDt::DateTime(e)) = parse_ics_dt(p) {
                        new.end_at = Some(e)
                    }
                }
            }
            None => {
                report
                    .warnings
                    .push(format!("{label}: DTSTART 无法解析，已跳过"));
                return Ok(());
            }
        }
    }

    // ---- 重复规则（锚点 = DUE（task）/ DTSTART（event）） -------------------
    if let Some(rrule) = &c.rrule {
        let anchor_at = if c.is_todo { new.due_at } else { new.start_at };
        match anchor_at {
            Some(at) => {
                let (spec, mut w) = rrule_to_spec(rrule, at);
                warnings.append(&mut w);
                new.recurrence = spec;
            }
            None => warnings.push(format!("{label}: 重复规则缺少锚点时间，已按不重复导入")),
        }
    }
    if c.is_todo && new.recurrence.is_some() && new.due_at.is_none() {
        new.recurrence = None;
    }

    // ---- 单次例外（EXDATE）--------------------------------------------------
    if new.recurrence.is_some() && !c.exdates.is_empty() {
        for raw in &c.exdates {
            for v in raw.split(',') {
                let v = v.trim();
                let parsed = if let Ok(date) = chrono::NaiveDate::parse_from_str(v, "%Y%m%d") {
                    Some(local_day_range(date).0)
                } else if let Some(stripped) = v.strip_suffix('Z') {
                    chrono::NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S")
                        .ok()
                        .and_then(|n| n.and_local_timezone(Utc).single())
                } else {
                    None
                };
                match parsed {
                    Some(t) => new.recurrence_exdates.push(t),
                    None => warnings.push(format!("{label}: EXDATE \"{v}\" 无法解析，已跳过")),
                }
            }
        }
        new.recurrence_exdates.sort();
        new.recurrence_exdates.dedup();
    }

    // ---- 提醒（VALARM TRIGGER）---------------------------------------------
    let (reminders, alarm_warnings) =
        alarms_to_reminders(&c.alarms, c.is_todo, new.start_at, new.end_at, &label);
    new.reminders = reminders;
    warnings.extend(alarm_warnings);

    // 重复导入保护 2（幂等键预检）：同文件重导时 add_item 幂等返回已有条目，
    // 但按「跳过」计数——导入报告不该把无操作算成新导入
    if let Some(key) = new.idempotency_key.as_deref() {
        if store.find_by_idempotency_key(key)?.is_some() {
            report.skipped_duplicates += 1;
            report.warnings.extend(warnings);
            return Ok(());
        }
    }

    match store.add_item(new) {
        Ok(item) => {
            if item.item_type == ItemType::Event {
                report.events += 1;
            } else {
                report.tasks += 1;
            }
            report.reminders += item.reminders.len();
        }
        Err(e) => report.warnings.push(format!("{label}: 导入失败（{e}）")),
    }
    report.warnings.extend(warnings);
    Ok(())
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
        assert_eq!(rrule_of("@daily", false).as_deref(), Some("FREQ=DAILY"));
        assert_eq!(
            rrule_of("@weekly:3", false).as_deref(),
            Some("FREQ=WEEKLY;BYDAY=WE")
        );
        assert_eq!(
            rrule_of("@weekly:7", false).as_deref(),
            Some("FREQ=WEEKLY;BYDAY=SU")
        );
        assert_eq!(
            rrule_of("@monthly:15", false).as_deref(),
            Some("FREQ=MONTHLY;BYMONTHDAY=15")
        );
        assert_eq!(rrule_of("bogus", false), None);
        // 结束条件：count 直接映射；until 全天用 DATE 形式
        assert_eq!(
            rrule_of("@daily;count=10", false).as_deref(),
            Some("FREQ=DAILY;COUNT=10")
        );
        let all_day = rrule_of("@daily;until=2026-12-31", true).unwrap();
        assert!(all_day.ends_with(";UNTIL=20261231"), "{all_day}");
        let timed = rrule_of("@daily;until=2026-12-31", false).unwrap();
        // 该日本地 23:59:59 → UTC（时钟随时区变化，只断言日期与 DATE-TIME 形式）
        assert!(
            timed.contains("UNTIL=20261231T") && timed.ends_with("Z"),
            "{timed}"
        );
    }

    #[test]
    fn duration_parse_and_relative_spec() {
        assert_eq!(parse_ics_duration("-PT10M"), Some(Duration::minutes(-10)));
        assert_eq!(parse_ics_duration("PT1H30M"), Some(Duration::minutes(90)));
        assert_eq!(parse_ics_duration("P1D"), Some(Duration::days(1)));
        assert_eq!(
            parse_ics_duration("-P1WT1H"),
            Some(-Duration::days(7) - Duration::hours(1))
        );
        assert_eq!(parse_ics_duration("PT"), None);
        assert_eq!(parse_ics_duration("PX"), None);
        assert_eq!(
            relative_spec("@start", Duration::minutes(-15)),
            "@start-15m"
        );
        assert_eq!(relative_spec("@due", Duration::hours(-1)), "@due-1h");
        assert_eq!(relative_spec("@due", Duration::days(-2)), "@due-2d");
        assert_eq!(relative_spec("@start", Duration::minutes(30)), "@start+30m");
        assert_eq!(relative_spec("@start", Duration::zero()), "@start");
    }

    #[test]
    fn unfold_and_categories() {
        let lines = unfold_lines("SUMMARY:hello\r\n there\r\nDESCRIPTION:a\\,b\r\n");
        // RFC 5545 反折行 = 去掉 CRLF + 单个空白标记（折点本身不产生空格）
        assert_eq!(
            lines,
            vec![
                "SUMMARY:hellothere".to_string(),
                "DESCRIPTION:a\\,b".to_string()
            ]
        );
        assert_eq!(
            split_categories("工作, a\\,b, 健康"),
            vec!["工作", "a,b", "健康"]
        );
        assert_eq!(unescape("a\\nb\\\\c"), "a\nb\\c");
    }

    #[test]
    fn backlink_extraction() {
        let (id, rest) = extract_backlink("原备注\nmyday://item/evt_ab12cd34");
        assert_eq!(id.as_deref(), Some("evt_ab12cd34"));
        assert_eq!(rest, "原备注");
        assert_eq!(extract_backlink("没有链接").0, None);
    }

    fn test_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open(&dir.path().join("myday.db"), dir.path()).expect("store");
        (dir, store)
    }

    #[test]
    fn import_roundtrip_of_myday_export() {
        let (_dir_a, a) = test_store();
        let start = crate::tpltime::local_to_utc(
            chrono::Local::now()
                .date_naive()
                .and_hms_opt(14, 0, 0)
                .unwrap(),
        );
        a.add_item(NewItem {
            title: Some("周会".into()),
            note: Some("备注".into()),
            start_at: Some(start),
            tags: vec!["工作".into()],
            recurrence: Some("@weekly:3".into()),
            reminders: vec![NewReminder {
                spec: "@start-30m".into(),
                channel: "notify".into(),
            }],
            ..Default::default()
        })
        .expect("add");
        let ics = export_ics(&a).expect("export");

        let (_dir_b, b) = test_store();
        let report = import_ics(&b, &ics).expect("import");
        assert_eq!(report.events, 1);
        assert_eq!(report.tasks, 0);
        assert_eq!(report.reminders, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let items = b.list_items(&ListFilter::default()).expect("list");
        let ev = &items[0];
        assert_eq!(crate::model::display_title(ev), "周会");
        assert_eq!(ev.recurrence.as_deref(), Some("@weekly:3"));
        assert_eq!(ev.tags, vec!["工作"]);
        assert_eq!(ev.reminders.len(), 1);
        assert_eq!(ev.reminders[0].spec, "@start-30m");
        assert_eq!(ev.note.as_deref(), Some("备注"));

        // 重复导入：回链命中 → 跳过
        let again = import_ics(&b, &ics).expect("import again");
        assert_eq!(again.skipped_duplicates, 1);
        assert_eq!(again.events, 0);
    }

    #[test]
    fn import_foreign_ics_events_and_todos() {
        let (_dir, store) = test_store();
        let ics = concat!(
            "BEGIN:VCALENDAR\r\n",
            "VERSION:2.0\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:ext-1@example.com\r\n",
            "DTSTAMP:20260901T000000Z\r\n",
            "SUMMARY:两天营期\r\n",
            "DTSTART;VALUE=DATE:20261001\r\n",
            "DTEND;VALUE=DATE:20261003\r\n",
            "RRULE:FREQ=WEEKLY;BYDAY=TH\r\n",
            "BEGIN:VALARM\r\n",
            "ACTION:DISPLAY\r\n",
            "TRIGGER:-PT15M\r\n",
            "END:VALARM\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VEVENT\r\n",
            "UID:ext-2@example.com\r\n",
            "DTSTAMP:20260901T000000Z\r\n",
            "SUMMARY:浮动时刻会\r\n",
            "DTSTART:20261005T090000\r\n",
            "DTEND:20261005T100000\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VTODO\r\n",
            "UID:ext-3@example.com\r\n",
            "DTSTAMP:20260901T000000Z\r\n",
            "SUMMARY:交房租\\,别忘\r\n",
            "CATEGORIES:生活,财务\r\n",
            "DUE;VALUE=DATE:20261010\r\n",
            "RRULE:FREQ=MONTHLY;BYMONTHDAY=10;UNTIL=20271210T000000Z\r\n",
            "BEGIN:VALARM\r\n",
            "ACTION:DISPLAY\r\n",
            "TRIGGER;RELATED=END:-PT1H\r\n",
            "END:VALARM\r\n",
            "END:VTODO\r\n",
            "BEGIN:VTODO\r\n",
            "UID:ext-4@example.com\r\n",
            "DTSTAMP:20260901T000000Z\r\n",
            "SUMMARY:无截止想法\r\n",
            "END:VTODO\r\n",
            "END:VCALENDAR\r\n",
        );
        let report = import_ics(&store, ics).expect("import");
        assert_eq!(report.events, 2, "{:?}", report.warnings);
        assert_eq!(report.tasks, 2);
        // 营期与房租各带 1 条 VALARM；浮动时刻会无 VALARM → 按设置自动补默认提醒
        assert_eq!(report.reminders, 3, "{:?}", report.warnings);

        let items = store.list_items(&ListFilter::default()).expect("list");
        let camp = items
            .iter()
            .find(|i| i.title.as_deref() == Some("两天营期"))
            .expect("camp");
        assert!(camp.all_day);
        assert_eq!(camp.recurrence.as_deref(), Some("@weekly:4")); // 10-01 恰是周四，BYDAY=TH 一致
        assert_eq!(camp.reminders[0].spec, "@start-15m");
        // 全天两天：10-01 00:00 → 10-02 23:59（DTEND 排他）
        let local_start = camp.start_at.unwrap().with_timezone(&chrono::Local);
        let local_end = camp.end_at.unwrap().with_timezone(&chrono::Local);
        assert_eq!(local_start.format("%m-%d %H:%M").to_string(), "10-01 00:00");
        assert_eq!(local_end.format("%m-%d %H:%M").to_string(), "10-02 23:59");

        let rent = items
            .iter()
            .find(|i| i.title.as_deref() == Some("交房租,别忘"))
            .expect("rent");
        assert_eq!(rent.item_type, ItemType::Task);
        assert!(rent.due_all_day);
        // UNTIL=20271210T000000Z → 本地日 2027-12-10，映射进扩展文法
        assert_eq!(
            rent.recurrence.as_deref(),
            Some("@monthly:10;until=2027-12-10")
        );
        assert_eq!(rent.reminders[0].spec, "@due-1h");
        assert_eq!(rent.tags, vec!["生活", "财务"]);

        let idea = items
            .iter()
            .find(|i| i.title.as_deref() == Some("无截止想法"))
            .expect("idea");
        assert!(idea.due_at.is_none());
        assert!(idea.recurrence.is_none());

        // 幂等重导：外部 UID 命中 idempotency_key → 按跳过计数，不新增
        let again = import_ics(&store, ics).expect("import again");
        assert_eq!(again.skipped_duplicates, 4);
        assert_eq!(again.events, 0);
        let all = store.list_items(&ListFilter::default()).expect("list");
        assert_eq!(all.len(), items.len(), "幂等重导不应新增条目");
    }

    #[test]
    fn import_skips_malformed_components_with_warning() {
        let (_dir, store) = test_store();
        let ics = concat!(
            "BEGIN:VEVENT\r\n",
            "UID:x\r\n",
            "SUMMARY:没有开始时间\r\n",
            "END:VEVENT\r\n",
            "BEGIN:VTODO\r\n",
            "UID:y\r\n",
            "RRULE:FREQ=DAILY\r\n",
            "SUMMARY:重复无截止\r\n",
            "END:VTODO\r\n",
        );
        let report = import_ics(&store, ics).expect("import");
        assert_eq!(report.events, 0);
        assert_eq!(report.tasks, 1);
        assert!(report.warnings.iter().any(|w| w.contains("DTSTART")));
        assert!(report.warnings.iter().any(|w| w.contains("锚点")));
    }
}

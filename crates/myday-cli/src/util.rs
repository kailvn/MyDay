//! 时间参数解析。
//!
//! 主流程不依赖自然语言解析（需求 §1.3、§八.6），
//! 只接受四种严格格式，全部按本地时区解释：
//! - RFC3339：`2026-09-16T14:30:00+08:00`
//! - `2026-09-16T14:30` / `2026-09-16 14:30`
//! - `2026-09-16`（零点）

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use myday_core::{MyDayError, Result};

pub fn parse_dt_arg(s: &str) -> Result<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    let naive = ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"]
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(s, fmt).ok())
        .or_else(|| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().map(|d| d.and_hms_opt(0, 0, 0).unwrap()));
    let Some(naive) = naive else {
        return Err(MyDayError::Invalid(format!(
            "无法解析时间 \"{s}\"（支持 2026-09-16T14:30 / 2026-09-16 14:30 / 2026-09-16 / RFC3339）"
        )));
    };
    Local
        .from_local_datetime(&naive)
        .single()
        .map(|dt| dt.with_timezone(&Utc))
        .ok_or_else(|| MyDayError::Invalid(format!("时间 \"{s}\" 在本地时区无效")))
}

/// 截止参数：`2026-09-16` → (当天零点, 仅日期)；带时间 → (时刻, false)
pub fn parse_due_arg(s: &str) -> Result<(DateTime<Utc>, bool)> {
    let s = s.trim();
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = Local
            .from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .ok_or_else(|| MyDayError::Invalid(format!("日期 \"{s}\" 在本地时区无效")))?;
        return Ok((dt.with_timezone(&Utc), true));
    }
    Ok((parse_dt_arg(s)?, false))
}

pub type DateTimeRange = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// 将 --date / --from / --to 解析为查询范围。
/// `to` 若为纯日期则取当天结束（23:59:59）。
pub fn resolve_range(
    date: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<DateTimeRange> {
    if let Some(d) = date {
        let start = parse_dt_arg(d)?;
        let end = start + chrono::Duration::days(1) - chrono::Duration::seconds(1);
        return Ok((Some(start), Some(end)));
    }
    let from = from.map(parse_dt_arg).transpose()?;
    let to = match to {
        Some(t) => {
            if NaiveDate::parse_from_str(t.trim(), "%Y-%m-%d").is_ok() {
                let midnight = parse_dt_arg(t)?;
                Some(midnight + chrono::Duration::days(1) - chrono::Duration::seconds(1))
            } else {
                Some(parse_dt_arg(t)?)
            }
        }
        None => None,
    };
    Ok((from, to))
}

/// 提醒 spec 参数：@token 原样透传（如 @due-1d / @dailyT09:00），
/// 其余按时间格式解析为绝对 RFC3339。
pub fn parse_reminder_spec(s: &str) -> Result<String> {
    let s = s.trim();
    if s.starts_with('@') {
        return Ok(s.to_string());
    }
    Ok(parse_dt_arg(s)?
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// 重复规则参数：@daily / @weekly:1-7（1=周一）/ @monthly:1-31（SPRINT-SPEC §2）。
/// 严格解析（规范化为 as_str），非法值直接报错（exit 2）。
pub fn parse_recurrence_arg(s: &str) -> Result<String> {
    Ok(myday_core::recurrence::Recurrence::parse(s.trim())?.as_str())
}

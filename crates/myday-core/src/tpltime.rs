//! 模板时间占位符：模板 defaults 的时间列存「意图」（token），条目存「事实」（UTC RFC3339）。
//!
//! 文法：`@<base>[<offset>][T<time>]`
//! - base   = `now` | `next_hour` | `start` | `due` | `daily` | `d0` | `d+N` | `d-N`
//! - offset = `+数字(m|h|d)` / `-数字(m|h|d)`（仅 now / start / due 基准）
//! - time   = `HH:MM` | `end`（=23:59，仅 d / daily 基准）
//!
//! 两族语义（解析锚点见 [`ResolveCtx`]）：
//! - clock 族（`now` / `next_hour`）：相对当前时刻，无视锚点日——「30分钟前」在
//!   任何语境都相对此刻；
//! - day 族（`d*`）：相对锚点日（日历选中日，缺省今天）的本地时区时刻——
//!   日历选中 9 月 20 日时，模板里的「当天 9 点」解析为 9 月 20 日 09:00。
//!
//! `start` / `due` 基准按列收窄（`@start` 仅 end_at 与提醒；`@due` / `@daily`
//! 仅提醒 spec）——提醒存相对意图（INTERACTION §6）：`@due-1d`（截止前一天）
//! 随条目时间变动自动跟随，`@dailyT09:00`（期间每天 9 点）由提醒环逐日展开。
//! `now` / `next_hour` 不允许作提醒 spec（存储后会随评估时刻漂移，永不触发）。
//!
//! 校验收窄（宁严勿猜）：`occurred_at` 只接受 clock 族的现在/过去方向，
//! 未来方向的记录语义不成立。UI 的「占位池」是本文件官方例子
//! （[`OFFICIAL_EXAMPLES`]）的人工筛选子集，池可增删，文法不变。

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};

/// 模板 token 的解析上下文（提醒 spec 不走这里——见 reminder.rs::occurrences）。
#[derive(Debug, Clone, Copy)]
pub struct ResolveCtx {
    pub now: DateTime<Utc>,
    /// 本地锚点日（日历选中日；缺省今天）
    pub anchor_day: NaiveDate,
    /// 已解析的开始时间（`@start` 基准；end_at 依赖）
    pub start: Option<DateTime<Utc>>,
}

impl ResolveCtx {
    pub fn new(now: DateTime<Utc>, anchor_day: NaiveDate) -> Self {
        Self { now, anchor_day, start: None }
    }
    pub fn with_start(mut self, start: Option<DateTime<Utc>>) -> Self {
        self.start = start;
        self
    }
}

/// 官方示例池（文法子集 + 人话标签）；UI 在此基础上筛选可用项。
pub const OFFICIAL_EXAMPLES: &[(&str, &str)] = &[
    ("@now", "现在"),
    ("@now-30m", "30分钟前"),
    ("@next_hour", "下一小时"),
    ("@d0T09:00", "当天 09:00"),
    ("@d0T18:00", "当天 18:00"),
    ("@d0Tend", "当天结束"),
    ("@d+1T09:00", "明天 09:00"),
    ("@d+1Tend", "明天结束"),
    ("@start+30m", "开始后30分钟"),
];

/// `@` 开头即视为 token（区别于绝对 RFC3339 / 日期字符串）。
pub fn is_token(v: &str) -> bool {
    v.starts_with('@')
}

/// 是否为「现在 / 现在过去方向」的 now 族 token（`@now` / `@now-30m` / `@now-1h`）。
/// 记录的 occurred_at 只允许这类占位——未来的「记录」语义不成立。
pub fn is_past_now_token(tok: &str) -> bool {
    if tok == "@now" {
        return true;
    }
    let Some(rest) = tok.strip_prefix("@now-") else {
        return false;
    };
    let (digits, unit) = rest.split_at(rest.len().saturating_sub(1));
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && matches!(unit, "m" | "h")
}

/// 模板保存时的语法与列合法性校验（不做解析——解析依赖运行时上下文）。
/// column = items 时间列名；提醒 spec 用 "reminder"。
pub fn validate_for_column(tok: &str, column: &str) -> Result<(), String> {
    parse_spec(tok, column)
        .map(|_| ())
        .map_err(|e| format!("时间占位 {tok}：{e}"))
}

/// 解析 token 为具体 UTC 时刻（一次性语义；`@due` / `@daily` 是提醒专属，
/// 由提醒环按条目时间展开，不做一次性解析）。
pub fn resolve(tok: &str, ctx: &ResolveCtx) -> Result<DateTime<Utc>, String> {
    match parse_spec(tok, "any")? {
        Spec::Now(off) => Ok(ctx.now + off),
        Spec::NextHour => Ok(next_hour(ctx.now)),
        Spec::Start(off) => ctx
            .start
            .map(|s| s + off)
            .ok_or_else(|| "@start 需要先有开始时间（end_at 依赖 start_at）".to_string()),
        Spec::Due(_) => Err("@due 仅提醒 spec 可用".into()),
        Spec::Daily(_) => Err("@daily 仅提醒 spec 可用".into()),
        Spec::Day(day_off, clock) => {
            let day = add_days(ctx.anchor_day, day_off)?;
            let naive = match clock {
                Some((h, m)) => day.and_hms_opt(h, m, 0),
                None => day.and_hms_opt(0, 0, 0),
            }
            .ok_or_else(|| "时刻超出范围".to_string())?;
            Ok(local_to_utc(naive))
        }
    }
}

/// 解析结果（中间表示，validate / resolve / 提醒展开共用）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spec {
    /// now + offset（offset 可为 0）
    Now(Duration),
    /// 下一整点（@now 的取整糖）
    NextHour,
    /// start + offset
    Start(Duration),
    /// due + offset（None = 截止当时；仅提醒 spec）
    Due(Option<Duration>),
    /// 期间每天 T<clock>（仅提醒 spec）
    Daily((u32, u32)),
    /// 锚点日 + N 天，可选时刻（None = 当日零点）
    Day(i64, Option<(u32, u32)>),
}

/// 基准 × 列（用途）合法性：模板时间列与提醒 spec 共用一套文法，各自收窄。
/// "any" = 解析宽松模式（resolve 用；提醒专属基准在 resolve 中因缺上下文报错）。
fn base_allowed(base: &str, column: &str) -> bool {
    match base {
        "now" | "next_hour" => column != "reminder", // 提醒不收 now 族（随评估漂移）
        "start" => column == "end_at" || column == "reminder" || column == "any",
        "due" | "daily" => column == "reminder" || column == "any",
        _ => false,
    }
}

/// 解析 token（column = 模板列名或 "reminder"）。
pub fn parse_spec(tok: &str, column: &str) -> Result<Spec, String> {
    let body = tok
        .strip_prefix('@')
        .ok_or_else(|| "必须以 @ 开头".to_string())?;
    if body.is_empty() {
        return Err("占位为空".into());
    }

    // 注意顺序：due / daily 必须先于通用 d 基准判定（前缀同样是 d）

    // daily 基准：@dailyT<clock>（提醒期间每日；时刻紧跟 base，不走 offset 切分）
    if let Some(rest) = body.strip_prefix("daily") {
        if !base_allowed("daily", column) {
            return Err("daily 基准仅提醒 spec 可用".into());
        }
        if !rest.starts_with('T') {
            return Err("daily 需要时刻，如 @dailyT09:00".into());
        }
        return Ok(Spec::Daily(parse_clock(rest)?));
    }

    // due 基准：@due / @due-N(m|h|d)（仅提醒）
    if let Some(rest) = body.strip_prefix("due") {
        if !base_allowed("due", column) {
            return Err("due 基准仅提醒 spec 可用".into());
        }
        return Ok(Spec::Due(parse_offset(rest)?));
    }

    // d 基准：d0 / d+N / d-N，可带 T<time>（模板锚点日语义，提醒不适用）
    if let Some(rest) = body.strip_prefix('d') {
        if column == "reminder" {
            return Err("d 基准仅模板锚点日可用；提醒请用 @start / @due / @daily".into());
        }
        let (day_off, rest) = parse_day_off(rest)?;
        let clock = match rest {
            "" => None,
            _ => Some(parse_clock(rest)?),
        };
        return Ok(Spec::Day(day_off, clock));
    }

    // now / next_hour / start 基准 + 可选 offset
    let (base, off_str) = body.split_at(find_offset_start(body));
    if !base_allowed(base, column) {
        return Err(format!("基准 {base} 不能用于 {column}"));
    }
    let off = parse_offset(off_str)?;
    match base {
        "now" => Ok(Spec::Now(off.unwrap_or(Duration::zero()))),
        "next_hour" => {
            if off.is_some() {
                return Err("next_hour 不接受偏移".into());
            }
            Ok(Spec::NextHour)
        }
        "start" => Ok(Spec::Start(off.unwrap_or(Duration::zero()))),
        other => Err(format!(
            "未知基准 \"{other}\"（可用 now / next_hour / start / due / daily / d0 / d+N）"
        )),
    }
}

/// offset 起始位置（第一个 +/- 后跟数字处）。
fn find_offset_start(s: &str) -> usize {
    s.char_indices()
        .find(|(i, c)| (*c == '+' || *c == '-') && s[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

fn parse_day_off(s: &str) -> Result<(i64, &str), String> {
    let end = s
        .find(|c: char| !c.is_ascii_digit() && c != '+' && c != '-')
        .unwrap_or(s.len());
    let (num, rest) = s.split_at(end);
    if num.is_empty() {
        return Err("d 基准天数无效（如 d0 / d+1）".into());
    }
    let n: i64 = num
        .parse()
        .map_err(|_| "d 基准天数无效（如 d0 / d+1）".to_string())?;
    Ok((n, rest))
}

/// `T09:00` / `Tend`（调用方已保证以 T 开头或为空）。
fn parse_clock(s: &str) -> Result<(u32, u32), String> {
    let Some(rest) = s.strip_prefix('T') else {
        return Err(format!("d 基准后只能是 T<时刻>，得到 \"{s}\""));
    };
    if rest == "end" {
        return Ok((23, 59));
    }
    let (h, m) = rest
        .split_once(':')
        .ok_or_else(|| format!("时刻格式应为 HH:MM，得到 \"{rest}\""))?;
    let h: u32 = h.parse().map_err(|_| format!("小时无效：\"{h}\""))?;
    let m: u32 = m.parse().map_err(|_| format!("分钟无效：\"{m}\""))?;
    if h > 23 || m > 59 {
        return Err(format!("时刻超出范围：{h:02}:{m:02}"));
    }
    Ok((h, m))
}

/// `+30m` / `-1h` / 空（= 无偏移）。
fn parse_offset(s: &str) -> Result<Option<Duration>, String> {
    if s.is_empty() {
        return Ok(None);
    }
    let sign = match s.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return Err(format!("偏移应以 +/- 开头，得到 \"{s}\"")),
    };
    let rest = &s[1..];
    let Some(unit) = rest.chars().last() else {
        return Err(format!("偏移无效：\"{s}\""));
    };
    let unit = unit.to_string();
    let num = &rest[..rest.len() - 1];
    let n: i64 = num
        .parse()
        .map_err(|_| format!("偏移数字无效：\"{num}\""))?;
    let minutes = match unit.as_str() {
        "m" => n,
        "h" => n.checked_mul(60).ok_or_else(|| "偏移过大".to_string())?,
        "d" => n.checked_mul(1440).ok_or_else(|| "偏移过大".to_string())?,
        _ => return Err(format!("偏移单位仅 m/h/d，得到 \"{unit}\"")),
    };
    Ok(Some(Duration::minutes(sign * minutes)))
}

fn add_days(day: NaiveDate, n: i64) -> Result<NaiveDate, String> {
    day.checked_add_signed(chrono::Duration::days(n))
        .ok_or_else(|| "日期偏移超出范围".to_string())
}

fn next_hour(now: DateTime<Utc>) -> DateTime<Utc> {
    // 下一整点按本地钟面算才符合直觉
    let local: DateTime<chrono::Local> = now.into();
    let floored = local
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(local)
        + Duration::hours(1);
    floored.with_timezone(&Utc)
}

/// 本地墙钟 → UTC。DST 缺口退化为当前时刻（与 store::local_day_bounds_utc 同策略）。
pub(crate) fn local_to_utc(n: NaiveDateTime) -> DateTime<Utc> {
    chrono::Local
        .from_local_datetime(&n)
        .single()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveTime;

    fn ctx() -> ResolveCtx {
        // 固定锚点：UTC 2026-09-16 10:00（本地 Asia/Shanghai 18:00，由 TZ 环境决定，
        // day 族断言只用「同日/次日 + 钟面」的相对关系，不依赖具体时区偏移）
        ResolveCtx::new(
            Utc.with_ymd_and_hms(2026, 9, 16, 10, 0, 0).unwrap(),
            NaiveDate::from_ymd_opt(2026, 9, 16).unwrap(),
        )
    }

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    #[test]
    fn now_family() {
        let c = ctx();
        assert_eq!(resolve("@now", &c).unwrap(), c.now);
        assert_eq!(resolve("@now+30m", &c).unwrap(), c.now + Duration::minutes(30));
        assert_eq!(resolve("@now-1h", &c).unwrap(), c.now - Duration::hours(1));
        assert!(resolve("@now+90m", &c).is_ok());
        assert!(resolve("@now+x", &c).is_err());
    }

    #[test]
    fn day_family_uses_anchor_day() {
        let c = ctx();
        let got = resolve("@d0T09:00", &c).unwrap();
        let local = got.with_timezone(&chrono::Local);
        assert_eq!(local.date_naive(), c.anchor_day);
        assert_eq!(local.time(), NaiveTime::from_hms_opt(9, 0, 0).unwrap());

        let got = resolve("@d+1Tend", &c).unwrap();
        let local = got.with_timezone(&chrono::Local);
        assert_eq!(local.date_naive(), c.anchor_day + chrono::Duration::days(1));
        assert_eq!(local.time(), NaiveTime::from_hms_opt(23, 59, 0).unwrap());

        // 无 T = 当日零点；d-1 = 前一天
        let got = resolve("@d0", &c).unwrap();
        assert_eq!(got.with_timezone(&chrono::Local).time(), NaiveTime::from_hms_opt(0, 0, 0).unwrap());
        let got = resolve("@d-1T21:00", &c).unwrap();
        assert_eq!(
            got.with_timezone(&chrono::Local).date_naive(),
            c.anchor_day - chrono::Duration::days(1)
        );
    }

    #[test]
    fn start_base_only_for_end() {
        let c = ctx().with_start(Some(utc(2026, 9, 16, 9, 0)));
        assert_eq!(resolve("@start+30m", &c).unwrap(), utc(2026, 9, 16, 9, 30));
        assert!(resolve("@start", &c).unwrap() == utc(2026, 9, 16, 9, 0));
        assert!(resolve("@start+30m", &ctx()).is_err()); // 无 start 上下文
        assert!(validate_for_column("@start+30m", "start_at").is_err());
        assert!(validate_for_column("@start+30m", "end_at").is_ok());
        assert!(validate_for_column("@start+30m", "due_at").is_err());
        assert!(validate_for_column("@start+30m", "reminder").is_ok());
    }

    #[test]
    fn reminder_only_bases() {
        // @due / @daily 仅提醒 spec；模板时间列拒绝
        assert!(validate_for_column("@due-1d", "reminder").is_ok());
        assert!(validate_for_column("@due", "reminder").is_ok());
        assert!(validate_for_column("@dailyT09:00", "reminder").is_ok());
        for col in ["start_at", "end_at", "due_at", "occurred_at"] {
            assert!(validate_for_column("@due-1d", col).is_err(), "@due 用于 {col} 应拒绝");
            assert!(validate_for_column("@dailyT09:00", col).is_err());
        }
        // now 族不作为提醒意图（随评估漂移永不触发）
        assert!(validate_for_column("@now", "reminder").is_err());
        assert!(validate_for_column("@now-30m", "reminder").is_err());
        assert!(validate_for_column("@next_hour", "reminder").is_err());
        // daily 必须带时刻、不吃偏移
        assert!(validate_for_column("@daily", "reminder").is_err());
        assert!(validate_for_column("@daily+1h", "reminder").is_err());
        assert!(validate_for_column("@dailyT25:00", "reminder").is_err());
        // d 单位偏移
        assert!(validate_for_column("@start-1d", "reminder").is_ok());
        assert!(validate_for_column("@due-3d", "reminder").is_ok());
        assert!(validate_for_column("@now-1d", "any").is_ok());
    }

    #[test]
    fn reminder_only_bases_reject_one_shot_resolve() {
        // @due / @daily 是提醒专属，不做一次性解析（展开在 reminder.rs）
        let c = ctx();
        assert!(resolve("@due", &c).is_err());
        assert!(resolve("@due-1d", &c).is_err());
        assert!(resolve("@dailyT09:00", &c).is_err());
        // 提醒列收窄：now 族与 d 基准均拒绝
        assert!(validate_for_column("@d0T09:00", "reminder").is_err());
    }

    #[test]
    fn validate_examples_and_rejects() {
        for (tok, _) in OFFICIAL_EXAMPLES {
            assert!(validate_for_column(tok, "any").is_ok(), "{tok} 应合法");
        }
        for bad in ["@", "@foo", "@d", "@dT09:00", "@d0T25:00", "@now+30x", "@next_hour+1h", "@daily", "@dailyT09:00x"] {
            assert!(validate_for_column(bad, "any").is_err(), "{bad} 应非法");
        }
    }

    #[test]
    fn next_hour_is_future_full_hour() {
        let c = ctx();
        let got = resolve("@next_hour", &c).unwrap();
        assert!(got > c.now);
        assert_eq!(got.with_timezone(&chrono::Local).minute(), 0);
        assert_eq!(got.with_timezone(&chrono::Local).second(), 0);
    }
}

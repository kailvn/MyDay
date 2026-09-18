//! 视图模型与过滤器引擎（FILTER-SPEC v1）。
//!
//! 四层概念模型：字段引用 → 条件（field + cmp + value）→ 条件 AST（AND/OR 组，
//! 两层封顶）→ 数据集（类型 + AST + 排序 + 分组 + limit）→ 视图 / 挂件。
//! 面板展示什么由「视图 = 数据集 + 布局」声明，内置视图是 seed 进 `view_defs`
//! 表的真实行，不是代码特判分支；统计挂件 = 数据集 × 聚合 × 派生指标 × 渲染器
//! 三段纯函数管线，渲染器只吃统一结果表（buckets / points）。
//!
//! 求值顺序（§10，定死）：
//! 1. 编译：keyword → OR 子树并入 AST；相对日期 / window 按查询时本地日展开；
//! 2. 预筛下推：AST 中「祖先全 and」的条件松化为 [`ListFilter`] 超集
//!    （OR 组整组下推取分支并集，跨列 OR / 含不可下推子项放弃该子树）；
//! 3. hydrate 一次，预筛层绝不施加视图 limit；
//! 4. AST 内存求值 = 最终真相；
//! 5. 排序 → limit 最后；
//! 6. 结果信封带 `evaluated_at` + `tz`（跨午夜 / 时区变更由前端整体重查）。

use std::collections::{BTreeMap, HashSet};

use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{MyDayError, Result};
use crate::model::{FieldDef, Item, ItemStatus, ItemType};
use crate::store::{ListFilter, StatsSummary, Store, FILE_LINKS_KEY};

// ==========================================================================
// 内置视图 id（种子进表，前端按 id 定位）
// ==========================================================================

pub const VIEW_LOGS_TIMELINE: &str = "view_builtin_logs_timeline";
pub const VIEW_TASKS_TODAY: &str = "view_builtin_tasks_today";
pub const VIEW_TASKS_UPCOMING: &str = "view_builtin_tasks_upcoming";
pub const VIEW_TASKS_ALL: &str = "view_builtin_tasks_all";
pub const VIEW_TASKS_DONE: &str = "view_builtin_tasks_done";
pub const VIEW_SEARCH_ALL: &str = "view_builtin_search_all";
pub const VIEW_STATS_HEATMAP: &str = "view_builtin_stats_heatmap";
pub const VIEW_STATS_STREAKS: &str = "view_builtin_stats_streaks";
pub const VIEW_STATS_SERIES: &str = "view_builtin_stats_series";

/// 面板（视图归属）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Panel {
    Logs,
    Tasks,
    Search,
    Stats,
}

impl Panel {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "logs" => Some(Self::Logs),
            "tasks" => Some(Self::Tasks),
            "search" => Some(Self::Search),
            "stats" => Some(Self::Stats),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Tasks => "tasks",
            Self::Search => "search",
            Self::Stats => "stats",
        }
    }
}

// ==========================================================================
// 条件 AST（§3 / §4）
// ==========================================================================

/// 组逻辑：AND / OR。根必须是组；嵌套 ≤ 2 层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Logic {
    And,
    Or,
}

/// 比较运算符（§4.3 矩阵；可用集随字段类型收窄）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cmp {
    Eq,
    Neq,
    Contains,
    NotContains,
    Gt,
    Gte,
    Lt,
    Lte,
    Between,
    Any,
    All,
    HasNone,
    IsTrue,
    IsFalse,
    On,
    Before,
    After,
    Within,
    Empty,
    NotEmpty,
}

impl Cmp {
    /// 稳定字符串形式（前端 chips / 编辑器共用词表）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::Neq => "neq",
            Self::Contains => "contains",
            Self::NotContains => "not_contains",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Between => "between",
            Self::Any => "any",
            Self::All => "all",
            Self::HasNone => "none",
            Self::IsTrue => "is_true",
            Self::IsFalse => "is_false",
            Self::On => "on",
            Self::Before => "before",
            Self::After => "after",
            Self::Within => "within",
            Self::Empty => "empty",
            Self::NotEmpty => "not_empty",
        }
    }
}

/// 相对日（点值）：查询时按本地日求值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RelDay {
    Today,
    Tomorrow,
    Yesterday,
}

/// 相对区间（范围值）：this_week = 周一 00:00 ~ 周日 24:00；last_days:N 含今天滚动。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelRange {
    ThisWeek,
    ThisMonth,
    LastDays(i64),
}

/// 日期值（§4.2）：点值 `{day}` / `{rel: today|tomorrow|yesterday}`，
/// 范围值 `{rel: this_week|this_month|last_days:N}`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateValue {
    Day(NaiveDate),
    RelPoint(RelDay),
    RelRange(RelRange),
}

impl Serialize for DateValue {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Day(d) => json!({ "day": d.format("%Y-%m-%d").to_string() }).serialize(s),
            Self::RelPoint(r) => json!({ "rel": r }).serialize(s),
            Self::RelRange(r) => {
                let v = match r {
                    RelRange::ThisWeek => json!({ "rel": "this_week" }),
                    RelRange::ThisMonth => json!({ "rel": "this_month" }),
                    RelRange::LastDays(n) => json!({ "rel": format!("last_days:{n}") }),
                };
                v.serialize(s)
            }
        }
    }
}

impl<'de> Deserialize<'de> for DateValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        parse_date_value(&v).map_err(serde::de::Error::custom)
    }
}

/// 日期值解析（点值 / 范围值两型；N ≥ 1 否则参数错误）。
pub fn parse_date_value(v: &Value) -> std::result::Result<DateValue, String> {
    if let Some(day) = v.get("day").and_then(|x| x.as_str()) {
        let d = NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .map_err(|_| format!("\"{day}\" 不是合法日期（YYYY-MM-DD）"))?;
        return Ok(DateValue::Day(d));
    }
    let Some(rel) = v.get("rel").and_then(|x| x.as_str()) else {
        return Err("日期值必须是 {\"day\":\"YYYY-MM-DD\"} 或 {\"rel\":\"…\"}".into());
    };
    match rel {
        "today" => Ok(DateValue::RelPoint(RelDay::Today)),
        "tomorrow" => Ok(DateValue::RelPoint(RelDay::Tomorrow)),
        "yesterday" => Ok(DateValue::RelPoint(RelDay::Yesterday)),
        "this_week" => Ok(DateValue::RelRange(RelRange::ThisWeek)),
        "this_month" => Ok(DateValue::RelRange(RelRange::ThisMonth)),
        other => {
            let Some(n) = other.strip_prefix("last_days:") else {
                return Err(format!(
                    "未知相对值 \"{rel}\"（可用 today/tomorrow/yesterday/this_week/this_month/last_days:N）"
                ));
            };
            let n: i64 = n.parse().map_err(|_| "last_days:N 的 N 必须是整数".to_string())?;
            if n < 1 {
                return Err("last_days:N 的 N 必须 ≥ 1".into());
            }
            Ok(DateValue::RelRange(RelRange::LastDays(n)))
        }
    }
}

/// between 的双点值：`{from: {day|rel}, to: {day|rel}}`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatePair {
    pub from: DateValue,
    pub to: DateValue,
}

impl Serialize for DatePair {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        json!({ "from": self.from, "to": self.to }).serialize(s)
    }
}

impl<'de> Deserialize<'de> for DatePair {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let from =
            parse_date_value(v.get("from").unwrap_or(&Value::Null)).map_err(serde::de::Error::custom)?;
        let to =
            parse_date_value(v.get("to").unwrap_or(&Value::Null)).map_err(serde::de::Error::custom)?;
        Ok(Self { from, to })
    }
}

/// number between 的双数值：`{from: n, to: n}`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NumPair {
    pub from: f64,
    pub to: f64,
}

/// 条件值（标量 / 列表 / 日期点与区间）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterValue {
    Bool(bool),
    Number(f64),
    Text(String),
    StrList(Vec<String>),
    Date(DateValue),
    DatePair(DatePair),
    NumPair(NumPair),
}

/// 条件：字段 + 运算符 + 值（empty / not_empty / is_true / is_false 无值）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub cmp: Cmp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FilterValue>,
}

/// AST 节点：组（op + children）或条件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterNode {
    Group { op: Logic, children: Vec<FilterNode> },
    Cond(Condition),
}

/// 恒真组（空 children）。
pub fn true_group() -> FilterNode {
    FilterNode::Group { op: Logic::And, children: Vec::new() }
}

impl FilterNode {
    /// 组节点的 children；条件节点返回空切片。
    pub fn children(&self) -> &[FilterNode] {
        match self {
            FilterNode::Group { children, .. } => children,
            FilterNode::Cond(_) => &[],
        }
    }
}

fn invalid(msg: impl Into<String>) -> MyDayError {
    MyDayError::Invalid(msg.into())
}

// --------------------------------------------------------------------------
// 字段引用与运算符矩阵（§4）
// --------------------------------------------------------------------------

/// 条件可引用的内置列（§4.1；v1 不暴露 col:id / idempotency_key）。
pub const BUILTIN_COLUMNS: &[&str] = &[
    "col:type",
    "col:status",
    "col:title",
    "col:note",
    "col:tags",
    "col:template_id",
    "col:recurrence",
    "col:start_at",
    "col:end_at",
    "col:due_at",
    "col:occurred_at",
    "col:created_at",
    "col:updated_at",
    "col:completed_at",
    "col:anchor",
];

/// 字段角色（决定可用运算符集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldRole {
    Text,
    Number,
    Select,
    Multi,
    Bool,
    Date,
    TypeCol,
    StatusCol,
    TemplateCol,
    RecurrenceCol,
}

fn col_role(col: &str) -> Option<FieldRole> {
    Some(match col {
        "col:type" => FieldRole::TypeCol,
        "col:status" => FieldRole::StatusCol,
        "col:title" | "col:note" => FieldRole::Text,
        "col:tags" => FieldRole::Multi,
        "col:template_id" => FieldRole::TemplateCol,
        "col:recurrence" => FieldRole::RecurrenceCol,
        "col:start_at" | "col:end_at" | "col:due_at" | "col:occurred_at" | "col:created_at"
        | "col:updated_at" | "col:completed_at" | "col:anchor" => FieldRole::Date,
        _ => return None,
    })
}

/// 自定义字段 kind → 角色（date 字段按日期条件求值，url / text 同文本）。
fn kind_role(kind: crate::model::FieldKind) -> FieldRole {
    use crate::model::FieldKind::*;
    match kind {
        Number => FieldRole::Number,
        Select => FieldRole::Select,
        MultiSelect => FieldRole::Multi,
        Bool => FieldRole::Bool,
        Date => FieldRole::Date,
        Text | Url => FieldRole::Text,
    }
}

/// 运算符是否在该角色可用（§4.3 矩阵）。
fn cmp_allowed(role: FieldRole, cmp: Cmp) -> bool {
    use Cmp::*;
    match role {
        FieldRole::Text => matches!(cmp, Eq | Neq | Contains | NotContains | Empty | NotEmpty),
        FieldRole::Number => {
            matches!(cmp, Eq | Neq | Gt | Gte | Lt | Lte | Between | Empty | NotEmpty)
        }
        FieldRole::Select => matches!(cmp, Eq | Neq | Empty | NotEmpty),
        // 多值字段在矩阵外补 contains / not_contains：关键词编译在标签上
        // 需要「子串命中任一标签」（与 search 的 LIKE 口径一致）
        FieldRole::Multi => {
            matches!(cmp, Any | All | HasNone | Contains | NotContains | Empty | NotEmpty)
        }
        FieldRole::Bool => matches!(cmp, IsTrue | IsFalse | Empty),
        FieldRole::Date => {
            matches!(cmp, On | Before | After | Between | Within | Empty | NotEmpty)
        }
        FieldRole::TypeCol | FieldRole::StatusCol => matches!(cmp, Eq),
        FieldRole::TemplateCol => matches!(cmp, Eq | Neq | Empty | NotEmpty),
        FieldRole::RecurrenceCol => matches!(cmp, Empty | NotEmpty),
    }
}

/// 值型与运算符配对封闭（§4.2）：非法组合 = 参数错误。
fn check_value_pairing(role: FieldRole, cmp: Cmp, value: Option<&FilterValue>) -> Result<()> {
    let needs = |req: &str| -> Result<&FilterValue> {
        value.ok_or_else(|| invalid(format!("{:?} 需要值（{}）", cmp, req)))
    };
    match cmp {
        Cmp::Empty | Cmp::NotEmpty | Cmp::IsTrue | Cmp::IsFalse => {
            if value.is_some() {
                return Err(invalid(format!("{:?} 不接受值", cmp)));
            }
        }
        Cmp::On | Cmp::Before | Cmp::After => {
            let v = needs("点值 {day}|{rel:today族}")?;
            match v {
                FilterValue::Date(DateValue::Day(_) | DateValue::RelPoint(_)) => {}
                _ => {
                    return Err(invalid(
                        "on/before/after 只接点值（{day} 或 {rel:today/tomorrow/yesterday}）",
                    ))
                }
            }
        }
        Cmp::Within => {
            let v = needs("范围值 {rel:this_week|this_month|last_days:N}")?;
            match v {
                FilterValue::Date(DateValue::RelRange(_)) => {}
                _ => return Err(invalid("within 只接范围值（this_week / this_month / last_days:N）")),
            }
        }
        Cmp::Between => {
            let v = needs("双点值 / 双数值")?;
            match (role, v) {
                (FieldRole::Number, FilterValue::NumPair(_)) => {}
                (
                    FieldRole::Date,
                    FilterValue::DatePair(DatePair {
                        from: DateValue::Day(_) | DateValue::RelPoint(_),
                        to: DateValue::Day(_) | DateValue::RelPoint(_),
                    }),
                ) => {}
                _ => return Err(invalid("between：日期接双点值 {from,to}，数字接 {from,to} 数值")),
            }
        }
        Cmp::Any | Cmp::All | Cmp::HasNone => {
            let v = needs("字符串数组")?;
            if !matches!(v, FilterValue::StrList(_)) {
                return Err(invalid("any/all/none 接字符串数组"));
            }
        }
        Cmp::Gt | Cmp::Gte | Cmp::Lt | Cmp::Lte => {
            let v = needs("数字")?;
            if !matches!(v, FilterValue::Number(_)) {
                return Err(invalid("数字比较接数值"));
            }
        }
        Cmp::Eq | Cmp::Neq => {
            let v = needs("标量值")?;
            let ok = match role {
                FieldRole::Number => matches!(v, FilterValue::Number(_)),
                FieldRole::Bool => matches!(v, FilterValue::Bool(_)),
                _ => matches!(v, FilterValue::Text(_) | FilterValue::Number(_)),
            };
            if !ok {
                return Err(invalid("eq/neq 值类型与字段不符"));
            }
        }
        Cmp::Contains | Cmp::NotContains => {
            let v = needs("文本")?;
            if !matches!(v, FilterValue::Text(_)) {
                return Err(invalid("contains 接文本"));
            }
        }
    }
    Ok(())
}

// --------------------------------------------------------------------------
// 校验（保存时；引擎对存量配置只按存储值求值，不改写）
// --------------------------------------------------------------------------

/// 校验 AST：根为组、深度 ≤ 2、运算符矩阵与值型配对。
/// `fields` 为活跃字段定义，供自定义字段角色判定。
pub fn validate_filter(node: &FilterNode, fields: &[FieldDef]) -> Result<()> {
    fn walk(node: &FilterNode, depth: u32, fields: &[FieldDef]) -> Result<()> {
        match node {
            FilterNode::Group { op: _, children } => {
                if depth >= 2 {
                    return Err(invalid("条件组嵌套超过两层"));
                }
                for c in children {
                    if matches!(c, FilterNode::Group { .. }) && depth + 1 >= 2 {
                        return Err(invalid("子组内只能包含条件，不能再嵌组"));
                    }
                    walk(c, depth + 1, fields)?;
                }
                Ok(())
            }
            FilterNode::Cond(cond) => validate_condition(cond, fields),
        }
    }
    match node {
        FilterNode::Group { .. } => walk(node, 0, fields),
        FilterNode::Cond(_) => Err(invalid("过滤器根必须是条件组（and/or）")),
    }
}

fn validate_condition(cond: &Condition, fields: &[FieldDef]) -> Result<()> {
    let role = if let Some(r) = col_role(&cond.field) {
        r
    } else {
        let def = fields
            .iter()
            .find(|f| f.id == cond.field)
            .ok_or_else(|| {
                invalid(format!("条件的字段 \"{}\" 不是内置列也不是活跃字段 id", cond.field))
            })?;
        kind_role(def.kind)
    };
    if !cmp_allowed(role, cond.cmp) {
        return Err(invalid(format!("字段 \"{}\" 不支持运算符 {:?}", cond.field, cond.cmp)));
    }
    check_value_pairing(role, cond.cmp, cond.value.as_ref())
}

// ==========================================================================
// 数据集 / 视图配置 / 挂件配置（§5 / §6 / §8）
// ==========================================================================

/// 数据集类型（含 all）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    All,
    Event,
    Task,
    Log,
}

impl Source {
    pub fn matches(self, t: ItemType) -> bool {
        match self {
            Self::All => true,
            Self::Event => t == ItemType::Event,
            Self::Task => t == ItemType::Task,
            Self::Log => t == ItemType::Log,
        }
    }
}

/// 排序方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDir {
    Asc,
    Desc,
}

/// 多级排序一项。NULL / 缺值恒排该级末尾（与方向无关）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortSpec {
    pub field: String,
    #[serde(default = "default_sort_dir")]
    pub dir: SortDir,
}

fn default_sort_dir() -> SortDir {
    SortDir::Asc
}

/// 分组：v1 仅时间桶（day）。NULL 单独成桶恒排末尾。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupSpec {
    #[serde(default = "default_bucket")]
    pub bucket: String,
    pub field: String,
}

fn default_bucket() -> String {
    "day".into()
}

/// 面板视图数据集。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dataset {
    pub item_type: Source,
    #[serde(default = "true_group")]
    pub filter: FilterNode,
    #[serde(default)]
    pub sort: Vec<SortSpec>,
    #[serde(default)]
    pub group: Option<GroupSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
}

/// 面板视图配置：数据集 + 布局（layout / visible 为预留枚举位）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewConfig {
    pub dataset: Dataset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Vec<String>>,
}

// ---- 挂件（§8） -----------------------------------------------------------

/// 挂件窗口：滚动窗口（以今天为终点的近 N 天）或全历史。缺省 365。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Window {
    Days { days: i64 },
    All { all: bool },
}

/// 聚合分组槽位：时间桶或字段分组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "lowercase")]
pub enum GroupClause {
    Time { bucket: String, time_field: String },
    Field { field: String },
}

/// 聚合指标槽位。fn ≠ count 时 field 必填（number 字段）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    #[serde(rename = "fn")]
    pub fun: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

/// 聚合（group / metric 分槽，字段角色不再一词多义）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Agg {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupClause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metric: Option<Metric>,
}

/// 派生指标声明：streak（+ 目标）等。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerivedSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal: Option<Goal>,
}

/// streak 目标：daily:N（连续达标日）或 weekly:N（连续达标周，本地周一起）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Goal {
    Daily { daily: i64 },
    Weekly { weekly: i64 },
}

/// 渲染器。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Render {
    Bar,
    Line,
    Pie,
    Heatmap,
    Card,
}

impl Render {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Line => "line",
            Self::Pie => "pie",
            Self::Heatmap => "heatmap",
            Self::Card => "card",
        }
    }
}

/// 渲染选项。unit 缺省取 metric.field 字段定义的 options.unit，此处仅覆盖。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetOptions {
    #[serde(default)]
    pub presence: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default = "default_top_n")]
    pub top_n: i64,
}

fn default_top_n() -> i64 {
    8
}

impl Default for WidgetOptions {
    fn default() -> Self {
        Self { presence: false, unit: None, top_n: default_top_n() }
    }
}

/// 挂件数据集 = 简化数据集：item_type + filter（无 sort / group / limit；
/// 范围用 window、形态用 agg 表达，§2 图注）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetDataset {
    pub item_type: Source,
    #[serde(default = "true_group")]
    pub filter: FilterNode,
}

/// 挂件配置（三段管线声明）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetConfig {
    pub kind: String,
    pub dataset: WidgetDataset,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<Window>,
    #[serde(default)]
    pub agg: Option<Agg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived: Option<DerivedSpec>,
    pub render: Render,
    #[serde(default)]
    pub options: WidgetOptions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// 面板视图配置校验。
pub fn validate_view_config(cfg: &Value, fields: &[FieldDef]) -> Result<ViewConfig> {
    let c: ViewConfig =
        serde_json::from_value(cfg.clone()).map_err(|e| invalid(format!("视图配置无效: {e}")))?;
    validate_filter(&c.dataset.filter, fields)?;
    if let Some(g) = &c.dataset.group {
        if g.bucket != "day" {
            return Err(invalid("v1 分组仅支持 day 时间桶"));
        }
        if col_role(&g.field) != Some(FieldRole::Date) {
            return Err(invalid("分组字段必须是日期列（v1 时间桶分组）"));
        }
    }
    for s in &c.dataset.sort {
        validate_sort_field(&s.field, fields)?;
    }
    Ok(c)
}

fn validate_sort_field(field: &str, fields: &[FieldDef]) -> Result<()> {
    if BUILTIN_COLUMNS.contains(&field) {
        return Ok(());
    }
    if fields.iter().any(|f| f.id == field) {
        return Ok(());
    }
    Err(invalid(format!("排序字段 \"{field}\" 不是内置列也不是活跃字段 id")))
}

/// 挂件配置校验（dataset 禁 limit；values 限 log 且禁 group；槽位规则 §8）。
pub fn validate_widget_config(cfg: &Value, fields: &[FieldDef]) -> Result<WidgetConfig> {
    if cfg.get("dataset").map(|d| d.get("limit").is_some()) == Some(true) {
        return Err(invalid(
            "挂件 dataset 禁用 limit：行截断会静默掏空桶，范围只用 window 表达",
        ));
    }
    let c: WidgetConfig =
        serde_json::from_value(cfg.clone()).map_err(|e| invalid(format!("挂件配置无效: {e}")))?;
    if c.kind != "widget" {
        return Err(invalid(format!("未知挂件 kind: \"{}\"（应为 widget）", c.kind)));
    }
    validate_filter(&c.dataset.filter, fields)?;
    if let Some(Window::Days { days }) = &c.window {
        if *days < 1 {
            return Err(invalid("window.days 必须 ≥ 1"));
        }
    }
    let agg = c.agg.clone().unwrap_or(Agg { group: None, metric: None });
    let metric_fn = agg.metric.as_ref().map(|m| m.fun.as_str()).unwrap_or("count");
    let metric_field = agg.metric.as_ref().and_then(|m| m.field.clone());
    if !matches!(metric_fn, "count" | "sum" | "avg" | "min" | "max" | "values") {
        return Err(invalid(format!("未知聚合 fn: {metric_fn}")));
    }
    match &agg.group {
        Some(GroupClause::Time { bucket, time_field }) => {
            if !matches!(bucket.as_str(), "day" | "week" | "month") {
                return Err(invalid("时间桶只支持 day / week / month"));
            }
            if col_role(time_field) != Some(FieldRole::Date) {
                return Err(invalid("time_field 必须是日期列"));
            }
        }
        Some(GroupClause::Field { field }) => {
            let allowed = match col_role(field) {
                Some(FieldRole::Multi) | Some(FieldRole::TemplateCol) => true,
                _ => fields.iter().any(|f| {
                    f.id == *field
                        && matches!(
                            f.kind,
                            crate::model::FieldKind::Select | crate::model::FieldKind::MultiSelect
                        )
                }),
            };
            if !allowed {
                return Err(invalid(
                    "字段分组只允许 select / multiselect / col:template_id / col:tags",
                ));
            }
        }
        None => {}
    }
    match metric_fn {
        "count" => {}
        "values" => {
            if c.dataset.item_type != Source::Log {
                return Err(invalid("metric.fn=values 仅支持 log 数据源"));
            }
            if agg.group.is_some() {
                return Err(invalid(
                    "values 是单数据集原始点列，group 必须为 null；多线请用 series 组（每线自带 filter）",
                ));
            }
            require_number_field(metric_field.as_deref(), fields, "values")?;
        }
        other => require_number_field(metric_field.as_deref(), fields, other)?,
    }
    if let Some(d) = &c.derived {
        if let Some(k) = &d.kind {
            if k != "streak" {
                return Err(invalid(format!("未知派生指标: {k}（可用 streak）")));
            }
        }
        if let Some(g) = &d.goal {
            match g {
                Goal::Daily { daily } if *daily >= 1 => {}
                Goal::Weekly { weekly } if *weekly >= 1 => {}
                _ => return Err(invalid("goal 的 daily / weekly 必须 ≥ 1")),
            }
        }
    }
    if c.options.top_n < 1 {
        return Err(invalid("options.top_n 必须 ≥ 1"));
    }
    Ok(c)
}

fn require_number_field(field: Option<&str>, fields: &[FieldDef], what: &str) -> Result<()> {
    let Some(f) = field else {
        return Err(invalid(format!("聚合 {what} 需要 metric.field（number 字段）")));
    };
    let def = fields
        .iter()
        .find(|d| d.id == f)
        .ok_or_else(|| invalid(format!("metric.field \"{f}\" 不是活跃字段 id")))?;
    if def.kind != crate::model::FieldKind::Number {
        return Err(invalid(format!("聚合 {what} 的 metric.field 必须是 number 字段")));
    }
    Ok(())
}

// ==========================================================================
// view_defs 行
// ==========================================================================

/// 视图 / 挂件行。生效配置 = config_user ?? config。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ViewDef {
    pub id: String,
    pub name: String,
    pub panel: Panel,
    /// seed（内置）或用户视图的生效配置（JSON）
    pub config: Value,
    /// 内置视图的用户定制（None = 未定制）
    pub config_user: Option<Value>,
    pub builtin: bool,
    pub sort: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ViewDef {
    pub fn effective_config(&self) -> &Value {
        self.config_user.as_ref().unwrap_or(&self.config)
    }
    /// 内置视图且 config_user 非空 = 已定制。
    pub fn customized(&self) -> bool {
        self.builtin && self.config_user.is_some()
    }
}

pub(crate) const VIEW_DEFS_SQL: &str = "
CREATE TABLE IF NOT EXISTS view_defs (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  panel       TEXT NOT NULL,
  config      TEXT NOT NULL,
  config_user TEXT,
  builtin     INTEGER NOT NULL DEFAULT 0,
  sort        INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
";

/// 内置面板视图种子（§9）。upsert 只覆盖 name / panel / config 列，config_user
/// 永不触碰——用户定制不因升级丢失（§7）。
/// 统计页预置容器不在此列：它们是开库时替用户建好的**普通容器**（一次性播种，
/// 见 [`Store::seed_stats_presets`]），之后与用户容器同权——可编辑可真删，
/// 「恢复默认统计页」= 完全重置铺回预设。
pub(crate) fn seed_view_defs(conn: &Connection) -> Result<()> {
    let now = crate::store::dt(Utc::now());
    let seeds: &[(&str, &str, Panel, Value)] = &[
        (
            VIEW_LOGS_TIMELINE,
            "时间线",
            Panel::Logs,
            json!({
                "dataset": {
                    "item_type": "log",
                    "filter": { "op": "and", "children": [] },
                    "sort": [{ "field": "col:occurred_at", "dir": "desc" }],
                    "group": { "bucket": "day", "field": "col:occurred_at" },
                    "limit": 200
                },
                "layout": "timeline"
            }),
        ),
        (
            VIEW_TASKS_TODAY,
            "今天",
            Panel::Tasks,
            json!({
                "dataset": {
                    "item_type": "task",
                    "filter": { "op": "and", "children": [
                        { "field": "col:status", "cmp": "eq", "value": "todo" },
                        { "op": "or", "children": [
                            { "field": "col:anchor", "cmp": "before", "value": { "rel": "tomorrow" } },
                            { "field": "col:anchor", "cmp": "empty" }
                        ] }
                    ] },
                    "sort": [{ "field": "col:anchor", "dir": "asc" }],
                    "limit": 500
                },
                "layout": "list"
            }),
        ),
        (
            VIEW_TASKS_UPCOMING,
            "即将到期",
            Panel::Tasks,
            json!({
                "dataset": {
                    "item_type": "task",
                    "filter": { "op": "and", "children": [
                        { "field": "col:status", "cmp": "eq", "value": "todo" },
                        { "field": "col:anchor", "cmp": "after", "value": { "rel": "today" } }
                    ] },
                    "sort": [{ "field": "col:anchor", "dir": "asc" }],
                    "limit": 100
                },
                "layout": "list"
            }),
        ),
        (
            VIEW_TASKS_ALL,
            "全部",
            Panel::Tasks,
            json!({
                "dataset": {
                    "item_type": "task",
                    "filter": { "op": "and", "children": [
                        { "field": "col:status", "cmp": "eq", "value": "todo" }
                    ] },
                    "sort": [{ "field": "col:anchor", "dir": "asc" }],
                    "limit": 500
                },
                "layout": "list"
            }),
        ),
        (
            VIEW_TASKS_DONE,
            "已完成",
            Panel::Tasks,
            json!({
                "dataset": {
                    "item_type": "task",
                    "filter": { "op": "and", "children": [
                        { "field": "col:status", "cmp": "eq", "value": "done" }
                    ] },
                    "sort": [{ "field": "col:completed_at", "dir": "desc" }],
                    "limit": 200
                },
                "layout": "list"
            }),
        ),
        (
            VIEW_SEARCH_ALL,
            "全部类型",
            Panel::Search,
            json!({
                "dataset": {
                    "item_type": "all",
                    "filter": { "op": "and", "children": [] },
                    "sort": [{ "field": "col:updated_at", "dir": "desc" }],
                    "limit": 50
                },
                "layout": "list"
            }),
        ),
    ];
    for (i, (id, name, panel, config)) in seeds.iter().enumerate() {
        conn.execute(
            "INSERT INTO view_defs (id, name, panel, config, config_user, builtin, sort, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, NULL, 1, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, panel = excluded.panel,
                                           config = excluded.config, updated_at = excluded.updated_at",
            params![id, name, panel.as_str(), config.to_string(), i as i64, now],
        )?;
    }
    Ok(())
}

fn view_def_mapper(row: &rusqlite::Row) -> rusqlite::Result<ViewDef> {
    let panel: String = row.get("panel")?;
    Ok(ViewDef {
        id: row.get("id")?,
        name: row.get("name")?,
        panel: Panel::parse(&panel).unwrap_or(Panel::Search),
        config: serde_json::from_str(&row.get::<_, String>("config")?).unwrap_or_else(|_| json!({})),
        config_user: row
            .get::<_, Option<String>>("config_user")?
            .and_then(|s| serde_json::from_str(&s).ok()),
        builtin: row.get::<_, i64>("builtin")? != 0,
        sort: row.get("sort")?,
        created_at: row
            .get::<_, Option<String>>("created_at")?
            .as_deref()
            .and_then(crate::store::parse_dt)
            .unwrap_or_else(Utc::now),
        updated_at: row
            .get::<_, Option<String>>("updated_at")?
            .as_deref()
            .and_then(crate::store::parse_dt)
            .unwrap_or_else(Utc::now),
    })
}

// ==========================================================================
// 求值上下文与结果信封
// ==========================================================================

/// 查询时锚点（本地日 + 当前时刻）。相对日期 / window 全部按此展开。
#[derive(Debug, Clone, Copy)]
pub struct EvalCtx {
    pub now: DateTime<Utc>,
    pub today: NaiveDate,
}

impl EvalCtx {
    pub fn now() -> Self {
        Self { now: Utc::now(), today: chrono::Local::now().date_naive() }
    }
    /// 本地日 d 的 [日始, 次日始)（UTC 时刻对）。
    fn day_bounds(&self, d: NaiveDate) -> (DateTime<Utc>, DateTime<Utc>) {
        let start = d.and_hms_opt(0, 0, 0).map(crate::tpltime::local_to_utc).unwrap_or(self.now);
        let end = (d + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .map(crate::tpltime::local_to_utc)
            .unwrap_or(self.now);
        (start, end)
    }
    /// 点值 → 具体本地日。
    fn point_day(&self, v: &DateValue) -> Result<NaiveDate> {
        match v {
            DateValue::Day(d) => Ok(*d),
            DateValue::RelPoint(r) => Ok(match r {
                RelDay::Today => self.today,
                RelDay::Tomorrow => self.today + Duration::days(1),
                RelDay::Yesterday => self.today - Duration::days(1),
            }),
            DateValue::RelRange(_) => {
                Err(invalid("点值位置不能使用区间（this_week / this_month / last_days:N）"))
            }
        }
    }
    /// 值 → [日始, 日末]（闭区间）。范围值展开为区间（本地时区、查询时求值）。
    fn value_bounds(&self, v: &DateValue) -> Result<(DateTime<Utc>, DateTime<Utc>)> {
        match v {
            DateValue::RelRange(RelRange::ThisWeek) => {
                let mon =
                    self.today - Duration::days(self.today.weekday().num_days_from_monday() as i64);
                let (s, _) = self.day_bounds(mon);
                let (_, e) = self.day_bounds(mon + Duration::days(6));
                Ok((s, e))
            }
            DateValue::RelRange(RelRange::ThisMonth) => {
                let first =
                    NaiveDate::from_ymd_opt(self.today.year(), self.today.month(), 1).unwrap();
                let nm = if self.today.month() == 12 {
                    NaiveDate::from_ymd_opt(self.today.year() + 1, 1, 1).unwrap()
                } else {
                    NaiveDate::from_ymd_opt(self.today.year(), self.today.month() + 1, 1).unwrap()
                };
                let (s, _) = self.day_bounds(first);
                let (_, e) = self.day_bounds(nm - Duration::days(1));
                Ok((s, e))
            }
            DateValue::RelRange(RelRange::LastDays(n)) => {
                if *n < 1 {
                    return Err(invalid("last_days:N 的 N 必须 ≥ 1"));
                }
                let (s, _) = self.day_bounds(self.today - Duration::days(n - 1));
                let (_, e) = self.day_bounds(self.today);
                Ok((s, e))
            }
            other => {
                let d = self.point_day(other)?;
                Ok(self.day_bounds(d))
            }
        }
    }
    /// 滚动窗口（window.days = N）→ [今天−N+1 日始, 今天末]；None = 全历史。
    fn window_bounds(
        &self,
        window: Option<Window>,
        default_days: i64,
    ) -> Result<Option<(DateTime<Utc>, DateTime<Utc>)>> {
        let days = match window {
            None => Some(default_days),
            Some(Window::Days { days }) => Some(days),
            Some(Window::All { all }) if all => None,
            Some(Window::All { .. }) => Some(default_days),
        };
        let Some(n) = days else { return Ok(None) };
        if n < 1 {
            return Err(invalid("window.days 必须 ≥ 1"));
        }
        let (s, _) = self.day_bounds(self.today - Duration::days(n - 1));
        let (_, e) = self.day_bounds(self.today);
        Ok(Some((s, e)))
    }
}

/// 本地时区名：TZ 环境变量 → /etc/localtime 链接目标 → UTC 偏移。
pub fn local_tz_name() -> String {
    if let Ok(tz) = std::env::var("TZ") {
        if !tz.trim().is_empty() {
            return tz.trim().to_string();
        }
    }
    #[cfg(unix)]
    if let Ok(path) = std::fs::read_link("/etc/localtime") {
        let s = path.to_string_lossy();
        if let Some(name) = s.split("/zoneinfo/").nth(1) {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    let off = chrono::Local::now().offset().local_minus_utc();
    let (h, m) = (off / 3600, (off % 3600) / 60);
    format!("UTC{h:+03}:{m:02}")
}

/// 分组后的条目组（组键 = 本地日；key None = 未安排 / 空）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ItemGroup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub label: String,
    pub items: Vec<Item>,
}

/// 桶（聚合结果表）。key = 本地日 / ISO 周 / 月 / 字段值。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Bucket {
    pub key: String,
    pub value: f64,
}

/// 点列（values 结果表）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct Point {
    /// RFC3339（发生时刻）
    pub t: String,
    pub v: f64,
}

/// streak 派生结果（current / longest 全历史判定；recent 随窗口）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct StreakOut {
    pub current: i64,
    pub longest: i64,
    pub recent: i64,
    /// weekly 未达标时的「本周还差 X 次」
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week_remaining: Option<i64>,
}

/// 派生指标输出（只产标量 / 标注，不改结果表）。
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DerivedOut {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streak: Option<StreakOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<f64>,
}

impl DerivedOut {
    fn is_empty(&self) -> bool {
        self.streak.is_none()
            && self.total.is_none()
            && self.min.is_none()
            && self.max.is_none()
            && self.last.is_none()
    }
}

/// 挂件求值结果（渲染器输入 + 标注）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct WidgetResult {
    /// 所属视图行 id（动态组展开的卡片同属一行）
    pub view_id: String,
    /// 卡片稳定键：模板 id / 字段 id / 显式卡序号
    pub key: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub render: Render,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buckets: Option<Vec<Bucket>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    /// 字段分组桶的展示名（模板 id → 模板名等）
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "DerivedOut::is_empty")]
    pub derived: DerivedOut,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub presence: bool,
    /// 渲染器不兼容提示（UI 置灰 + 一键建议）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 面板视图求值结果。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ViewResult {
    pub view_id: String,
    pub name: String,
    pub panel: Panel,
    /// RFC3339（UTC）——跨午夜 / 时区变更检测锚点（§10.6）
    pub evaluated_at: String,
    pub tz: String,
    pub customized: bool,
    /// 生效配置回显（前端 chips / 编辑器回填）
    pub config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<ItemGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<Item>>,
    /// keyword 命中位置（title / note / tag / field），仅带关键词查询时
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched: Option<BTreeMap<String, Vec<String>>>,
    /// 统计面板：挂件求值结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub widgets: Option<Vec<WidgetResult>>,
    /// limit 截断前的命中总数
    pub total: usize,
}

/// 统计页一个容器（= 一张 stats 视图行）的元信息。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContainerMeta {
    pub view_id: String,
    pub name: String,
    /// horizontal / vertical
    pub layout: String,
}

/// 统计页一次求值（容器 + 挂件流；共享扫描，同源不重复扫 §10）。
/// 挂件按 `view_id` 归属容器，顺序即容器内展示顺序。
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatsPageResult {
    pub evaluated_at: String,
    pub tz: String,
    pub containers: Vec<ContainerMeta>,
    pub widgets: Vec<WidgetResult>,
}

// ==========================================================================
// 条件求值（AST 内存求值 = 最终真相）
// ==========================================================================

/// 锚点合成列（§4.1）：task = COALESCE(due, start)，event = start，log = occurred。
pub fn anchor_time(item: &Item) -> Option<DateTime<Utc>> {
    match item.item_type {
        ItemType::Task => item.due_at.or(item.start_at),
        ItemType::Event => item.start_at,
        ItemType::Log => item.occurred_at,
    }
}

fn col_time(item: &Item, col: &str) -> Option<DateTime<Utc>> {
    match col {
        "col:start_at" => item.start_at,
        "col:end_at" => item.end_at,
        "col:due_at" => item.due_at,
        "col:occurred_at" => item.occurred_at,
        "col:created_at" => Some(item.created_at),
        "col:updated_at" => Some(item.updated_at),
        "col:completed_at" => item.completed_at,
        "col:anchor" => anchor_time(item),
        _ => None,
    }
}

/// extra 中的原始值（文件链接保留键不是字段，不参与条件求值）。
fn extra_value<'a>(item: &'a Item, field: &str) -> Option<&'a Value> {
    if field == FILE_LINKS_KEY {
        return None;
    }
    item.extra.get(field)
}

fn value_is_empty(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        _ => false,
    }
}

fn value_to_list(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        Value::String(s) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 自定义字段值的时刻解析：RFC3339 → 原值；YYYY-MM-DD → 本地日中点。
fn field_value_time(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::String(s) => {
            if let Some(t) = crate::store::parse_dt(s) {
                return Some(t);
            }
            let d = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
            d.and_hms_opt(12, 0, 0).map(crate::tpltime::local_to_utc)
        }
        _ => None,
    }
}

/// 单条件求值。日期条件对 NULL 恒假（empty / not_empty 除外）；
/// 文本比较大小写不敏感（对齐 search）。
pub fn eval_condition(item: &Item, cond: &Condition, ctx: &EvalCtx) -> Result<bool> {
    let cmp = cond.cmp;
    let field = cond.field.as_str();

    // ---- 内置列 ----
    if let Some(role) = col_role(field) {
        return match role {
            FieldRole::TypeCol => {
                let Some(FilterValue::Text(v)) = cond.value.as_ref() else { return Ok(false) };
                Ok(item.item_type.as_str().eq_ignore_ascii_case(v))
            }
            FieldRole::StatusCol => {
                let Some(FilterValue::Text(v)) = cond.value.as_ref() else { return Ok(false) };
                Ok(item
                    .status
                    .map(|s| s.as_str().eq_ignore_ascii_case(v))
                    .unwrap_or(false))
            }
            FieldRole::TemplateCol => {
                let Some(v) = item.template_id.as_deref() else {
                    return Ok(matches!(cmp, Cmp::Empty));
                };
                let Some(FilterValue::Text(x)) = cond.value.as_ref() else {
                    return match cmp {
                        Cmp::NotEmpty => Ok(true),
                        _ => Ok(false),
                    };
                };
                match cmp {
                    Cmp::Eq => Ok(v == x),
                    Cmp::Neq => Ok(v != x),
                    _ => Err(invalid("col:template_id 只支持 eq/neq/empty/not_empty")),
                }
            }
            FieldRole::RecurrenceCol => match cmp {
                Cmp::Empty => Ok(item.recurrence.is_none()),
                Cmp::NotEmpty => Ok(item.recurrence.is_some()),
                _ => Err(invalid("col:recurrence 只支持 empty / not_empty")),
            },
            FieldRole::Text => {
                let v = if field == "col:title" { item.title.as_deref() } else { item.note.as_deref() };
                eval_text(v, cmp, cond.value.as_ref())
            }
            FieldRole::Multi => eval_list(&item.tags, cmp, cond.value.as_ref()),
            FieldRole::Date => {
                let t = col_time(item, field);
                eval_time(t, cmp, cond.value.as_ref(), ctx)
            }
            FieldRole::Number | FieldRole::Select | FieldRole::Bool => {
                Err(invalid(format!("内置列 {field} 不支持该运算符")))
            }
        };
    }

    // ---- 自定义字段（按存储值类型分派：软删字段照常按 extra 历史值求值） ----
    let raw = extra_value(item, field);
    if value_is_empty(raw) {
        return match cmp {
            Cmp::Empty => Ok(true),
            Cmp::NotEmpty => Ok(false),
            // 空集合不含任何指定值：none 恒真；其余比较对空值恒假
            Cmp::HasNone => Ok(true),
            _ => Ok(false),
        };
    }
    let v = raw.unwrap();
    // 关键词编译在任何字段值上使用 contains（与 search 的 CAST AS TEXT 口径一致）
    if matches!(cmp, Cmp::Contains | Cmp::NotContains) {
        let Some(FilterValue::Text(x)) = cond.value.as_ref() else {
            return Err(invalid("contains 比较值必须是字符串"));
        };
        let hit = value_text(v).to_lowercase().contains(&x.to_lowercase());
        return Ok(if cmp == Cmp::Contains { hit } else { !hit });
    }
    match v {
        Value::Number(_) => eval_number(v.as_f64(), cmp, cond.value.as_ref()),
        Value::Bool(b) => match cmp {
            Cmp::IsTrue => Ok(*b),
            Cmp::IsFalse => Ok(!b),
            _ => Ok(false),
        },
        Value::Array(_) => eval_list(&value_to_list(v), cmp, cond.value.as_ref()),
        Value::String(_) => {
            if matches!(cmp, Cmp::On | Cmp::Before | Cmp::After | Cmp::Between | Cmp::Within) {
                eval_time(field_value_time(v), cmp, cond.value.as_ref(), ctx)
            } else {
                eval_text(v.as_str(), cmp, cond.value.as_ref())
            }
        }
        // 对象形态不是合法字段值（写路径已校验）；防御性视为不命中
        Value::Object(_) | Value::Null => Ok(false),
    }
}

fn eval_text(v: Option<&str>, cmp: Cmp, value: Option<&FilterValue>) -> Result<bool> {
    match cmp {
        Cmp::Empty => Ok(v.is_none() || v.is_some_and(|s| s.is_empty())),
        Cmp::NotEmpty => Ok(v.is_some_and(|s| !s.is_empty())),
        _ => {
            let Some(s) = v else { return Ok(false) };
            let Some(FilterValue::Text(x)) = value else {
                return Err(invalid("文本比较值必须是字符串"));
            };
            let (a, b) = (s.to_lowercase(), x.to_lowercase());
            Ok(match cmp {
                Cmp::Eq => a == b,
                Cmp::Neq => a != b,
                Cmp::Contains => a.contains(&b),
                Cmp::NotContains => !a.contains(&b),
                _ => return Err(invalid("文本字段不支持该运算符")),
            })
        }
    }
}

fn eval_number(v: Option<f64>, cmp: Cmp, value: Option<&FilterValue>) -> Result<bool> {
    match cmp {
        Cmp::Empty => Ok(v.is_none()),
        Cmp::NotEmpty => Ok(v.is_some()),
        _ => {
            let Some(n) = v else { return Ok(false) };
            match value.ok_or_else(|| invalid("缺少比较值"))? {
                FilterValue::Number(x) => Ok(match cmp {
                    Cmp::Eq => n == *x,
                    Cmp::Neq => n != *x,
                    Cmp::Gt => n > *x,
                    Cmp::Gte => n >= *x,
                    Cmp::Lt => n < *x,
                    Cmp::Lte => n <= *x,
                    _ => return Err(invalid("数字字段不支持该运算符")),
                }),
                FilterValue::NumPair(p) => {
                    Ok(matches!(cmp, Cmp::Between) && n >= p.from && n <= p.to)
                }
                _ => Err(invalid("数字比较值类型不符")),
            }
        }
    }
}

fn eval_list(list: &[String], cmp: Cmp, value: Option<&FilterValue>) -> Result<bool> {
    match cmp {
        Cmp::Empty => Ok(list.is_empty()),
        Cmp::NotEmpty => Ok(!list.is_empty()),
        Cmp::Contains | Cmp::NotContains => {
            let Some(FilterValue::Text(x)) = value else {
                return Err(invalid("contains 比较值必须是字符串"));
            };
            let x = x.to_lowercase();
            let hit = list.iter().any(|t| t.to_lowercase().contains(&x));
            Ok(if cmp == Cmp::Contains { hit } else { !hit })
        }
        _ => {
            let Some(FilterValue::StrList(vs)) = value else {
                return Err(invalid("any/all/none 接字符串数组"));
            };
            let lower: Vec<String> = vs.iter().map(|v| v.to_lowercase()).collect();
            let has = |t: &String| lower.iter().any(|x| *x == t.to_lowercase());
            Ok(match cmp {
                Cmp::Any => list.iter().any(has),
                Cmp::All => lower.iter().all(|x| list.iter().any(|t| t.to_lowercase() == *x)),
                Cmp::HasNone => !list.iter().any(has),
                _ => return Err(invalid("多值字段不支持该运算符")),
            })
        }
    }
}

fn eval_time(
    t: Option<DateTime<Utc>>,
    cmp: Cmp,
    value: Option<&FilterValue>,
    ctx: &EvalCtx,
) -> Result<bool> {
    match cmp {
        Cmp::Empty => Ok(t.is_none()),
        Cmp::NotEmpty => Ok(t.is_some()),
        _ => {
            let Some(t) = t else { return Ok(false) }; // 日期条件对 NULL 恒假
            match (cmp, value.ok_or_else(|| invalid("缺少比较值"))?) {
                (Cmp::On, FilterValue::Date(v)) => {
                    let (s, e) = ctx.value_bounds(v)?;
                    Ok(t >= s && t <= e)
                }
                (Cmp::Before, FilterValue::Date(v)) => {
                    // < 日始；「今天或之前」写 before tomorrow
                    let d = ctx.point_day(v)?;
                    Ok(t < ctx.day_bounds(d).0)
                }
                (Cmp::After, FilterValue::Date(v)) => {
                    // > 日末
                    let d = ctx.point_day(v)?;
                    Ok(t > ctx.day_bounds(d).1)
                }
                (Cmp::Between, FilterValue::DatePair(p)) => {
                    let (s, _) = ctx.value_bounds(&p.from)?;
                    let (_, e) = ctx.value_bounds(&p.to)?;
                    Ok(t >= s && t <= e)
                }
                (Cmp::Within, FilterValue::Date(v)) => {
                    let (s, e) = ctx.value_bounds(v)?;
                    Ok(t >= s && t <= e)
                }
                _ => Err(invalid("日期运算符与值型不配对")),
            }
        }
    }
}

/// AST 求值：空 children = 恒真。
pub fn eval_filter(item: &Item, node: &FilterNode, ctx: &EvalCtx) -> Result<bool> {
    match node {
        FilterNode::Group { op, children } => {
            if children.is_empty() {
                return Ok(true);
            }
            match op {
                Logic::And => {
                    for c in children {
                        if !eval_filter(item, c, ctx)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                }
                Logic::Or => {
                    for c in children {
                        if eval_filter(item, c, ctx)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
            }
        }
        FilterNode::Cond(c) => eval_condition(item, c, ctx),
    }
}

// ==========================================================================
// 排序（NULL 恒排该级末尾）与分组
// ==========================================================================

#[derive(Debug, Clone, PartialEq)]
enum SortKey {
    Text(Option<String>),
    Num(Option<f64>),
    Time(Option<DateTime<Utc>>),
}

fn sort_key(item: &Item, field: &str) -> SortKey {
    if let Some(role) = col_role(field) {
        return match role {
            FieldRole::Date => SortKey::Time(col_time(item, field)),
            FieldRole::Multi => SortKey::Text(Some(item.tags.join(","))),
            FieldRole::Text => {
                let v = if field == "col:title" { item.title.as_deref() } else { item.note.as_deref() };
                SortKey::Text(v.map(str::to_string))
            }
            _ => SortKey::Text(None),
        };
    }
    match extra_value(item, field) {
        Some(Value::Number(n)) => SortKey::Num(n.as_f64()),
        Some(Value::Bool(b)) => SortKey::Num(Some(if *b { 1.0 } else { 0.0 })),
        Some(Value::String(s)) => SortKey::Text(Some(s.clone())),
        _ => SortKey::Text(None),
    }
}

fn cmp_sort(a: &SortKey, b: &SortKey, dir: SortDir) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (none_a, none_b) = match (a, b) {
        (SortKey::Text(x), SortKey::Text(y)) => (x.is_none(), y.is_none()),
        (SortKey::Num(x), SortKey::Num(y)) => (x.is_none(), y.is_none()),
        (SortKey::Time(x), SortKey::Time(y)) => (x.is_none(), y.is_none()),
        _ => (false, false),
    };
    match (none_a, none_b) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater, // NULL 恒排该级末尾
        (false, true) => Ordering::Less,
        (false, false) => {
            let ord = match (a, b) {
                (SortKey::Text(x), SortKey::Text(y)) => {
                    // 文本按字典序（大小写不敏感）
                    let (x, y) = (
                        x.as_deref().unwrap_or_default().to_lowercase(),
                        y.as_deref().unwrap_or_default().to_lowercase(),
                    );
                    x.cmp(&y)
                }
                (SortKey::Num(x), SortKey::Num(y)) => x
                    .unwrap_or_default()
                    .partial_cmp(&y.unwrap_or_default())
                    .unwrap_or(Ordering::Equal),
                (SortKey::Time(x), SortKey::Time(y)) => {
                    x.unwrap_or_default().cmp(&y.unwrap_or_default())
                }
                _ => Ordering::Equal,
            };
            if dir == SortDir::Desc {
                ord.reverse()
            } else {
                ord
            }
        }
    }
}

fn sort_items(items: &mut [Item], spec: &[SortSpec]) {
    items.sort_by(|a, b| {
        for s in spec {
            let ord = cmp_sort(&sort_key(a, &s.field), &sort_key(b, &s.field), s.dir);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

fn day_key(t: DateTime<Utc>) -> String {
    t.with_timezone(&chrono::Local).date_naive().format("%Y-%m-%d").to_string()
}

/// 时间桶分组（v1 仅 day）：组序 = 组键倒序，NULL 单独成桶恒排末尾，组内按 sort。
fn group_items(items: &[Item], spec: &GroupSpec, sort: &[SortSpec]) -> Vec<ItemGroup> {
    let mut ordered: Vec<&Item> = items.iter().collect();
    ordered.sort_by(|a, b| {
        let ka = col_time(a, &spec.field).map(day_key);
        let kb = col_time(b, &spec.field).map(day_key);
        match (ka, kb) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(x), Some(y)) => y.cmp(&x), // 组键倒序
        }
    });
    let mut out: Vec<ItemGroup> = Vec::new();
    for item in ordered {
        let key = col_time(item, &spec.field).map(day_key);
        match out.last_mut() {
            Some(g) if g.key == key => g.items.push(item.clone()),
            _ => out.push(ItemGroup {
                label: key.clone().unwrap_or_else(|| "未安排".into()),
                key,
                items: vec![item.clone()],
            }),
        }
    }
    for g in out.iter_mut() {
        let mut list = std::mem::take(&mut g.items);
        sort_items(&mut list, sort);
        g.items = list;
    }
    out
}

// ==========================================================================
// 关键词编译（§10.1）：标题 / 备注 / 标签 / 字段值 contains 的 OR 子树
// ==========================================================================

fn compile_keyword(keyword: &str, source: Source, fields: &[FieldDef]) -> FilterNode {
    let kw = keyword.trim();
    if kw.is_empty() {
        return true_group();
    }
    let mut children = vec![
        FilterNode::Cond(Condition {
            field: "col:title".into(),
            cmp: Cmp::Contains,
            value: Some(FilterValue::Text(kw.into())),
        }),
        FilterNode::Cond(Condition {
            field: "col:note".into(),
            cmp: Cmp::Contains,
            value: Some(FilterValue::Text(kw.into())),
        }),
        FilterNode::Cond(Condition {
            field: "col:tags".into(),
            cmp: Cmp::Contains,
            value: Some(FilterValue::Text(kw.into())),
        }),
    ];
    for f in fields {
        if f.id == FILE_LINKS_KEY {
            continue;
        }
        // scope 匹配：限定类型时排除不适用 scope 的字段（all 类型取全部）
        let scope_ok = match source {
            Source::All => true,
            Source::Event => f.scope.is_none_or(|s| s == ItemType::Event),
            Source::Task => f.scope.is_none_or(|s| s == ItemType::Task),
            Source::Log => f.scope.is_none_or(|s| s == ItemType::Log),
        };
        if !scope_ok {
            continue;
        }
        children.push(FilterNode::Cond(Condition {
            field: f.id.clone(),
            cmp: Cmp::Contains,
            value: Some(FilterValue::Text(kw.into())),
        }));
    }
    FilterNode::Group { op: Logic::Or, children }
}

// ==========================================================================
// 预筛下推（§10.2）：松化为 ListFilter 超集；合成列（anchor）不下推
// ==========================================================================

#[derive(Debug, Clone, Default)]
struct ColConstraints {
    item_type: Option<ItemType>,
    status: Option<ItemStatus>,
    start: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    end: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    due: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    occurred: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
}

impl ColConstraints {
    fn has_any(&self) -> bool {
        self.item_type.is_some()
            || self.status.is_some()
            || self.start.0.is_some()
            || self.start.1.is_some()
            || self.end.0.is_some()
            || self.end.1.is_some()
            || self.due.0.is_some()
            || self.due.1.is_some()
            || self.occurred.0.is_some()
            || self.occurred.1.is_some()
    }
    fn merge(self, other: ColConstraints) -> ColConstraints {
        let tighten = |a: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
                       b: (Option<DateTime<Utc>>, Option<DateTime<Utc>>)| {
            (
                match (a.0, b.0) {
                    (Some(x), Some(y)) => Some(x.max(y)),
                    (x, y) => x.or(y),
                },
                match (a.1, b.1) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                },
            )
        };
        ColConstraints {
            item_type: self.item_type.or(other.item_type),
            status: self.status.or(other.status),
            start: tighten(self.start, other.start),
            end: tighten(self.end, other.end),
            due: tighten(self.due, other.due),
            occurred: tighten(self.occurred, other.occurred),
        }
    }
    fn to_list_filter(&self) -> ListFilter {
        let mut f = ListFilter {
            item_type: self.item_type,
            status: self.status,
            ..Default::default()
        };
        f.start_from = self.start.0;
        f.start_to = self.start.1;
        f.end_from = self.end.0;
        f.end_to = self.end.1;
        f.due_from = self.due.0;
        f.due_to = self.due.1;
        f.occurred_from = self.occurred.0;
        f.occurred_to = self.occurred.1;
        f
    }
}

/// 时间窗（起 / 止，None = 不限）。
type TimeWindow = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// 可下推叶子 → 列约束；不可下推（anchor / 文本 / empty 族…）→ None。
fn leaf_constraints(cond: &Condition, ctx: &EvalCtx) -> Option<ColConstraints> {
    let mut c = ColConstraints::default();
    match cond.field.as_str() {
        "col:type" => {
            if cond.cmp != Cmp::Eq {
                return None;
            }
            let Some(FilterValue::Text(v)) = cond.value.as_ref() else { return None };
            c.item_type = ItemType::parse(v);
            c.item_type.is_some().then_some(c)
        }
        "col:status" => {
            if cond.cmp != Cmp::Eq {
                return None;
            }
            let Some(FilterValue::Text(v)) = cond.value.as_ref() else { return None };
            c.status = ItemStatus::parse(v);
            c.status.is_some().then_some(c)
        }
        "col:start_at" | "col:end_at" | "col:due_at" | "col:occurred_at" => {
            let (from, to) = time_window_of(cond, ctx)?;
            let slot = match cond.field.as_str() {
                "col:start_at" => &mut c.start,
                "col:end_at" => &mut c.end,
                "col:due_at" => &mut c.due,
                _ => &mut c.occurred,
            };
            *slot = (from.or(slot.0), to.or(slot.1));
            Some(c)
        }
        _ => None, // anchor 等 COALESCE 合成列不下推（无索引可走）
    }
}

/// 时间条件 → 松化列窗（宁可多取，窗口放宽为整天）。
fn time_window_of(cond: &Condition, ctx: &EvalCtx) -> Option<TimeWindow> {
    match (cond.cmp, cond.value.as_ref()?) {
        (Cmp::On, FilterValue::Date(v)) => {
            let (s, e) = ctx.value_bounds(v).ok()?;
            Some((Some(s), Some(e)))
        }
        (Cmp::Before, FilterValue::Date(v)) => {
            let d = ctx.point_day(v).ok()?;
            Some((None, Some(ctx.day_bounds(d).0)))
        }
        (Cmp::After, FilterValue::Date(v)) => {
            let d = ctx.point_day(v).ok()?;
            Some((Some(ctx.day_bounds(d).1), None))
        }
        (Cmp::Within, FilterValue::Date(v)) => {
            let (s, e) = ctx.value_bounds(v).ok()?;
            Some((Some(s), Some(e)))
        }
        (Cmp::Between, FilterValue::DatePair(p)) => {
            let (s, _) = ctx.value_bounds(&p.from).ok()?;
            let (_, e) = ctx.value_bounds(&p.to).ok()?;
            Some((Some(s), Some(e)))
        }
        _ => None,
    }
}

/// 下推推导代数：AND = 合取，OR = 分支并集，GiveUp = 放弃该子树
/// （少下推 = 候选更多，仍安全——AST 终审）。
#[derive(Debug, Clone)]
enum Push {
    GiveUp,
    Conj(ColConstraints),
    Union(Vec<ColConstraints>),
}

const UNION_CAP: usize = 12;

fn push_node(node: &FilterNode, ctx: &EvalCtx) -> Push {
    match node {
        FilterNode::Cond(c) => match leaf_constraints(c, ctx) {
            Some(c) => Push::Conj(c),
            None => Push::Conj(ColConstraints::default()), // AND 语境下的中性元
        },
        FilterNode::Group { op, children } => {
            if children.is_empty() {
                return Push::Conj(ColConstraints::default());
            }
            let parts: Vec<Push> = children.iter().map(|c| push_node(c, ctx)).collect();
            match op {
                Logic::And => {
                    let mut acc = ColConstraints::default();
                    let mut unions: Vec<Vec<ColConstraints>> = Vec::new();
                    for p in parts {
                        match p {
                            Push::GiveUp => {} // AND 下放弃 = 空约束（超集仍成立）
                            Push::Conj(c) => acc = acc.merge(c),
                            Push::Union(us) => unions.push(us),
                        }
                    }
                    if unions.is_empty() {
                        Push::Conj(acc)
                    } else {
                        // 分配律：acc ∧ (⋃Ui) = ⋃(acc ∧ ui)
                        let mut out = Vec::new();
                        for u in unions.iter().flatten() {
                            out.push(acc.clone().merge(u.clone()));
                            if out.len() > UNION_CAP {
                                return Push::GiveUp;
                            }
                        }
                        Push::Union(out)
                    }
                }
                Logic::Or => {
                    // OR 组整组下推 ⇔ 组内全部子条件可下推（各取超集，SQL 层 OR 并集）
                    let mut out: Vec<ColConstraints> = Vec::new();
                    for p in parts {
                        match p {
                            Push::GiveUp => return Push::GiveUp,
                            Push::Conj(c) => {
                                if c.has_any() {
                                    out.push(c);
                                } else {
                                    return Push::GiveUp; // OR 含不可下推子项 → 放弃该子树
                                }
                            }
                            Push::Union(us) => out.extend(us),
                        }
                        if out.len() > UNION_CAP {
                            return Push::GiveUp;
                        }
                    }
                    if out.is_empty() {
                        Push::GiveUp
                    } else {
                        Push::Union(out)
                    }
                }
            }
        }
    }
}

/// AST + widget 窗口 → 预筛 ListFilter 并集（每条都是超集；预筛不施 limit）。
fn build_prefilters(
    filter: &FilterNode,
    window_col: Option<&str>,
    window: Option<(DateTime<Utc>, DateTime<Utc>)>,
    ctx: &EvalCtx,
) -> Vec<ListFilter> {
    let with_window = |mut lf: ListFilter| {
        if let (Some(col), Some((s, e))) = (window_col, window) {
            let slot = match col {
                "col:start_at" => (&mut lf.start_from, &mut lf.start_to),
                "col:end_at" => (&mut lf.end_from, &mut lf.end_to),
                "col:due_at" => (&mut lf.due_from, &mut lf.due_to),
                _ => (&mut lf.occurred_from, &mut lf.occurred_to),
            };
            slot.0.get_or_insert(s);
            slot.1.get_or_insert(e);
        }
        lf
    };
    match push_node(filter, ctx) {
        Push::GiveUp => vec![with_window(ListFilter::default())],
        Push::Conj(c) => vec![with_window(c.to_list_filter())],
        Push::Union(us) => us
            .into_iter()
            .map(|c| with_window(c.to_list_filter()))
            .collect(),
    }
}

// ==========================================================================
// 挂件管线（§8）：filter → window → 聚合 → 结果表 → 派生指标 → 渲染标注
// ==========================================================================

fn iso_week_key(d: NaiveDate) -> String {
    format!("{}-W{:02}", d.iso_week().year(), d.iso_week().week())
}

fn month_key(d: NaiveDate) -> String {
    d.format("%Y-%m").to_string()
}

fn bucket_key_of(d: NaiveDate, bucket: &str) -> String {
    match bucket {
        "week" => iso_week_key(d),
        "month" => month_key(d),
        _ => d.format("%Y-%m-%d").to_string(),
    }
}

/// 结果表 key 是否时间桶（day / ISO week / month 的字符串形态）。
fn bucket_is_time_key(key: &str) -> bool {
    if NaiveDate::parse_from_str(key, "%Y-%m-%d").is_ok() {
        return true; // day
    }
    let b = key.as_bytes();
    if key.len() == 7 && b[4] == b'-' && key[..4].bytes().all(|c| c.is_ascii_digit()) {
        return true; // month 2026-09
    }
    if key.len() == 8 && &key[4..6] == "-W" {
        return true; // ISO week 2026-W38
    }
    false
}

/// 渲染器兼容性检查（§8 约束表）：不兼容时给 UI 一键建议文案。
fn render_incompatibility(
    render: Render,
    points: Option<&[Point]>,
    buckets: Option<&[Bucket]>,
) -> Option<String> {
    let time_keys = |bs: &[Bucket]| bs.iter().all(|b| bucket_is_time_key(&b.key));
    match render {
        Render::Bar => None,
        Render::Line => {
            if points.is_some() || buckets.is_some_and(|bs| !bs.is_empty() && time_keys(bs)) {
                None
            } else {
                Some("折线需要点列或有序时间桶：字段分组可改用柱状/饼图，或把聚合切成时间桶".into())
            }
        }
        Render::Pie => {
            if buckets.is_some_and(|bs| !bs.is_empty() && !time_keys(bs)) {
                None
            } else {
                Some("饼图需要字段分组（时间桶不适用）：把聚合改为按字段分组".into())
            }
        }
        Render::Heatmap => {
            if buckets.is_some_and(|bs| {
                !bs.is_empty() && bs.iter().all(|b| NaiveDate::parse_from_str(&b.key, "%Y-%m-%d").is_ok())
            }) {
                None
            } else {
                Some("热力图需要按天时间桶（bucket=day）".into())
            }
        }
        Render::Card => None,
    }
}

/// 模板名标注来源（模板桶 key → 展示名 / 图标）。
#[derive(Debug, Clone)]
pub struct TplInfo {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
}

/// 挂件 window 绑定的时间列（§8：group.time_field → 缺省锚点列）。
fn window_time_col(cfg: &WidgetConfig) -> &'static str {
    match &cfg.agg {
        Some(Agg { group: Some(GroupClause::Time { time_field, .. }), .. }) => match time_field.as_str() {
            "col:start_at" => "col:start_at",
            "col:end_at" => "col:end_at",
            "col:due_at" => "col:due_at",
            _ => "col:occurred_at",
        },
        _ => match cfg.dataset.item_type {
            Source::Task => "col:due_at",
            Source::Event => "col:start_at",
            _ => "col:occurred_at",
        },
    }
}

fn window_time_of(item: &Item, col: &str) -> Option<DateTime<Utc>> {
    if col == "col:occurred_at" {
        // values 点列与记录窗口都以 occurred_at 为发生时刻
        return item.occurred_at;
    }
    col_time(item, col)
}

/// 三段管线求值。`pool` 为共享扫描的候选集（已含预筛超集）；streak 的
/// current / longest 在全历史上判定（window 只作用于 recent，§8 特例）。
#[allow(clippy::too_many_arguments)]
pub fn eval_widget(
    view_id: &str,
    key: &str,
    cfg: &WidgetConfig,
    pool: &[Item],
    ctx: &EvalCtx,
    window_override_days: Option<i64>,
    field_defs: &[FieldDef],
    templates: &[TplInfo],
) -> Result<WidgetResult> {
    // ---- 第一段前半：filter（AST 终审）----
    let mut items: Vec<Item> = Vec::new();
    for it in pool {
        if cfg.dataset.item_type.matches(it.item_type) && eval_filter(it, &cfg.dataset.filter, ctx)? {
            items.push(it.clone());
        }
    }

    // ---- 第一段后半：window（滚动窗口；时间列随聚合槽位）----
    let default_days = window_override_days.unwrap_or(365);
    let win_bounds = ctx.window_bounds(cfg.window, default_days)?;
    let win_col = window_time_col(cfg);
    let windowed: Vec<Item> = match win_bounds {
        Some((s, e)) => items
            .iter()
            .filter(|it| window_time_of(it, win_col).is_some_and(|t| t >= s && t <= e))
            .cloned()
            .collect(),
        None => items.clone(),
    };

    let agg = cfg.agg.clone().unwrap_or(Agg { group: None, metric: None });
    let metric_fn = agg.metric.as_ref().map(|m| m.fun.as_str()).unwrap_or("count").to_string();
    let metric_field = agg.metric.as_ref().and_then(|m| m.field.clone());

    // ---- 第一段收口：聚合 → 统一结果表 ----
    let mut buckets: Option<Vec<Bucket>> = None;
    let mut points: Option<Vec<Point>> = None;
    let mut labels: BTreeMap<String, String> = BTreeMap::new();

    if metric_fn == "values" {
        // 点列：仅 log；数值 = metric.field，时刻 = occurred_at，group 必为 null
        let mut pts: Vec<(DateTime<Utc>, f64)> = windowed
            .iter()
            .filter_map(|it| {
                let v = extra_value(it, metric_field.as_deref().unwrap_or(""))?.as_f64()?;
                Some((it.occurred_at?, v))
            })
            .collect();
        pts.sort_by_key(|(t, _)| *t);
        points = Some(pts.into_iter().map(|(t, v)| Point { t: crate::store::dt(t), v }).collect());
    } else {
        let metric_of = |it: &Item| -> Option<f64> {
            match metric_fn.as_str() {
                "count" => Some(1.0),
                _ => extra_value(it, metric_field.as_deref().unwrap_or(""))?.as_f64(),
            }
        };
        match &agg.group {
            Some(GroupClause::Time { bucket, time_field }) => {
                let mut sums: BTreeMap<String, f64> = BTreeMap::new();
                let mut counts: BTreeMap<String, i64> = BTreeMap::new();
                let mut mins: BTreeMap<String, f64> = BTreeMap::new();
                let mut maxs: BTreeMap<String, f64> = BTreeMap::new();
                for it in &windowed {
                    let Some(t) = col_time(it, time_field) else { continue };
                    let Some(v) = metric_of(it) else { continue };
                    let local_day = t.with_timezone(&chrono::Local).date_naive();
                    let key = bucket_key_of(local_day, bucket);
                    *sums.entry(key.clone()).or_insert(0.0) += v;
                    *counts.entry(key.clone()).or_insert(0) += 1;
                    mins.entry(key.clone()).and_modify(|m| *m = m.min(v)).or_insert(v);
                    maxs.entry(key.clone()).and_modify(|m| *m = m.max(v)).or_insert(v);
                }
                // 时间桶补零：窗口内每个桶都出桶（热力图与折线依赖完整时间轴）；
                // 时间桶升序（line / heatmap 用）
                let mut ordered: Vec<Bucket> = Vec::new();
                let cur_start = match win_bounds {
                    Some((s, _)) => s.with_timezone(&chrono::Local).date_naive(),
                    None => windowed
                        .iter()
                        .filter_map(|it| col_time(it, time_field))
                        .map(|t| t.with_timezone(&chrono::Local).date_naive())
                        .min()
                        .unwrap_or(ctx.today),
                };
                // 滚动窗口恒以今天为终点（窗口末端是次日零点，按天填充取今天）
                let cur_end = ctx.today;
                let mut cur = cur_start;
                while cur <= cur_end {
                    let key = bucket_key_of(cur, bucket);
                    let v = match metric_fn.as_str() {
                        "sum" => sums.get(&key).copied().unwrap_or(0.0),
                        "avg" => {
                            let c = counts.get(&key).copied().unwrap_or(0);
                            if c == 0 {
                                0.0
                            } else {
                                sums.get(&key).copied().unwrap_or(0.0) / c as f64
                            }
                        }
                        "min" => mins.get(&key).copied().unwrap_or(0.0),
                        "max" => maxs.get(&key).copied().unwrap_or(0.0),
                        _ => counts.get(&key).copied().unwrap_or(0) as f64,
                    };
                    ordered.push(Bucket { key, value: v });
                    cur = match bucket.as_str() {
                        "week" => cur + Duration::weeks(1),
                        "month" => {
                            let (ny, nm) = if cur.month() == 12 {
                                (cur.year() + 1, 1)
                            } else {
                                (cur.year(), cur.month() + 1)
                            };
                            NaiveDate::from_ymd_opt(ny, nm, 1).unwrap()
                        }
                        _ => cur + Duration::days(1),
                    };
                }
                buckets = Some(ordered);
            }
            Some(GroupClause::Field { field }) => {
                // multiselect / tags = 每个命中值各计一次（一条可进多个桶）
                let mut counts: BTreeMap<String, f64> = BTreeMap::new();
                let add = |counts: &mut BTreeMap<String, f64>, v: &str| {
                    *counts.entry(v.to_string()).or_insert(0.0) += 1.0;
                };
                for it in &windowed {
                    if field == "col:template_id" {
                        if let Some(tid) = &it.template_id {
                            add(&mut counts, tid);
                        }
                        continue;
                    }
                    if field == "col:tags" {
                        for t in &it.tags {
                            add(&mut counts, t);
                        }
                        continue;
                    }
                    let Some(v) = extra_value(it, field) else { continue };
                    match v {
                        Value::Array(_) => {
                            for x in value_to_list(v) {
                                add(&mut counts, &x);
                            }
                        }
                        Value::String(s) => add(&mut counts, s),
                        _ => {}
                    }
                }
                let mut bs: Vec<Bucket> =
                    counts.into_iter().map(|(k, v)| Bucket { key: k, value: v }).collect();
                // 字段分组按 value 降序（bar / pie 默认）
                bs.sort_by(|a, b| {
                    b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal)
                });
                if field == "col:template_id" {
                    for b in &bs {
                        if let Some(t) = templates.iter().find(|t| t.id == b.key) {
                            labels.insert(b.key.clone(), t.name.clone());
                        }
                    }
                }
                buckets = Some(bs);
            }
            None => {
                // 无分组：整窗聚合为单桶（card / 汇总形态）
                let vals: Vec<f64> = windowed.iter().filter_map(metric_of).collect();
                let v = match metric_fn.as_str() {
                    "count" => vals.len() as f64,
                    "sum" => vals.iter().sum(),
                    "avg" => {
                        if vals.is_empty() {
                            0.0
                        } else {
                            vals.iter().sum::<f64>() / vals.len() as f64
                        }
                    }
                    "min" => vals.iter().cloned().reduce(f64::min).unwrap_or(0.0),
                    "max" => vals.iter().cloned().reduce(f64::max).unwrap_or(0.0),
                    _ => vals.len() as f64,
                };
                buckets = Some(vec![Bucket { key: "all".into(), value: v }]);
            }
        }
    }

    // ---- 第二段：派生指标（纯函数 over 结果表，只产标量 / 标注）----
    let mut derived = DerivedOut::default();
    if cfg.derived.as_ref().and_then(|d| d.kind.as_deref()) == Some("streak") {
        // streak 特例：current / longest 在全历史上判定，window 只作用 recent
        let mut day_counts: BTreeMap<NaiveDate, i64> = BTreeMap::new();
        for it in &items {
            if let Some(t) = window_time_of(it, win_col) {
                *day_counts.entry(t.with_timezone(&chrono::Local).date_naive()).or_insert(0) += 1;
            }
        }
        let goal = cfg.derived.as_ref().and_then(|d| d.goal);
        derived.streak = Some(compute_streak(&day_counts, goal, ctx, windowed.len() as i64));
    }
    {
        // total / min / max / last：over 窗口内结果表
        let vals: Vec<f64> = match (&points, &buckets) {
            (Some(ps), _) => ps.iter().map(|p| p.v).collect(),
            (None, Some(bs)) => bs.iter().map(|b| b.value).collect(),
            _ => Vec::new(),
        };
        if !vals.is_empty() {
            derived.total = Some(vals.iter().sum());
            derived.min = Some(vals.iter().cloned().reduce(f64::min).unwrap());
            derived.max = Some(vals.iter().cloned().reduce(f64::max).unwrap());
            derived.last = Some(vals[vals.len() - 1]);
        }
    }

    // ---- 第三段前：unit（options 覆盖，缺省取字段定义）+ 渲染兼容性标注 ----
    let unit = cfg.options.unit.clone().or_else(|| {
        metric_field.as_deref().and_then(|fid| {
            field_defs
                .iter()
                .find(|f| f.id == fid)
                .and_then(|f| f.options.get("unit"))
                .and_then(|u| u.as_str().map(str::to_string))
        })
    });
    let error = render_incompatibility(cfg.render, points.as_deref(), buckets.as_deref());

    Ok(WidgetResult {
        view_id: view_id.into(),
        key: key.into(),
        title: cfg.title.clone().unwrap_or_else(|| key.to_string()),
        icon: cfg.icon.clone(),
        render: cfg.render,
        buckets,
        points,
        labels,
        derived,
        unit,
        presence: cfg.options.presence,
        error,
    })
}

/// streak：daily:N（缺省 1）= 连续达标日；weekly:N = 连续达标周（本地周一起）。
/// 今天已达标含今天，否则存活到昨天 / 上周。
fn compute_streak(
    day_counts: &BTreeMap<NaiveDate, i64>,
    goal: Option<Goal>,
    ctx: &EvalCtx,
    recent: i64,
) -> StreakOut {
    match goal {
        Some(Goal::Weekly { weekly }) => {
            let need = weekly.max(1);
            let mut week_counts: BTreeMap<NaiveDate, i64> = BTreeMap::new();
            for (d, c) in day_counts {
                let mon = *d - Duration::days(d.weekday().num_days_from_monday() as i64);
                *week_counts.entry(mon).or_insert(0) += c;
            }
            let met_mondays: HashSet<NaiveDate> = week_counts
                .iter()
                .filter(|(_, c)| **c >= need)
                .map(|(m, _)| *m)
                .collect();
            let this_mon =
                ctx.today - Duration::days(ctx.today.weekday().num_days_from_monday() as i64);
            let mut current = 0i64;
            let mut cursor = if met_mondays.contains(&this_mon) {
                Some(this_mon)
            } else {
                Some(this_mon - Duration::weeks(1))
            };
            while let Some(m) = cursor {
                if met_mondays.contains(&m) {
                    current += 1;
                    cursor = Some(m - Duration::weeks(1));
                } else {
                    break;
                }
            }
            let mut longest = 0i64;
            let mut run = 0i64;
            let mut prev: Option<NaiveDate> = None;
            for m in week_counts.keys().filter(|m| met_mondays.contains(*m)) {
                run = if prev == Some(*m - Duration::weeks(1)) { run + 1 } else { 1 };
                longest = longest.max(run);
                prev = Some(*m);
            }
            let remaining = need - week_counts.get(&this_mon).copied().unwrap_or(0);
            StreakOut {
                current,
                longest,
                recent,
                week_remaining: (remaining > 0).then_some(remaining),
            }
        }
        _ => {
            let need = match goal {
                Some(Goal::Daily { daily }) => daily.max(1),
                _ => 1,
            };
            let met_days: HashSet<NaiveDate> =
                day_counts.iter().filter(|(_, c)| **c >= need).map(|(d, _)| *d).collect();
            let mut current = 0i64;
            if !met_days.is_empty() {
                let mut cursor = if met_days.contains(&ctx.today) {
                    Some(ctx.today)
                } else {
                    Some(ctx.today - Duration::days(1))
                };
                while let Some(c) = cursor {
                    if met_days.contains(&c) {
                        current += 1;
                        cursor = Some(c - Duration::days(1));
                    } else {
                        break;
                    }
                }
            }
            let mut longest = 0i64;
            let mut run = 0i64;
            let mut prev: Option<NaiveDate> = None;
            for d in day_counts.keys().filter(|d| met_days.contains(*d)) {
                run = if prev == Some(*d - Duration::days(1)) { run + 1 } else { 1 };
                longest = longest.max(run);
                prev = Some(*d);
            }
            StreakOut { current, longest, recent, week_remaining: None }
        }
    }
}

// ==========================================================================
// 容器模型（v2）：统计页 = 容器列表，容器 = 布局 + 挂件列表。
// 三个内置预设只是预置的容器组合——热力图 = 纵向容器 × 1 挂件；打卡连续 =
// 横向容器 × 动态规则（每个 pinned log 模板一张卡）；数值趋势 = 纵向容器 ×
// 动态规则（每个活跃 number 字段一条线）。用户用同一条路径（＋容器 → ＋挂件）
// 可以搭出同样的页面，不存在代码特判组件。
// ==========================================================================

/// 容器布局：水平（并排，前端每行最多 4 张，可换行）或垂直（单列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContainerLayout {
    Horizontal,
    #[default]
    Vertical,
}

impl ContainerLayout {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "horizontal" => Some(Self::Horizontal),
            "vertical" => Some(Self::Vertical),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }
}

/// 容器的解析态（v2.1：全部容器同权——无内置 / 隐藏 / 动态概念，
/// 预设只是开库时替用户建好的普通容器，可编辑可真删，恢复 = 重新铺预设）。
#[derive(Debug, Clone, Default)]
struct ContainerState {
    layout: ContainerLayout,
    widgets: Vec<WidgetConfig>,
}

/// 容器配置解析（值形态；kind = container，layout + 挂件列表）。
fn parse_container_value(v: &Value) -> std::result::Result<ContainerState, String> {
    match v.get("kind").and_then(|k| k.as_str()) {
        Some("container") => {
            let mut st = ContainerState { layout: ContainerLayout::Vertical, ..Default::default() };
            st.layout = v
                .get("layout")
                .and_then(|l| l.as_str())
                .and_then(ContainerLayout::parse)
                .ok_or_else(|| "容器的 layout 必须是 horizontal / vertical".to_string())?;
            if let Some(list) = v.get("widgets").and_then(|w| w.as_array()) {
                for w in list {
                    st.widgets.push(
                        serde_json::from_value::<WidgetConfig>(w.clone())
                            .map_err(|e| format!("容器内挂件配置无效: {e}"))?,
                    );
                }
            }
            Ok(st)
        }
        _ => Err("统计视图配置必须是容器（kind=container）".into()),
    }
}

fn parse_container(row: &ViewDef) -> Result<ContainerState> {
    parse_container_value(&row.config).map_err(|e| invalid(format!("视图 {} 配置无效: {e}", row.id)))
}

/// 预设统计容器定义（恢复 / 首次播种的唯一事实源；名称 + 布局 + 挂件构建器）。
const PRESET_STATS: &[(&str, &str, ContainerLayout)] = &[
    (VIEW_STATS_HEATMAP, "记录热力图", ContainerLayout::Vertical),
    (VIEW_STATS_STREAKS, "打卡连续", ContainerLayout::Horizontal),
    (VIEW_STATS_SERIES, "数值趋势", ContainerLayout::Vertical),
];


/// 预设热力图挂件（全部 log，按天计数，365 天窗口）。
fn preset_heatmap_widget() -> WidgetConfig {
    serde_json::from_value(json!({
        "kind": "widget",
        "title": "记录热力图",
        "dataset": { "item_type": "log", "filter": { "op": "and", "children": [] } },
        "window": { "days": 365 },
        "agg": {
            "group": { "by": "time", "bucket": "day", "time_field": "col:occurred_at" },
            "metric": { "fn": "count" }
        },
        "render": "heatmap",
        "options": { "presence": false, "top_n": 8 }
    }))
    .expect("预设热力图配置内联合法")
}

/// 预设打卡卡（每个 pinned log 模板一张；恢复 / 播种时按当前数据展开）。
fn preset_streak_widgets(templates: &[TplInfo]) -> Vec<WidgetConfig> {
    let st = ContainerState {
        layout: ContainerLayout::Horizontal,
        widgets: Vec::new(),
    };
    let _ = &st;
    templates
        .iter()
        .map(|tpl| {
            let mut c = streak_card_cfg(tpl, Some(Window::Days { days: 365 }), Some(Goal::Daily { daily: 1 }));
            c.window = Some(Window::Days { days: 365 });
            c
        })
        .collect()
}

/// 预设趋势线（每个活跃 number 字段一条 values 线）。
fn preset_series_widgets(fields: &[FieldDef]) -> Vec<WidgetConfig> {
    fields
        .iter()
        .filter(|f| f.kind == crate::model::FieldKind::Number)
        .map(|f| series_line_cfg(f, Some(Window::Days { days: 365 })))
        .collect()
}

/// 打卡卡配置：filter = `col:template_id eq <tpl>`，day×count + streak derived（§8）。
fn streak_card_cfg(
    tpl: &TplInfo,
    window: Option<Window>,
    goal: Option<Goal>,
) -> WidgetConfig {
    WidgetConfig {
        kind: "widget".into(),
        dataset: WidgetDataset {
            item_type: Source::Log,
            filter: FilterNode::Group {
                op: Logic::And,
                children: vec![FilterNode::Cond(Condition {
                    field: "col:template_id".into(),
                    cmp: Cmp::Eq,
                    value: Some(FilterValue::Text(tpl.id.clone())),
                })],
            },
        },
        window,
        agg: Some(Agg {
            group: Some(GroupClause::Time {
                bucket: "day".into(),
                time_field: "col:occurred_at".into(),
            }),
            metric: Some(Metric { fun: "count".into(), field: None }),
        }),
        derived: Some(DerivedSpec {
            kind: Some("streak".into()),
            goal: goal.or(Some(Goal::Daily { daily: 1 })),
        }),
        render: Render::Card,
        options: WidgetOptions::default(),
        title: Some(tpl.name.clone()),
        icon: tpl.icon.clone(),
    }
}

/// 趋势线配置：活跃 number 字段 → values 点列。
fn series_line_cfg(field: &FieldDef, window: Option<Window>) -> WidgetConfig {
    WidgetConfig {
        kind: "widget".into(),
        dataset: WidgetDataset {
            item_type: Source::Log,
            filter: FilterNode::Group {
                op: Logic::And,
                children: vec![FilterNode::Cond(Condition {
                    field: field.id.clone(),
                    cmp: Cmp::NotEmpty,
                    value: None,
                })],
            },
        },
        window,
        agg: Some(Agg {
            group: None,
            metric: Some(Metric { fun: "values".into(), field: Some(field.id.clone()) }),
        }),
        derived: None,
        render: Render::Line,
        options: WidgetOptions::default(),
        title: Some(field.name.clone()),
        icon: None,
    }
}

// ==========================================================================
// Store：view CRUD + 求值入口
// ==========================================================================

fn invalid_cfg(msg: impl Into<String>) -> MyDayError {
    MyDayError::Invalid(msg.into())
}

impl Store {
    // ---- CRUD ------------------------------------------------------------

    pub fn list_views(&self, panel: Option<Panel>) -> Result<Vec<ViewDef>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, panel, config, config_user, builtin, sort, created_at, updated_at \
             FROM view_defs ORDER BY sort ASC, id ASC",
        )?;
        let rows: Vec<ViewDef> = stmt
            .query_map([], view_def_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().filter(|v| panel.is_none_or(|p| v.panel == p)).collect())
    }

    pub fn get_view(&self, id: &str) -> Result<ViewDef> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, panel, config, config_user, builtin, sort, created_at, updated_at \
             FROM view_defs WHERE id = ?1",
            params![id],
            view_def_mapper,
        )
        .optional()?
        .ok_or_else(|| crate::MyDayError::NotFound(format!("view {id} not found")))
    }

    /// 新建用户视图（校验配置；id = view_<8hex>）。
    pub fn create_view(&self, name: &str, panel: Panel, config: &Value) -> Result<ViewDef> {
        let name = name.trim();
        if name.is_empty() {
            return Err(invalid_cfg("视图名不能为空"));
        }
        self.validate_config_for(panel, config)?;
        let id = format!("view_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let now = crate::store::dt(Utc::now());
        {
            let conn = self.lock()?;
            conn.execute(
                "INSERT INTO view_defs (id, name, panel, config, config_user, builtin, sort, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, 0, (SELECT COALESCE(MAX(sort), 0) + 1 FROM view_defs), ?5, ?5)",
                params![id, name, panel.as_str(), config.to_string(), now],
            )?;
        }
        self.get_view(&id)
    }

    /// 保存：内置视图写 config_user（定制，与 GUI 编辑一致，可脚本化，不拒绝）；
    /// 用户视图覆盖其 config。name 传 Some 时同时改名（仅用户视图）。
    pub fn save_view(&self, id: &str, name: Option<&str>, config: &Value) -> Result<ViewDef> {
        let view = self.get_view(id)?;
        if let Some(n) = name {
            if n.trim().is_empty() {
                return Err(invalid_cfg("视图名不能为空"));
            }
        }
        if view.builtin {
            self.validate_config_for(view.panel, config)?;
            let conn = self.lock()?;
            conn.execute(
                "UPDATE view_defs SET config_user = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, config.to_string(), crate::store::dt(Utc::now())],
            )?;
        } else {
            self.validate_config_for(view.panel, config)?;
            let conn = self.lock()?;
            if let Some(n) = name {
                conn.execute(
                    "UPDATE view_defs SET name = ?2, config = ?3, updated_at = ?4 WHERE id = ?1",
                    params![id, n.trim(), config.to_string(), crate::store::dt(Utc::now())],
                )?;
            } else {
                conn.execute(
                    "UPDATE view_defs SET config = ?2, updated_at = ?3 WHERE id = ?1",
                    params![id, config.to_string(), crate::store::dt(Utc::now())],
                )?;
            }
        }
        self.get_view(id)
    }

    /// 删除用户视图；内置视图不可删。
    pub fn delete_view(&self, id: &str) -> Result<ViewDef> {
        let view = self.get_view(id)?;
        if view.builtin {
            return Err(crate::MyDayError::Conflict("内置视图不可删除（可重置回 seed）".into()));
        }
        {
            let conn = self.lock()?;
            conn.execute("DELETE FROM view_defs WHERE id = ?1 AND builtin = 0", params![id])?;
        }
        Ok(view)
    }

    /// 重置内置视图 = 清空 config_user（用户视图无 seed，不支持重置）。
    pub fn reset_view(&self, id: &str) -> Result<ViewDef> {
        let view = self.get_view(id)?;
        if !view.builtin {
            return Err(invalid_cfg("用户视图没有 seed，无法重置（可直接编辑或删除）"));
        }
        {
            let conn = self.lock()?;
            conn.execute(
                "UPDATE view_defs SET config_user = NULL, updated_at = ?2 WHERE id = ?1",
                params![id, crate::store::dt(Utc::now())],
            )?;
        }
        self.get_view(id)
    }

    /// 另存为：从任意视图复制**生效配置**出一张用户视图（§11）。
    pub fn duplicate_view(&self, id: &str, new_name: &str) -> Result<ViewDef> {
        let view = self.get_view(id)?;
        let effective = view.effective_config().clone();
        self.create_view(new_name, view.panel, &effective)
    }

    fn validate_config_for(&self, panel: Panel, config: &Value) -> Result<()> {
        let fields = self.list_field_defs(None)?;
        match panel {
            Panel::Stats => {
                // 容器（kind=container）或单挂件（kind=widget，兼容旧版/快捷创建）
                if config.get("kind") == Some(&json!("widget")) {
                    validate_widget_config(config, &fields).map(|_| ())
                } else {
                    parse_container_value(config)
                        .map_err(|e| invalid_cfg(format!("容器配置无效: {e}")))?;
                    if let Some(list) = config.get("widgets").and_then(|w| w.as_array()) {
                        for w in list {
                            validate_widget_config(w, &fields)?;
                        }
                    }
                    Ok(())
                }
            }
            _ => validate_view_config(config, &fields).map(|_| ()),
        }
    }

    // ---- 求值入口 ---------------------------------------------------------

    /// 视图求值（§10 顺序）。keyword 为运行时输入（搜索页），编译进 AST；
    /// limit_override 覆盖数据集 limit（CLI `item query --limit`）。
    pub fn query_view(
        &self,
        view_id: &str,
        keyword: Option<&str>,
        limit_override: Option<i64>,
    ) -> Result<ViewResult> {
        let view = self.get_view(view_id)?;
        let ctx = EvalCtx::now();
        let config = view.effective_config().clone();
        let fields = self.list_field_defs(None)?;

        if view.panel == Panel::Stats {
            let templates = self.tpl_infos()?;
            let widgets = self.eval_stats_row(&view, &ctx, &fields, &templates)?;
            let customized = view.customized();
            return Ok(ViewResult {
                view_id: view.id,
                name: view.name.clone(),
                panel: view.panel,
                evaluated_at: crate::store::dt(ctx.now),
                tz: local_tz_name(),
                customized,
                config,
                group: None,
                groups: None,
                items: None,
                matched: None,
                widgets: Some(widgets),
                total: 0,
            });
        }

        let cfg: ViewConfig =
            serde_json::from_value(config.clone()).map_err(|e| invalid_cfg(format!("视图配置无效: {e}")))?;
        let mut dataset = cfg.dataset.clone();
        if let Some(n) = limit_override {
            dataset.limit = Some(n);
        }

        // 1. 编译：keyword → OR 子树并入 AST（合取）
        let filter = match keyword.map(str::trim).filter(|k| !k.is_empty()) {
            Some(kw) => FilterNode::Group {
                op: Logic::And,
                children: vec![dataset.filter.clone(), compile_keyword(kw, dataset.item_type, &fields)],
            },
            None => dataset.filter.clone(),
        };

        // 2-4. 预筛（超集，不施 limit）→ hydrate 一次 → AST 终审
        let prefilters = build_prefilters(&filter, None, None, &ctx);
        let mut pool: Vec<Item> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for lf in &prefilters {
            for it in self.list_items_unbounded(lf)? {
                if seen.insert(it.id.clone()) {
                    pool.push(it);
                }
            }
        }
        let mut matched_items: Vec<Item> = Vec::new();
        for it in &pool {
            if eval_filter(it, &filter, &ctx)? {
                matched_items.push(it.clone());
            }
        }

        // 5. 排序 → limit / offset 永远最后
        let total = matched_items.len();
        let customized = view.customized();
        sort_items(&mut matched_items, &dataset.sort);
        if let Some(n) = dataset.limit {
            if n >= 0 {
                matched_items.truncate(n as usize);
            }
        }

        // 分组（v1 时间桶）：组序 = 组键倒序，组内按 sort
        let groups =
            dataset.group.as_ref().map(|g| group_items(&matched_items, g, &dataset.sort));

        // keyword 命中位置（与 search() 同口径，信息量不缩水）
        let matched = keyword.map(str::trim).filter(|k| !k.is_empty()).map(|kw| {
            let mut map = BTreeMap::new();
            let kwl = kw.to_lowercase();
            for it in &matched_items {
                let mut where_: Vec<String> = Vec::new();
                if it.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&kwl)) {
                    where_.push("title".into());
                }
                if it.note.as_deref().is_some_and(|n| n.to_lowercase().contains(&kwl)) {
                    where_.push("note".into());
                }
                if it.tags.iter().any(|t| t.to_lowercase().contains(&kwl)) {
                    where_.push("tag".into());
                }
                if let Some(obj) = it.extra.as_object() {
                    if obj.iter().any(|(k, v)| {
                        k != FILE_LINKS_KEY && value_text(v).to_lowercase().contains(&kwl)
                    }) {
                        where_.push("field".into());
                    }
                }
                if !where_.is_empty() {
                    map.insert(it.id.clone(), where_);
                }
            }
            map
        });

        Ok(ViewResult {
            view_id: view.id,
            name: view.name.clone(),
            panel: view.panel,
            evaluated_at: crate::store::dt(ctx.now),
            tz: local_tz_name(),
            customized,
            config,
            group: dataset.group.clone(),
            groups,
            items: Some(matched_items),
            matched,
            widgets: None,
            total,
        })
    }

    fn tpl_infos(&self) -> Result<Vec<TplInfo>> {
        Ok(self
            .list_templates()?
            .into_iter()
            .map(|t| TplInfo { id: t.id, name: t.name, icon: t.icon })
            .collect())
    }

    /// 统计页一次求值：全部 stats 容器（预置 + 用户自建，完全同权）展开为挂件流。
    /// 窗口完全由各挂件自身配置决定（页面不再有全局范围档）；
    /// 共享扫描：存在 streak 卡（全历史）或非 log 挂件 → 整页取 all；
    /// 否则取各挂件窗口的并集（最大窗口）——同源不重复扫（§10）。
    pub fn query_stats_page(&self) -> Result<StatsPageResult> {
        let ctx = EvalCtx::now();
        let fields = self.list_field_defs(None)?;
        let templates: Vec<TplInfo> = self
            .list_templates()?
            .into_iter()
            .map(|t| TplInfo { id: t.id, name: t.name, icon: t.icon })
            .collect();
        let rows = self.list_views(Some(Panel::Stats))?;

        let mut metas: Vec<ContainerMeta> = Vec::new();
        // (view_id, key, cfg)：key = `{view_id}#{i}`（容器内显式挂件下标）
        let mut jobs: Vec<(String, String, WidgetConfig)> = Vec::new();
        for row in &rows {
            let st = parse_container(row)?;
            metas.push(ContainerMeta {
                view_id: row.id.clone(),
                name: row.name.clone(),
                layout: st.layout.as_str().into(),
            });
            for (i, cfg) in st.widgets.iter().enumerate() {
                jobs.push((row.id.clone(), format!("{}#{}", row.id, i), cfg.clone()));
            }
        }

        // 共享扫描：正确性优先——拿不准就取 all（个人量级毫秒级）
        let needs_all = jobs.iter().any(|(_, _, w)| {
            w.dataset.item_type != Source::Log
                || w.derived.as_ref().and_then(|d| d.kind.as_deref()) == Some("streak")
        });
        let pool: Vec<Item> = if needs_all {
            self.list_items_unbounded(&ListFilter::default())?
        } else {
            let mut min_start: Option<DateTime<Utc>> = None;
            for (_, _, w) in &jobs {
                match ctx.window_bounds(w.window, 365)? {
                    Some((s, _)) => {
                        min_start = Some(match min_start {
                            Some(m) => m.min(s),
                            None => s,
                        });
                    }
                    None => {
                        min_start = None;
                        break;
                    }
                }
            }
            let mut lf = ListFilter { item_type: Some(ItemType::Log), ..Default::default() };
            lf.occurred_from = min_start;
            self.list_items_unbounded(&lf)?
        };

        let mut widgets = Vec::new();
        for (vid, key, cfg) in &jobs {
            widgets.push(eval_widget(
                vid, key, cfg, &pool, &ctx, None, &fields, &templates,
            )?);
        }
        Ok(StatsPageResult {
            evaluated_at: crate::store::dt(ctx.now),
            tz: local_tz_name(),
            containers: metas,
            widgets,
        })
    }

    /// 单个统计容器求值（query_view 对 stats 行）。
    fn eval_stats_row(
        &self,
        row: &ViewDef,
        ctx: &EvalCtx,
        fields: &[FieldDef],
        templates: &[TplInfo],
    ) -> Result<Vec<WidgetResult>> {
        let st = parse_container(row)?;
        let pool: Vec<Item> = if st.widgets.iter().any(|w| {
            w.dataset.item_type != Source::Log
                || w.derived.as_ref().and_then(|d| d.kind.as_deref()) == Some("streak")
        }) {
            self.list_items_unbounded(&ListFilter::default())?
        } else {
            self.list_items_unbounded(&ListFilter {
                item_type: Some(ItemType::Log),
                ..Default::default()
            })?
        };
        let mut out = Vec::new();
        for (i, cfg) in st.widgets.iter().enumerate() {
            out.push(eval_widget(
                &row.id,
                &format!("{}#{}", row.id, i),
                cfg,
                &pool,
                ctx,
                None,
                fields,
                templates,
            )?);
        }
        Ok(out)
    }

    // ---- 统计页预置容器（v2.1）：一次性播种 + 恢复 ------------------------

    /// 一次性播种统计页预置容器（settings.stats_presets_seeded 标记）。
    /// 只在数据库尚无任何 stats 行时铺预设——之后删除即删除，恢复走
    /// [`Store::restore_stats_defaults`]。
    pub(crate) fn seed_stats_presets(&self) -> Result<()> {
        let seeded = {
            let conn = self.lock()?;
            crate::store::Store::get_setting_via(&conn, "stats_presets_seeded")?.is_some()
        };
        if seeded {
            return Ok(());
        }
        if self.list_views(Some(Panel::Stats))?.is_empty() {
            for (id, name, _) in PRESET_STATS {
                self.insert_preset(id, name)?;
            }
        }
        self.set_setting("stats_presets_seeded", "1")?;
        Ok(())
    }
    /// 恢复默认统计页 = **完全重置**：清除当前全部统计容器与挂件
    /// （含用户新增 / 修改，不影响任何条目数据），按当前数据铺回三个预设
    /// （打卡卡 = 当前 pinned log 模板各一张；趋势线 = 当前活跃 number 字段
    /// 各一条；热力图固定配置）。不常用，UI 藏在页面底部 + 确认弹窗。
    pub fn restore_stats_defaults(&self) -> Result<Vec<String>> {
        {
            let conn = self.lock()?;
            conn.execute("DELETE FROM view_defs WHERE panel = 'stats'", [])?;
        }
        let mut created = Vec::new();
        for (id, name, _) in PRESET_STATS {
            self.insert_preset(id, name)?;
            created.push((*name).to_string());
        }
        Ok(created)
    }

    fn insert_preset(&self, id: &str, name: &str) -> Result<()> {
        let layout = PRESET_STATS
            .iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|(_, _, l)| *l)
            .unwrap_or(ContainerLayout::Vertical);
        let templates: Vec<TplInfo> = self
            .list_templates()?
            .into_iter()
            .filter(|t| t.item_type == ItemType::Log && t.pinned)
            .map(|t| TplInfo { id: t.id, name: t.name, icon: t.icon })
            .collect();
        let fields = self.list_field_defs(None)?;
        let widgets = if id == VIEW_STATS_HEATMAP {
            vec![preset_heatmap_widget()]
        } else if id == VIEW_STATS_STREAKS {
            preset_streak_widgets(&templates)
        } else {
            preset_series_widgets(&fields)
        };
        let config = json!({
            "kind": "container",
            "layout": layout.as_str(),
            "widgets": widgets,
        });
        let now = crate::store::dt(Utc::now());
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO view_defs (id, name, panel, config, config_user, builtin, sort, created_at, updated_at)
             VALUES (?1, ?2, 'stats', ?3, NULL, 0, (SELECT COALESCE(MAX(sort), 0) + 1 FROM view_defs), ?4, ?4)",
            params![id, name, config.to_string(), now],
        )?;
        Ok(())
    }
}

/// `myday stats --json` / IPC `stats_summary` 的挂件化实现（§12）：
/// 固定按**预设定义**（代码常量，不读 view_defs——统计页已是用户事实）+
/// 当前数据求值；机器消费的稳定契约不随页面定制 / 删除 / 恢复漂移；
/// 输出信封与旧实现完全兼容。全部挂件在同一份 log 全量内存集上求值（§10）。
pub fn stats_summary_via_widgets(store: &Store, days: i64) -> Result<StatsSummary> {
    let days = days.clamp(7, 1095);
    let ctx = EvalCtx::now();
    let fields = store.list_field_defs(None)?;
    let templates: Vec<TplInfo> = store
        .list_templates()?
        .into_iter()
        .map(|t| TplInfo { id: t.id, name: t.name, icon: t.icon })
        .collect();
    // streak 卡全历史 → 整页取 all（一份 log 全量内存集，同源不重复扫）
    let pool = store.list_items_unbounded(&crate::store::ListFilter {
        item_type: Some(ItemType::Log),
        ..Default::default()
    })?;

    let mut out = StatsSummary::default();

    // ---- 热力图：预设热力图挂件（day×count），window = days ----
    {
        let mut w = preset_heatmap_widget();
        w.window = Some(Window::Days { days });
        let r = eval_widget(
            VIEW_STATS_HEATMAP,
            VIEW_STATS_HEATMAP,
            &w,
            &pool,
            &ctx,
            Some(days),
            &fields,
            &templates,
        )?;
        if let Some(bs) = r.buckets {
            out.heatmap = bs
                .into_iter()
                .map(|b| crate::store::HeatDay { day: b.key, count: b.value as i64 })
                .collect();
        }
    }

    // ---- 打卡连续：全部 pinned log 模板各一张卡 ----
    for tpl in &templates {
        let cfg = streak_card_cfg(tpl, Some(Window::Days { days }), Some(Goal::Daily { daily: 1 }));
        let r = eval_widget(
            VIEW_STATS_STREAKS,
            &tpl.id,
            &cfg,
            &pool,
            &ctx,
            Some(days),
            &fields,
            &templates,
        )?;
        let s = r.derived.streak.unwrap_or_default();
        out.streaks.push(crate::store::TplStreak {
            template_id: tpl.id.clone(),
            name: tpl.name.clone(),
            icon: tpl.icon.clone(),
            current: s.current,
            longest: s.longest,
            recent: s.recent,
        });
    }

    // ---- number 字段趋势：每字段一条 values 线，窗口内 ≥ 2 点 ----
    for f in &fields {
        if f.kind != crate::model::FieldKind::Number {
            continue;
        }
        let cfg = series_line_cfg(f, Some(Window::Days { days }));
        let r = eval_widget(
            VIEW_STATS_SERIES,
            &f.id,
            &cfg,
            &pool,
            &ctx,
            Some(days),
            &fields,
            &templates,
        )?;
        let Some(pts) = r.points else { continue };
        if pts.len() < 2 {
            continue;
        }
        out.series.push(crate::store::NumSeries {
            field_id: f.id.clone(),
            name: f.name.clone(),
            unit: r.unit,
            points: pts.into_iter().map(|p| (p.t, p.v)).collect(),
        });
    }
    Ok(out)
}

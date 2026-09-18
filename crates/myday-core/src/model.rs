//! 统一数据模型（SCHEMA-REDESIGN 定稿）。
//!
//! 一张 `items` 表统一 event / task / log：
//! - 结构化列只留查询、排序、索引需要的字段（时间与状态）；
//! - 类型专属、用户可管理的字段全部进 `extra` JSON，key = `field_defs.id`
//!   （改名零成本）；保留键见 [`crate::store::FILE_LINKS_KEY`]。
//! - 时间约定：全库 `*_at` 列统一 UTC RFC3339 字符串。
//!
//! 类型创建即定（自动识别一次性决定，保存后锁定，DB trigger 兜底）；
//! 三种类型不互相转换。log 的发生时间用 `occurred_at`，与 `created_at` 严格分离。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 条目类型。创建即定，不可互转。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemType {
    /// 日程：未来要发生的事（必须有 start_at / end_at）
    Event,
    /// 待办：需要完成的事（status 必填，默认 todo）
    Task,
    /// 记录：已发生的事（occurred_at 必填，默认 now）
    Log,
}

impl ItemType {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemType::Event => "event",
            ItemType::Task => "task",
            ItemType::Log => "log",
        }
    }

    /// ID 前缀：`evt_` / `tsk_` / `log_`
    pub fn id_prefix(self) -> &'static str {
        match self {
            ItemType::Event => "evt",
            ItemType::Task => "tsk",
            ItemType::Log => "log",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "event" | "e" => Some(ItemType::Event),
            "task" | "t" => Some(ItemType::Task),
            "log" | "l" => Some(ItemType::Log),
            _ => None,
        }
    }
}

impl std::fmt::Display for ItemType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 待办状态（只服务 task；event / log 强制 NULL）。
/// 只有两态：未完成（todo）/ 已完成（done）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Todo,
    Done,
}

impl ItemStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemStatus::Todo => "todo",
            ItemStatus::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(ItemStatus::Todo),
            "done" => Some(ItemStatus::Done),
            _ => None,
        }
    }

    /// 未完成集合：Today / Upcoming / All 视图与部分索引共用。
    pub const OPEN: [ItemStatus; 1] = [ItemStatus::Todo];
    /// 已结束集合：Done 视图。
    pub const CLOSED: [ItemStatus; 1] = [ItemStatus::Done];
}

/// 字段类型（Notion 属性类型的子集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    #[default]
    Text,
    Number,
    Select,
    MultiSelect,
    Bool,
    Date,
    Url,
}

impl FieldKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "text" => Some(FieldKind::Text),
            "number" | "num" => Some(FieldKind::Number),
            "select" => Some(FieldKind::Select),
            "multiselect" | "multi" => Some(FieldKind::MultiSelect),
            "bool" | "checkbox" | "check" => Some(FieldKind::Bool),
            "date" => Some(FieldKind::Date),
            "url" | "link" => Some(FieldKind::Url),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Number => "number",
            FieldKind::Select => "select",
            FieldKind::MultiSelect => "multiselect",
            FieldKind::Bool => "bool",
            FieldKind::Date => "date",
            FieldKind::Url => "url",
        }
    }
}

/// 字段定义。值存 `items.extra[field id]`，类型配置在 `options`
/// （如 number：`{"unit":"kg"}`，select：`{"choices":["低","中","高"]}`）。
/// 软删（deleted_at）后读取时忽略，不占用名字位，同名可重建。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldDef {
    pub id: String,
    pub name: String,
    pub kind: FieldKind,
    pub options: serde_json::Value,
    /// None = 全局（DB 存哨兵 'all'）；Some(t) = 仅该类型
    pub scope: Option<ItemType>,
    pub sort: i64,
    pub builtin: bool,
}

/// 附件（图片存文件目录，数据库只存相对路径）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: i64,
    pub item_id: String,
    /// 相对数据目录的路径，如 `attachments/<item_id>/<uuid>.png`
    pub rel_path: String,
    pub mime: String,
    pub size: u64,
    pub created_at: DateTime<Utc>,
}

/// 提醒（1:N）。存「意图」而非时刻：`spec` = @token（相对，随条目时间跟随）
/// 或 RFC3339（绝对一次性）。发生时刻由提醒环运行时展开并经 reminder_log 去重
/// （INTERACTION §6）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub item_id: String,
    /// @token（@start-1h / @due-1d / @dailyT09:00）或 RFC3339 绝对时刻
    pub spec: String,
    /// notify / sound / popup
    pub channel: String,
}

/// 新建提醒输入。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewReminder {
    pub spec: String,
    #[serde(default = "default_channel")]
    pub channel: String,
}

fn default_channel() -> String {
    "notify".into()
}

/// 模板 = 类型 + 列默认值 / 字段默认值（`defaults` 双命名空间）+ 随模板启用的字段。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub tag: Option<String>,
    /// emoji 图标（按钮 / chip 展示用）
    #[serde(default)]
    pub icon: Option<String>,
    /// 模板归属的条目类型
    pub item_type: ItemType,
    /// 默认值补丁。key 二选一：items 列白名单成员（写入对应列），
    /// 或 `field_defs.id`（写入 extra，按 kind 校验）。
    pub defaults: serde_json::Value,
    /// 随模板启用的字段定义（启用时一次性物化进 field_defs，builtin = 1）
    #[serde(default = "empty_fields")]
    pub fields: serde_json::Value,
    pub note: Option<String>,
    pub sort: i64,
    /// 记录页 / 面板按钮钉选
    #[serde(default = "default_pinned")]
    pub pinned: bool,
    pub builtin: bool,
}

fn default_pinned() -> bool {
    true
}

fn empty_fields() -> serde_json::Value {
    serde_json::json!([])
}

/// 条目——三类对象的统一模型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    /// 可空（log 允许只有 note）；展示用 `title ?? note 截断 ?? "无标题"`
    pub title: Option<String>,
    pub note: Option<String>,
    /// 日程（/待办）开始时间
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    /// 仅 event 有效
    pub all_day: bool,
    /// 待办截止时间
    pub due_at: Option<DateTime<Utc>>,
    pub due_all_day: bool,
    /// 记录发生时间（log 必填，与 created_at 严格分离）
    pub occurred_at: Option<DateTime<Utc>>,
    /// 仅 task 有值
    pub status: Option<ItemStatus>,
    pub completed_at: Option<DateTime<Utc>>,
    /// 重复规则（SPRINT-SPEC §2）：@daily / @weekly:n / @monthly:d；
    /// 仅 event / task 允许（DB CHECK 兜底）。None = 不重复。
    pub recurrence: Option<String>,
    pub template_id: Option<String>,
    pub reminders: Vec<Reminder>,
    pub tags: Vec<String>,
    pub attachments: Vec<Attachment>,
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// 类型扩展字段（JSON 对象）：key = field_defs.id（保留键除外）。
    /// 类型不可变，不存在"转换时保留"的问题。
    #[serde(default = "empty_extra")]
    pub extra: serde_json::Value,
}

/// 新建条目输入。`id` 由存储层生成；`item_type` 缺省时按内容自动识别
/// （§5.8：截止→task，开始→event，log 范围字段→log，裸文本→task）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewItem {
    #[serde(default)]
    pub item_type: Option<ItemType>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub start_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub due_all_day: bool,
    #[serde(default)]
    pub occurred_at: Option<DateTime<Utc>>,
    /// 仅 task 有意义；缺省 todo
    #[serde(default)]
    pub status: Option<ItemStatus>,
    /// 重复规则（@daily / @weekly:n / @monthly:d），None = 不重复
    #[serde(default)]
    pub recurrence: Option<String>,
    #[serde(default)]
    pub template_id: Option<String>,
    /// 模板时间占位 token 的解析锚点日（本地时区；缺省今天）。
    /// GUI 面板自行解析 token 后传绝对时间，一般用不到本字段；
    /// CLI / 一键记录走 core 解析时可指定（如日历场景）。
    #[serde(default)]
    pub anchor_day: Option<chrono::NaiveDate>,
    /// 绝对提醒时间集合；空 = 按设置自动补一条（未来时间才生效）
    #[serde(default)]
    pub reminders: Vec<NewReminder>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// 类型扩展字段（JSON 对象），key = field_defs.id，缺省空对象
    #[serde(default = "empty_extra")]
    pub extra: serde_json::Value,
}

/// 条目部分更新（None 表示不修改）。类型不可变，无类型变更入口。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemPatch {
    pub title: Option<String>,
    pub note: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub all_day: Option<bool>,
    pub due_at: Option<DateTime<Utc>>,
    pub due_all_day: Option<bool>,
    pub occurred_at: Option<DateTime<Utc>>,
    pub status: Option<ItemStatus>,
    /// 设置重复规则（Some 时须为合法 spec，解析校验在存储层）
    pub recurrence: Option<String>,
    /// 清除重复规则
    #[serde(default)]
    pub clear_recurrence: bool,
    pub tags: Option<Vec<String>>,
    /// 整体替换提醒集合（None = 不修改）
    pub reminders: Option<Vec<NewReminder>>,
    /// 删除该条目的全部提醒
    #[serde(default)]
    pub clear_reminders: bool,
    /// 清空开始时间（event 不可清，CHECK 兜底拒绝）
    #[serde(default)]
    pub clear_start_at: bool,
    #[serde(default)]
    pub clear_end_at: bool,
    #[serde(default)]
    pub clear_due_at: bool,
    /// 扩展字段：整对象替换（不是按键合并）
    #[serde(default)]
    pub extra: Option<serde_json::Value>,
}

/// 列表过滤条件。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListFilter {
    pub item_type: Option<ItemType>,
    /// 日期区间（含边界）。锚点列随类型：log → occurred_at，其余 → start_at。
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// 列级区间过滤（与 from/to 正交，可任意组合；NULL 列自然不命中）。
    /// 日历「和某天相关的所有条目」请用 [`crate::store::Store::list_items_window`]（OR 语义）。
    #[serde(default)]
    pub start_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub start_to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub end_to: Option<DateTime<Utc>>,
    /// due_at 字段区间过滤（与 from/to 正交，可组合）
    #[serde(default)]
    pub due_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub due_to: Option<DateTime<Utc>>,
    #[serde(default)]
    pub occurred_from: Option<DateTime<Utc>>,
    #[serde(default)]
    pub occurred_to: Option<DateTime<Utc>>,
    /// 待办：按状态过滤
    pub status: Option<ItemStatus>,
    /// 活动过滤：该本地日期创建或修改过的条目（今天视图"活动"区）
    #[serde(default)]
    pub changed_on: Option<chrono::NaiveDate>,
    pub tag: Option<String>,
    pub limit: Option<i64>,
    /// 所有非 Option 字段必须带 default：前端按需传参，缺省即默认行为
    #[serde(default)]
    pub offset: i64,
    /// 排序方向，默认倒序
    #[serde(default)]
    pub order: crate::store::ListOrder,
}

/// 搜索命中。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub item: Item,
    /// 命中位置：title / note / tag / field
    pub matched_in: Vec<String>,
}

/// 提醒中心历史条目（SPRINT2-SPEC §5）：一次已处理 occurrence + 所属条目快照。
/// 条目删除时 reminders / reminder_log 级联消失，历史天然只含现存条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderHistoryEntry {
    /// 该次提醒的原定时刻
    pub remind_at: DateTime<Utc>,
    pub item: Item,
}

/// 展示标题：title ?? note 截断(40 字) ?? "无标题"（§5.7）。
pub fn display_title(item: &Item) -> String {
    if let Some(t) = item.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if let Some(n) = item.note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        let truncated: String = n.chars().take(40).collect();
        return truncated;
    }
    "无标题".into()
}

/// 生成带类型前缀的短 ID，如 `evt_1a2b3c4d`。
pub fn new_id(item_type: ItemType) -> String {
    let uid = uuid::Uuid::new_v4().simple().to_string();
    format!("{}_{}", item_type.id_prefix(), &uid[..8])
}

fn empty_extra() -> serde_json::Value {
    serde_json::json!({})
}

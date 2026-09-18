//! SQLite 存储层（SCHEMA-REDESIGN 定稿）。
//!
//! - WAL + 外键 + busy_timeout，写入使用事务
//! - 迁移（1.0 数据承诺）：v4 起只走 MIGRATIONS 附加式迁移链，缺步骤 = 报错
//!   拒绝打开，绝不重建丢数据；更旧的遗留库仍备份后 DROP 重建（仅 1.0 前）
//! - 9 条 CHECK + 类型不可变 trigger 下放 DB，绕过应用层也保证一致性
//! - 幂等键：`items.idempotency_key` 唯一索引，重复创建返回已有条目
//! - 提醒 1:N，触发去重靠 `reminder_log(reminder_id, remind_at)` 复合键；
//!   Snooze 只改 `reminders.remind_at`，旧日志保留
//! - 字段不常驻播种：种子只写一次（settings.seed_version 标记），
//!   用户删除（软删）的字段不会复活

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Duration, NaiveDate, SubsecRound, TimeZone, Timelike, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::attachment;
use crate::error::{MyDayError, Result};
use crate::model::*;
// ListFilter 定义在 model 中，此处公开重导出，调用方统一从 store 命名空间取
pub use crate::model::ListFilter;

/// 文件链接保留键：extra 中唯一不要求是字段 id 的键（值 = 路径字符串数组）。
pub const FILE_LINKS_KEY: &str = "文件";

/// 重复待办完成记账保留键（SPRINT-SPEC §2.3）：值 = 完成前的 due_at（RFC3339）。
/// 供取消完成回拨到完成前的那一期；系统写，不经字段校验。
pub const RECURRED_DONE_KEY: &str = "recurred_done_at";

/// 模板 defaults 可写的 items 列白名单（§2.6 双命名空间之一）。
/// 时间列的值只能为 @ 时间占位 token（tpltime 文法）——模板存意图，条目存事实。
/// 无 status：新建待办恒为未完成，模板不带状态默认。
const COLUMN_DEFAULT_KEYS: &[&str] = &[
    "title",
    "note",
    "start_at",
    "end_at",
    "all_day",
    "due_at",
    "due_all_day",
    "occurred_at",
];

/// 列白名单（CLI / 前端判断模板 defaults 键归属用）。
pub fn column_default_keys() -> &'static [&'static str] {
    COLUMN_DEFAULT_KEYS
}

/// 当前库结构版本。v5：view_defs 视图模型表（FILTER-SPEC §7，附加式迁移：
/// v4 库只补建表 + 种子，不重建、不动数据）。更旧结构仍走备份重建。
const SCHEMA_VERSION: i64 = 5;

/// 迁移基线：v4 = 「只追加迁移，永不重建」政策的起点（1.0 数据承诺）。
/// 基线及以上的库升级只允许走 MIGRATIONS 链；DROP 重建只属于更旧的遗留库。
const MIGRATION_BASELINE: i64 = 4;

/// 附加式迁移链：每项 `(from_version, sql)` 把库从 from_version 升一级。
/// 发布后每次递增 SCHEMA_VERSION 必须在此追加一项；缺项时 init 报错拒绝打开
/// ——宁可打不开，也不静默重建丢数据。SQL 要求幂等（迁移中途崩溃可重开重放）。
const MIGRATIONS: &[(i64, &str)] = &[
    // v4 → v5（1.0 基线）：view_defs 表 + 内置视图种子（IF NOT EXISTS，幂等）
    (4, crate::view::VIEW_DEFS_SQL),
];

/// 内置种子版本（fd_priority + 3 个内置模板）。未来内置内容变更时递增触发升级。
/// 内置内容版本：v2 = 模板精简为通用的 喝水/体重（emoji 图标 + 数字字段单位）。
/// 升级只按 id 覆盖内置模板行；用户自建/改名/删除一律不动。
const BUILTIN_SEED_VERSION: i64 = 2;

const SCHEMA_SQL: &str = r#"
CREATE TABLE items (
  id              TEXT PRIMARY KEY,
  type            TEXT NOT NULL CHECK (type IN ('event','task','log')),

  title           TEXT,
  note            TEXT,

  start_at        TEXT,
  end_at          TEXT,
  all_day         INTEGER NOT NULL DEFAULT 0,

  due_at          TEXT,
  due_all_day     INTEGER NOT NULL DEFAULT 0,

  occurred_at     TEXT,

  status          TEXT,
  completed_at    TEXT,

  -- 重复规则（SPRINT-SPEC §2）：@daily / @weekly:n / @monthly:d（仅 event / task，CHECK 见下）
  recurrence      TEXT,

  template_id     TEXT,
  idempotency_key TEXT UNIQUE,

  extra           TEXT NOT NULL DEFAULT '{}',
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL,

  CHECK (type != 'event' OR (start_at IS NOT NULL AND end_at IS NOT NULL AND end_at >= start_at)),
  CHECK (type != 'event' OR (status IS NULL AND occurred_at IS NULL AND completed_at IS NULL AND due_at IS NULL)),
  CHECK (type != 'task' OR status IN ('todo','done')),
  CHECK (type != 'task' OR occurred_at IS NULL),
  CHECK (type != 'task' OR status != 'done' OR completed_at IS NOT NULL),
  CHECK (type != 'task' OR status =  'done' OR completed_at IS NULL),
  CHECK (type != 'log'  OR occurred_at IS NOT NULL),
  CHECK (type != 'log'  OR (status IS NULL AND completed_at IS NULL AND start_at IS NULL AND end_at IS NULL AND due_at IS NULL)),
  CHECK (title IS NOT NULL OR note IS NOT NULL),
  CHECK (recurrence IS NULL OR type IN ('event','task'))
);

-- 类型创建即定：UPDATE 改 type 一律拒绝（CHECK 无法跨行比较 OLD/NEW）
CREATE TRIGGER trg_items_type_immutable
BEFORE UPDATE OF type ON items
WHEN OLD.type != NEW.type
BEGIN
  SELECT RAISE(ABORT, 'items.type is immutable');
END;

CREATE TABLE tags (
  id   INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE item_tags (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_id)
);

-- 提醒存意图：spec = @token（相对，随条目时间跟随）或 RFC3339（绝对一次性）；
-- 发生时刻由提醒环运行时展开（reminder.rs），reminder_log 按 (reminder_id, 解析时刻) 去重
CREATE TABLE reminders (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id   TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  spec      TEXT NOT NULL,
  channel   TEXT NOT NULL DEFAULT 'notify' CHECK (channel IN ('notify','sound','popup'))
);

CREATE TABLE reminder_log (
  reminder_id INTEGER NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
  remind_at   TEXT NOT NULL,
  sent_at     TEXT NOT NULL,
  PRIMARY KEY (reminder_id, remind_at)
);

CREATE TABLE attachments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  rel_path   TEXT NOT NULL,
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE field_defs (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  kind       TEXT NOT NULL CHECK (kind IN ('text','number','select','multiselect','bool','date','url')),
  options    TEXT NOT NULL DEFAULT '{}',
  scope      TEXT NOT NULL DEFAULT 'all' CHECK (scope IN ('all','event','task','log')),
  sort       INTEGER NOT NULL DEFAULT 0,
  builtin    INTEGER NOT NULL DEFAULT 0,
  deleted_at TEXT
);

-- 部分唯一索引：软删字段不占名字位，同名可重建；活跃字段仍唯一
CREATE UNIQUE INDEX idx_field_defs_name_scope
  ON field_defs(name, scope) WHERE deleted_at IS NULL;

CREATE TABLE templates (
  id        TEXT PRIMARY KEY,
  name      TEXT NOT NULL UNIQUE,
  item_type TEXT NOT NULL CHECK (item_type IN ('event','task','log')),
  icon      TEXT,
  tag       TEXT,
  note      TEXT,
  defaults  TEXT NOT NULL DEFAULT '{}',
  fields    TEXT NOT NULL DEFAULT '[]',
  sort      INTEGER NOT NULL DEFAULT 0,
  pinned    INTEGER NOT NULL DEFAULT 0,
  builtin   INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE INDEX idx_items_type_start     ON items(type, start_at);
CREATE INDEX idx_items_type_due       ON items(type, due_at);
CREATE INDEX idx_items_type_occurred  ON items(type, occurred_at);
CREATE INDEX idx_items_type_status    ON items(type, status);
CREATE INDEX idx_items_end_at         ON items(end_at);

-- status 集合须与 §5.2 视图查询（未完成 = status = 'todo'）保持一致
CREATE INDEX idx_items_task_open ON items(due_at)
  WHERE type = 'task' AND status = 'todo';

CREATE INDEX idx_item_tags_tag  ON item_tags(tag_id);
CREATE INDEX idx_attach_item    ON attachments(item_id);
CREATE INDEX idx_reminders_item ON reminders(item_id);

CREATE TABLE view_defs (
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
"#;

/// 内置字段种子：只有优先级一条（对齐需求 §7.1）。
const SEED_FIELD_DEF: (&str, &str, FieldKind, &str, &str) =
    ("fd_priority", "优先级", FieldKind::Select, r#"{"choices":["低","中","高"]}"#, "task");

/// 内置模板种子（§4）：最少、最通用——不做健康记录的用户也用得上，
/// 避免为了清场删一堆内置项。均 pinned = 1，跳过引导的用户记录页也有入口。
/// `defaults` 的 key 只用列白名单（title）——字段值由用户现场填写。
const SEED_TEMPLATES: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "tpl_water",
        "喝水",
        "log",
        "💧",
        r#"{"title":"喝水"}"#,
        r#"[
          {"id":"fd_water_cup","name":"杯数","kind":"number","scope":"log",
           "options":{"unit":"杯"}}
        ]"#,
    ),
    (
        "tpl_weight",
        "体重",
        "log",
        "⚖️",
        r#"{"title":"体重"}"#,
        r#"[
          {"id":"fd_weight_kg","name":"体重","kind":"number","scope":"log",
           "options":{"unit":"kg"}}
        ]"#,
    ),
];

/// 排序方向，默认倒序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ListOrder {
    Asc,
    #[default]
    Desc,
}

/// 统计（SPRINT-SPEC §6）：热力图单日计数。
#[derive(Debug, Clone, serde::Serialize)]
pub struct HeatDay {
    /// 本地日 `YYYY-MM-DD`
    pub day: String,
    pub count: i64,
}

/// pinned log 模板的打卡连续性。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TplStreak {
    pub template_id: String,
    pub name: String,
    pub icon: Option<String>,
    /// 当前连续天数（今天或昨天起算的自然日连续）
    pub current: i64,
    /// 历史最长连续
    pub longest: i64,
    /// 统计范围内的次数（随 stats_summary 的 days 窗口）
    pub recent: i64,
}

/// number 字段的取值序列（近 90 天，升序）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct NumSeries {
    pub field_id: String,
    pub name: String,
    pub unit: Option<String>,
    /// (occurred_at RFC3339, value)
    pub points: Vec<(String, f64)>,
}

/// 一次调用取全（IPC `stats_summary` / CLI `myday stats --json`）。
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct StatsSummary {
    /// 近 365 天（含 0 的每一天）
    pub heatmap: Vec<HeatDay>,
    pub streaks: Vec<TplStreak>,
    pub series: Vec<NumSeries>,
}

/// 待办标准视图（§5.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskView {
    /// 今天（含逾期）
    Today,
    /// 即将到期
    Upcoming,
    /// 全部未完成（todo）
    All,
    /// 已完成
    Done,
}

/// 线程安全的 SQLite 存储。GUI / CLI / 测试共用。
pub struct Store {
    conn: Mutex<Connection>,
    root: PathBuf,
}

impl Store {
    /// 打开默认位置（`~/.local/share/myday/myday.db`）。
    pub fn open_default() -> Result<Self> {
        let root = crate::paths::data_dir()?;
        Self::open(&root.join("myday.db"), &root)
    }

    /// 在指定路径打开数据库，`root` 为数据根目录（附件、备份所在）。
    /// 旧结构库：先在 `<root>/backups/` 备份一次，再 DROP 重建（不考虑历史数据）。
    pub fn open(db_path: &Path, root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root)?;
        let conn = Connection::open(db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_millis(3_000))?;
        let store = Store {
            conn: Mutex::new(conn),
            root: root.to_path_buf(),
        };
        store.init()?;
        Ok(store)
    }

    /// 数据根目录（整体备份 = 复制此目录）。
    pub fn data_root(&self) -> &Path {
        &self.root
    }

    /// 测试与维护用途：直接访问底层连接。
    #[doc(hidden)]
    pub fn raw_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.lock()
    }

    fn init(&self) -> Result<()> {
        let conn = self.lock()?;
        let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let items_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'items'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)?;
        if items_exists && version >= MIGRATION_BASELINE {
            // v4+ 结构：沿迁移链逐级升级，缺迁移步骤 = 报错拒绝打开（保护数据），
            // 不重建、不动既有数据。
            while version < SCHEMA_VERSION {
                let step = MIGRATIONS
                    .iter()
                    .find(|(from, _)| *from == version)
                    .ok_or_else(|| {
                        MyDayError::Internal(format!(
                            "数据库结构 v{version} 缺少升到 v{} 的迁移步骤，为保护数据拒绝打开（开发时需补充 MIGRATIONS）",
                            version + 1
                        ))
                    })?;
                conn.execute_batch(step.1)?;
                version += 1;
                conn.pragma_update(None, "user_version", version)?;
            }
            // 「完成待办自动写记录」功能已删除：清掉历史遗留的影子记录（幂等，
            // 来源待办记在 note，是影子条目的可靠标识；标签随行级联删除）
            conn.execute(
                "DELETE FROM items WHERE type = 'log' AND note LIKE '来源待办 %'",
                [],
            )?;
            // 内置视图种子：升级/每次打开 upsert 覆盖 config 列，config_user 永不触碰
            crate::view::seed_view_defs(&conn)?;
            drop(conn);
            // 统计页预置容器：一次性播种（普通容器，可删可恢复）
            self.seed_stats_presets()?;
            return Ok(());
        }
        if items_exists {
            Self::backup_legacy(&conn, &self.root)?;
            for t in [
                "items",
                "item_tags",
                "tags",
                "reminders",
                "reminder_log",
                "attachments",
                "templates",
                "field_defs",
                "settings",
                "view_defs",
            ] {
                conn.execute_batch(&format!("DROP TABLE IF EXISTS {t}"))?;
            }
        }
        conn.execute_batch(SCHEMA_SQL)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Self::seed_builtin(&conn)?;
        crate::view::seed_view_defs(&conn)?;
        drop(conn);
        self.seed_stats_presets()?;
        Ok(())
    }

    /// 旧库一次性备份（§8.1）。VACUUM INTO 生成一致快照，失败不阻断重建。
    fn backup_legacy(conn: &Connection, root: &Path) -> Result<()> {
        let dir = root.join("backups");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("myday: 备份目录创建失败，跳过旧库备份: {e}");
            return Ok(());
        }
        let target = dir.join(format!("myday-legacy-{}.db", Utc::now().timestamp()));
        let quoted = target.to_string_lossy().replace('\'', "''");
        if let Err(e) = conn.execute_batch(&format!("VACUUM INTO '{quoted}'")) {
            eprintln!("myday: 旧库备份失败（继续重建）: {e}");
        } else {
            eprintln!("myday: 旧库已备份到 {}", target.display());
        }
        Ok(())
    }

    /// 种子：只播种一次（settings.seed_version 标记），用户删除的字段不复活。
    fn seed_builtin(conn: &Connection) -> Result<()> {
        let stored: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'seed_version'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let version = stored.and_then(|v| v.parse::<i64>().ok());
        match version {
            Some(v) if v >= BUILTIN_SEED_VERSION => return Ok(()),
            Some(_) => {
                // 内置内容升级：按 id 覆盖（不动 deleted_at，不复活已删模板）
                Self::upgrade_builtin_templates(conn)?;
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('seed_version', ?1) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![BUILTIN_SEED_VERSION.to_string()],
                )?;
                return Ok(());
            }
            None => {}
        }
        let (id, name, kind, options, scope) = SEED_FIELD_DEF;
        conn.execute(
            "INSERT OR IGNORE INTO field_defs (id, name, kind, options, scope, sort, builtin)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 1)",
            params![id, name, kind.as_str(), options, scope],
        )?;
        for (i, (id, tpl_name, item_type, icon, defaults, fields)) in
            SEED_TEMPLATES.iter().enumerate()
        {
            conn.execute(
                "INSERT OR IGNORE INTO templates
                    (id, name, item_type, icon, tag, note, defaults, fields, sort, pinned, builtin)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, ?7, 1, 1)",
                params![id, tpl_name, *item_type, icon, *defaults, *fields, i as i64],
            )?;
            Self::materialize_template_fields(conn, fields)?;
        }
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('seed_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![BUILTIN_SEED_VERSION.to_string()],
        )?;
        Ok(())
    }

    /// 升级内置模板：行存在且 builtin = 1 才覆盖；字段按 id 覆盖活跃定义，
    /// 撞名（部分唯一索引拒绝）跳过并告警，不动 deleted_at。
    fn upgrade_builtin_templates(conn: &Connection) -> Result<()> {
        for (id, tpl_name, item_type, icon, defaults, fields) in SEED_TEMPLATES {
            let n = conn.execute(
                "UPDATE templates SET name = ?2, item_type = ?3, icon = ?4,
                                        defaults = ?5, fields = ?6
                 WHERE id = ?1 AND builtin = 1",
                params![id, tpl_name, *item_type, icon, *defaults, *fields],
            )?;
            if n == 0 {
                continue; // 用户已删除该模板，不复活
            }
            Self::materialize_template_fields(conn, fields)?;
        }
        Ok(())
    }

    /// 将模板 `fields` 数组一次性物化进 field_defs（builtin = 1）。
    /// id 已存在（含软删）或活跃撞名 → 跳过并告警，不回写、不复活。
    fn materialize_template_fields(conn: &Connection, fields: &str) -> Result<()> {
        let parsed: serde_json::Value = serde_json::from_str(fields)
            .map_err(|e| MyDayError::Internal(format!("template fields JSON 无效: {e}")))?;
        let Some(list) = parsed.as_array() else {
            return Ok(());
        };
        for f in list {
            let Some(id) = f.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(name) = f.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(kind) = f.get("kind").and_then(|v| v.as_str()).and_then(FieldKind::parse)
            else {
                continue;
            };
            let options = f.get("options").cloned().unwrap_or(serde_json::json!({}));
            let scope = f
                .get("scope")
                .and_then(|v| v.as_str())
                .and_then(ItemType::parse)
                .map(|t| t.as_str())
                .unwrap_or("all");
            let n = conn.execute(
                "INSERT OR IGNORE INTO field_defs (id, name, kind, options, scope, sort, builtin)
                 VALUES (?1, ?2, ?3, ?4, ?5,
                         (SELECT COALESCE(MAX(sort), 0) + 1 FROM field_defs), 1)",
                params![id, name, kind.as_str(), options.to_string(), scope],
            )?;
            if n == 0 {
                eprintln!("myday: 模板字段 {id}({name}) 已存在或撞名，跳过物化");
            }
        }
        Ok(())
    }

    pub(crate) fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| MyDayError::Internal("storage mutex poisoned".into()))
    }

    // ------------------------------------------------------------------
    // 创建
    // ------------------------------------------------------------------

    /// 新建条目。类型缺省时自动识别（§5.8：截止→task、开始→event、
    /// log 范围字段→log、裸文本→task）。
    ///
    /// 模板 defaults 双命名空间：列白名单成员写入对应列（显式传参优先；
    /// 时间列可为 `@` 占位 token，按 anchor_day（缺省今天）解析），
    /// 字段 id 写入 extra。幂等：idempotency_key 命中直接返回已有条目。
    pub fn add_item(&self, mut new: NewItem) -> Result<Item> {
        if let Some(key) = new.idempotency_key.as_deref() {
            if !key.is_empty() {
                if let Some(existing) = self.find_by_idempotency_key(key)? {
                    return Ok(existing);
                }
            }
        }

        // 统一截断到秒：保证数据库往返后与内存值相等
        let now = Utc::now().trunc_subsecs(0);
        let conn = self.lock()?;

        // 模板 defaults 铺底（列 + 字段 id），显式传参 / extra 覆盖
        let mut extra = serde_json::Map::new();
        let mut tags = new.tags.clone();
        let mut tpl_type: Option<ItemType> = None;
        if let Some(tpl) = new
            .template_id
            .as_deref()
            .and_then(|id| template_by_name_or_id(&conn, id).ok().flatten())
        {
            tpl_type = Some(tpl.item_type);
            if let Some(obj) = tpl.defaults.as_object() {
                // 时间键固定顺序解析（end 的 @start 基准依赖 start），显式传参优先
                let anchor = new
                    .anchor_day
                    .unwrap_or_else(|| now.with_timezone(&chrono::Local).date_naive());
                let mut resolved_start: Option<DateTime<Utc>> = None;
                for key in ["start_at", "end_at", "due_at", "occurred_at"] {
                    let Some(v) = obj.get(key) else {
                        continue;
                    };
                    let target = match key {
                        "start_at" => &mut new.start_at,
                        "end_at" => &mut new.end_at,
                        "due_at" => &mut new.due_at,
                        _ => &mut new.occurred_at,
                    };
                    if target.is_some() {
                        if key == "start_at" {
                            resolved_start = new.start_at;
                        }
                        continue;
                    }
                    if let Some(t) = resolve_template_token(v, now, anchor, resolved_start) {
                        *target = Some(t);
                    }
                    if key == "start_at" {
                        resolved_start = new.start_at;
                    }
                }
                for (k, v) in obj {
                    // 注意：守卫必须写进分支体而不是 match 守卫——match 守卫不满足时
                    // 会落入 other 分支，把列键当字段 id 塞进 extra（[INVALID] 报错）
                    match k.as_str() {
                        "title" => {
                            if new.title.is_none() {
                                new.title = v.as_str().map(str::to_string);
                            }
                        }
                        "note" => {
                            if new.note.is_none() {
                                new.note = v.as_str().map(str::to_string);
                            }
                        }
                        "all_day" => {
                            if !new.all_day {
                                new.all_day = v.as_bool().unwrap_or(false);
                            }
                        }
                        "due_all_day" => {
                            if !new.due_all_day {
                                new.due_all_day = v.as_bool().unwrap_or(false);
                            }
                        }
                        "start_at" | "end_at" | "due_at" | "occurred_at" => {
                            // 已按固定顺序解析过（@start 依赖），此处跳过
                        }
                        other => {
                            // 字段 id → extra 铺底；未知键忽略（保存模板时已校验）
                            extra.insert(other.to_string(), v.clone());
                        }
                    }
                }
            }
            if let Some(tag) = tpl.tag.as_deref() {
                if !tag.is_empty() && !tags.iter().any(|t| t == tag) {
                    tags.push(tag.to_string());
                }
            }
        }
        if let Some(map) = new.extra.as_object() {
            for (k, v) in map {
                extra.insert(k.clone(), v.clone());
            }
        }
        new.extra = serde_json::Value::Object(extra.clone());

        let item_type = match new.item_type.or(tpl_type) {
            // §5.8 规则 1：模板自带 item_type 即类型（显式传参仍最优先）
            Some(t) => t,
            None => Self::infer_type(&conn, new.due_at.is_some(), new.start_at.is_some(), &new.extra)?,
        };
        new.item_type = Some(item_type);

        Self::normalize_new(&mut new, item_type, now)?;
        Self::validate_extra(&conn, &extra, item_type)?;

        // 重复规则（SPRINT-SPEC §2.1）：仅 event / task；严格解析；锚点字段必须就位
        let recurrence: Option<String> = match new.recurrence.as_deref() {
            None => None,
            Some(spec) => {
                if item_type == ItemType::Log {
                    return Err(MyDayError::Invalid("记录不支持重复规则".into()));
                }
                let rec = crate::recurrence::Recurrence::parse(spec)?;
                if item_type == ItemType::Task && new.due_at.is_none() {
                    return Err(MyDayError::Invalid("重复待办必须带截止时间".into()));
                }
                Some(rec.as_str())
            }
        };

        let title = new.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
        let note = new.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
        if (title.is_none() && note.is_none())
            || (item_type != ItemType::Log && title.is_none())
        {
            return Err(MyDayError::Invalid(
                "标题不能为空（记录类至少需要标题或备注之一）".into(),
            ));
        }

        // status：仅 task；缺省 todo；done 自动写 completed_at
        let status = match item_type {
            ItemType::Task => {
                let s = new.status.unwrap_or(ItemStatus::Todo);
                Some(s)
            }
            _ => {
                if new.status.is_some() {
                    return Err(MyDayError::Invalid("只有待办有状态".into()));
                }
                None
            }
        };
        let completed_at = match status {
            Some(ItemStatus::Done) => Some(now),
            _ => None,
        };

        // 提醒：显式集合优先；否则按设置补一条默认提醒（未来时间才生效）
        let mut reminders = new.reminders.clone();
        for r in &reminders {
            validate_reminder_spec(&r.spec)?;
        }
        if reminders.is_empty() {
            let minutes = get_setting_on(&conn, "default_reminder_minutes")
                .ok()
                .flatten()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(10);
            if minutes > 0 {
                // 自动默认提醒只锚「开始」：日程/带开始的待办提前提醒；
                // 不按截止算（截止是完成期限，不是发生时刻），log 不自动提醒
                let anchor = match item_type {
                    ItemType::Event | ItemType::Task => new.start_at,
                    ItemType::Log => None,
                };
                if let Some(at) = anchor {
                    if at - Duration::minutes(minutes) > now {
                        reminders.push(NewReminder {
                            spec: format!("@start-{minutes}m"),
                            channel: "notify".into(),
                        });
                    }
                }
            }
        }

        let id = new_id(item_type);
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO items (id, type, title, note, start_at, end_at, all_day,
                                due_at, due_all_day, occurred_at, status, completed_at,
                                recurrence, template_id, idempotency_key, extra, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?17)",
            params![
                id,
                item_type.as_str(),
                title,
                note,
                opt_dt(new.start_at),
                opt_dt(new.end_at),
                if item_type == ItemType::Event { new.all_day } else { false } as i64,
                opt_dt(new.due_at),
                if item_type == ItemType::Task { new.due_all_day } else { false } as i64,
                opt_dt(new.occurred_at),
                status.map(|s| s.as_str()),
                opt_dt(completed_at),
                recurrence,
                new.template_id,
                empty_to_none(new.idempotency_key.as_deref()),
                serde_json::to_string(&serde_json::Value::Object(extra))
                    .unwrap_or_else(|_| "{}".into()),
                dt(now),
            ],
        )?;
        self.attach_tags(&tx, &id, &tags)?;
        for r in &reminders {
            tx.execute(
                "INSERT INTO reminders (item_id, spec, channel) VALUES (?1, ?2, ?3)",
                params![id, r.spec, r.channel],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_item(&id)
    }

    /// 类型自动识别（§5.8 规则 2-5；规则 1 显式指定由调用方短路）。
    /// 必须在未持有锁或已持锁的调用方中以 `&Connection` 传入（Mutex 不可重入）。
    fn infer_type(
        conn: &Connection,
        has_due: bool,
        has_start: bool,
        extra: &serde_json::Value,
    ) -> Result<ItemType> {
        if has_due {
            return Ok(ItemType::Task); // 截止语义优先于开始
        }
        if has_start {
            return Ok(ItemType::Event);
        }
        if let Some(map) = extra.as_object() {
            for k in map.keys() {
                if k == FILE_LINKS_KEY {
                    continue;
                }
                if let Ok(def) = field_def_by_id(conn, k) {
                    if def.scope == Some(ItemType::Log) {
                        return Ok(ItemType::Log);
                    }
                }
            }
        }
        Ok(ItemType::Task) // 裸文本默认为待办
    }

    /// 类型相关规范化与校验（§5.1 写入规则，宁可报错不猜测）。
    fn normalize_new(new: &mut NewItem, item_type: ItemType, now: DateTime<Utc>) -> Result<()> {
        new.start_at = new.start_at.map(|t| t.trunc_subsecs(0));
        new.end_at = new.end_at.map(|t| t.trunc_subsecs(0));
        new.due_at = new.due_at.map(|t| t.trunc_subsecs(0));
        new.occurred_at = new.occurred_at.map(|t| t.trunc_subsecs(0));
        match item_type {
            ItemType::Event => {
                if new.due_at.is_some() {
                    return Err(MyDayError::Invalid("日程不允许截止时间；同时有开始与截止请用待办".into()));
                }
                let Some(start) = new.start_at else {
                    return Err(MyDayError::Invalid("日程必须带开始时间".into()));
                };
                if new.end_at.is_none() {
                    new.end_at = Some(start + Duration::hours(1));
                }
                if new.end_at.unwrap() <= start {
                    return Err(MyDayError::Invalid("结束时间需晚于开始时间".into()));
                }
                new.occurred_at = None;
            }
            ItemType::Task => {
                if new.occurred_at.is_some() {
                    return Err(MyDayError::Invalid("待办没有发生时间".into()));
                }
            }
            ItemType::Log => {
                if new.start_at.is_some() || new.end_at.is_some() {
                    return Err(MyDayError::Invalid("记录没有开始/结束时间；发生时间请填 occurred_at".into()));
                }
                if new.due_at.is_some() {
                    return Err(MyDayError::Invalid("记录没有截止时间".into()));
                }
                new.occurred_at = Some(new.occurred_at.unwrap_or(now));
                if new.occurred_at.unwrap() > now {
                    return Err(MyDayError::Invalid(
                        "记录的发生时间不能晚于现在；未来要做的事请改建日程或待办".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// extra 严格校验（§5.5 / §5.1）：key 必须是活跃字段 id（scope 匹配），
    /// 值按 kind 校验，不做自动纠偏。文件链接保留键除外。
    /// 同上：以 `&Connection` 传入，调用方不得在持锁时再走会加锁的公开方法。
    fn validate_extra(
        conn: &Connection,
        extra: &serde_json::Map<String, serde_json::Value>,
        item_type: ItemType,
    ) -> Result<()> {
        for (k, v) in extra {
            if k == FILE_LINKS_KEY {
                if v.is_null() {
                    continue;
                }
                let Some(list) = v.as_array() else {
                    return Err(MyDayError::Invalid(format!("extra[{FILE_LINKS_KEY}] 必须是路径数组")));
                };
                if list.iter().any(|p| !p.is_string()) {
                    return Err(MyDayError::Invalid(format!("extra[{FILE_LINKS_KEY}] 必须是路径数组")));
                }
                continue;
            }
            if k == RECURRED_DONE_KEY {
                // 系统记账键（重复待办完成回拨）：值须为 RFC3339 字符串
                let ok = v.as_str().map(parse_dt).flatten().is_some();
                if !ok {
                    return Err(MyDayError::Invalid(format!("extra[{RECURRED_DONE_KEY}] 必须是 RFC3339 时刻")));
                }
                continue;
            }
            let (def, active) = field_def_any(conn, k)
                .ok_or_else(|| {
                    MyDayError::Invalid(format!("extra 的键 \"{k}\" 不是已定义的字段 id"))
                })??;
            if !active {
                // 软删字段的历史值：不清理、不强校验，原样保留（§2.5）
                continue;
            }
            if def.scope.is_some_and(|s| s != item_type) {
                return Err(MyDayError::Invalid(format!(
                    "字段「{}」不适用于{}",
                    def.name,
                    item_type.as_str()
                )));
            }
            validate_value_by_kind(k, &def, v)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // 读取
    // ------------------------------------------------------------------

    pub fn get_item(&self, id: &str) -> Result<Item> {
        let conn = self.lock()?;
        conn.query_row("SELECT * FROM items WHERE id = ?1", params![id], item_mapper)
            .optional()?
            .ok_or_else(|| MyDayError::NotFound(format!("item {id} not found")))
            .and_then(|mut item| {
                Self::hydrate(&conn, &mut item)?;
                Ok(item)
            })
    }

    pub fn find_by_idempotency_key(&self, key: &str) -> Result<Option<Item>> {
        let conn = self.lock()?;
        let mut item = conn
            .query_row(
                "SELECT * FROM items WHERE idempotency_key = ?1",
                params![key],
                item_mapper,
            )
            .optional()?;
        if let Some(item) = item.as_mut() {
            Self::hydrate(&conn, item)?;
        }
        Ok(item)
    }

    /// 通用列表查询。时间过滤锚点随类型（§8）：log → occurred_at，
    /// 其余 → start_at；due_from/due_to 恒看 due_at。limit 缺省 500。
    pub fn list_items(&self, filter: &ListFilter) -> Result<Vec<Item>> {
        self.query_items(filter, Some(500))
    }

    /// 视图引擎预筛专用：limit = None 时**不加** LIMIT 子句——
    /// 预筛层绝不施加视图的 limit（FILTER-SPEC §10，limit 属排序后语义）。
    #[doc(hidden)]
    pub fn list_items_unbounded(&self, filter: &ListFilter) -> Result<Vec<Item>> {
        self.query_items(filter, None)
    }

    fn query_items(&self, filter: &ListFilter, default_limit: Option<i64>) -> Result<Vec<Item>> {
        let conn = self.lock()?;
        let mut sql = String::from("SELECT * FROM items WHERE 1=1");
        let mut dyn_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(t) = filter.item_type {
            sql.push_str(" AND type = ?");
            dyn_params.push(Box::new(t.as_str().to_string()));
        }
        if let Some(s) = filter.status {
            sql.push_str(" AND status = ?");
            dyn_params.push(Box::new(s.as_str().to_string()));
        }
        if let Some(tag) = filter.tag.as_deref() {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id \
                 WHERE it.item_id = items.id AND t.name = ?)",
            );
            dyn_params.push(Box::new(tag.to_string()));
        }
        let anchor = match filter.item_type {
            Some(ItemType::Log) => "occurred_at",
            _ => "start_at",
        };
        match (filter.from, filter.to) {
            (Some(from), Some(to)) => {
                sql.push_str(&format!(
                    " AND datetime({anchor}) >= datetime(?) AND datetime({anchor}) <= datetime(?)"
                ));
                dyn_params.push(Box::new(dt(from)));
                dyn_params.push(Box::new(dt(to)));
            }
            (Some(from), None) => {
                sql.push_str(&format!(" AND datetime({anchor}) >= datetime(?)"));
                dyn_params.push(Box::new(dt(from)));
            }
            (None, Some(to)) => {
                sql.push_str(&format!(" AND datetime({anchor}) <= datetime(?)"));
                dyn_params.push(Box::new(dt(to)));
            }
            (None, None) => {}
        }
        match (filter.due_from, filter.due_to) {
            (Some(from), Some(to)) => {
                sql.push_str(
                    " AND datetime(due_at) >= datetime(?) AND datetime(due_at) <= datetime(?)",
                );
                dyn_params.push(Box::new(dt(from)));
                dyn_params.push(Box::new(dt(to)));
            }
            (Some(from), None) => {
                sql.push_str(" AND datetime(due_at) >= datetime(?)");
                dyn_params.push(Box::new(dt(from)));
            }
            (None, Some(to)) => {
                sql.push_str(" AND datetime(due_at) <= datetime(?)");
                dyn_params.push(Box::new(dt(to)));
            }
            (None, None) => {}
        }
        // 其余内置时间列的列级区间（NULL 列不命中，无需按类型分守卫）
        for (col, from, to) in [
            ("start_at", filter.start_from, filter.start_to),
            ("end_at", filter.end_from, filter.end_to),
            ("occurred_at", filter.occurred_from, filter.occurred_to),
        ] {
            match (from, to) {
                (Some(f), Some(t)) => {
                    sql.push_str(&format!(
                        " AND datetime({col}) >= datetime(?) AND datetime({col}) <= datetime(?)"
                    ));
                    dyn_params.push(Box::new(dt(f)));
                    dyn_params.push(Box::new(dt(t)));
                }
                (Some(f), None) => {
                    sql.push_str(&format!(" AND datetime({col}) >= datetime(?)"));
                    dyn_params.push(Box::new(dt(f)));
                }
                (None, Some(t)) => {
                    sql.push_str(&format!(" AND datetime({col}) <= datetime(?)"));
                    dyn_params.push(Box::new(dt(t)));
                }
                (None, None) => {}
            }
        }
        // 活动过滤：created_at / updated_at 落在本地当日 [零点, 次日零点)
        if let Some(day) = filter.changed_on {
            let (from, to) = local_day_bounds_utc(day);
            sql.push_str(
                " AND ((datetime(created_at) >= datetime(?) AND datetime(created_at) < datetime(?)) \
                    OR (datetime(updated_at) >= datetime(?) AND datetime(updated_at) < datetime(?)))",
            );
            for b in [from.as_str(), to.as_str(), from.as_str(), to.as_str()] {
                dyn_params.push(Box::new(b.to_string()));
            }
        }
        let order_expr = match filter.item_type {
            Some(ItemType::Log) => "COALESCE(occurred_at, created_at)",
            Some(ItemType::Event) => "COALESCE(start_at, created_at)",
            Some(ItemType::Task) => "COALESCE(due_at, start_at, created_at)",
            None => "COALESCE(start_at, due_at, occurred_at, created_at)",
        };
        let dir = match filter.order {
            ListOrder::Asc => "ASC",
            ListOrder::Desc => "DESC",
        };
        sql.push_str(&format!(" ORDER BY {order_expr} {dir}"));
        match filter.limit.or(default_limit) {
            Some(limit) => {
                sql.push_str(" LIMIT ? OFFSET ?");
                dyn_params.push(Box::new(limit));
                dyn_params.push(Box::new(filter.offset));
            }
            None => {
                sql.push_str(" LIMIT -1 OFFSET ?");
                dyn_params.push(Box::new(filter.offset));
            }
        }

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(dyn_params.iter().map(|p| p.as_ref())),
            item_mapper,
        )?;
        let mut items = Vec::new();
        for item in rows {
            items.push(item?);
        }
        for item in items.iter_mut() {
            Self::hydrate(&conn, item)?;
        }
        Ok(items)
    }

    /// 窗口查询（日历）：与本地时间窗口 [from, to] 有时间关联的全部条目（OR 语义）。
    /// - 日程/带开始条目：[start_at, end_at] 区间与窗口相交（跨天事件两头都算）；
    /// - 待办：due_at 落入窗口；
    /// - 记录：occurred_at 落入窗口。
    /// NULL 列自然不命中，无需按类型分守卫。排序按 COALESCE 主时间升序。
    pub fn list_items_window(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        item_type: Option<ItemType>,
    ) -> Result<Vec<Item>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT * FROM items \
             WHERE (?3 IS NULL OR type = ?3) \
               AND ( \
                 (start_at IS NOT NULL AND datetime(start_at) < datetime(?2) \
                    AND datetime(COALESCE(end_at, start_at)) >= datetime(?1)) \
                 OR (datetime(due_at) >= datetime(?1) AND datetime(due_at) <= datetime(?2)) \
                 OR (datetime(occurred_at) >= datetime(?1) AND datetime(occurred_at) <= datetime(?2)) \
               ) \
             ORDER BY COALESCE(start_at, due_at, occurred_at, created_at) ASC",
        )?;
        let rows = stmt.query_map(
            params![from.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    to.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    item_type.map(|t| t.as_str())],
            item_mapper,
        )?;
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for item in items.iter_mut() {
            Self::hydrate(&conn, item)?;
        }
        Ok(items)
    }

    /// 待办四个标准视图（§5.2）。未完成 = todo；已完成 = done。
    pub fn tasks_view(&self, view: TaskView, today: Option<NaiveDate>) -> Result<Vec<Item>> {
        // 「今天」按本地时区取（UTC 会让东八区 16:00 后的本地今天查不到今天的待办）；
        // 显式传入的 today 也按本地日期解释
        let today = today.unwrap_or_else(|| chrono::Local::now().date_naive());
        let today_end = chrono::Local
            .from_local_datetime(&today.and_hms_opt(23, 59, 59).unwrap())
            .single()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);
        let conn = self.lock()?;
        let open = "('todo')";
        let closed = "('done')";
        // 截止缺省时退回 start_at 判断：带开始时间的待办同样有到期语义
        let (sql, bind): (&str, bool) = match view {
            TaskView::Today => (
                &format!(
                    "SELECT * FROM items WHERE type='task' AND status IN {open} \
                     AND ((due_at IS NOT NULL AND datetime(due_at) <= datetime(?1)) \
                       OR (due_at IS NULL AND (start_at IS NULL OR datetime(start_at) <= datetime(?1)))) \
                     ORDER BY COALESCE(due_at, start_at) IS NULL, COALESCE(due_at, start_at) ASC"
                ),
                true,
            ),
            TaskView::Upcoming => (
                &format!(
                    "SELECT * FROM items WHERE type='task' AND status IN {open} \
                     AND ((due_at IS NOT NULL AND datetime(due_at) > datetime(?1)) \
                       OR (due_at IS NULL AND start_at IS NOT NULL AND datetime(start_at) > datetime(?1))) \
                     ORDER BY COALESCE(due_at, start_at) ASC LIMIT 100"
                ),
                true,
            ),
            TaskView::All => (
                &format!(
                    "SELECT * FROM items WHERE type='task' AND status IN {open} \
                     ORDER BY due_at IS NULL, due_at ASC"
                ),
                false,
            ),
            TaskView::Done => (
                &format!(
                    "SELECT * FROM items WHERE type='task' AND status IN {closed} \
                     ORDER BY COALESCE(completed_at, updated_at) DESC LIMIT 200"
                ),
                false,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = if bind {
            stmt.query_map(params![dt(today_end)], item_mapper)?
        } else {
            stmt.query_map([], item_mapper)?
        };
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for item in items.iter_mut() {
            Self::hydrate(&conn, item)?;
        }
        Ok(items)
    }

    /// 全文搜索（§5.3）：标题 / 备注 / 字段名与字段值（extra 经 field_defs 关联）/
    /// 标签。字段名命中要求条目真的用了该字段，不会全表命中。
    pub fn search(&self, query: &str, item_type: Option<ItemType>) -> Result<Vec<SearchHit>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let like = format!("%{}%", q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT i.* FROM items i \
             WHERE (?2 IS NULL OR i.type = ?2) \
               AND ( \
                 i.title LIKE ?1 ESCAPE '\\' OR i.note LIKE ?1 ESCAPE '\\' \
                 OR EXISTS ( \
                   SELECT 1 FROM json_each(i.extra) e \
                   JOIN field_defs f ON f.id = e.key AND f.deleted_at IS NULL \
                   WHERE f.name LIKE ?1 ESCAPE '\\' OR CAST(e.value AS TEXT) LIKE ?1 ESCAPE '\\' \
                 ) \
                 OR EXISTS ( \
                   SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id \
                   WHERE it.item_id = i.id AND t.name LIKE ?1 ESCAPE '\\' \
                 ) \
               ) \
             ORDER BY i.updated_at DESC LIMIT 50",
        )?;
        let rows = stmt.query_map(params![like, item_type.map(|t| t.as_str())], item_mapper)?;
        let mut items = Vec::new();
        for item in rows {
            items.push(item?);
        }
        let mut hits = Vec::new();
        for mut item in items {
            Self::hydrate(&conn, &mut item)?;
            let q_lower = q.to_lowercase();
            let mut matched_in = Vec::new();
            if item
                .title
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains(&q_lower))
            {
                matched_in.push("title".into());
            }
            if item
                .note
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&q_lower))
            {
                matched_in.push("note".into());
            }
            if item.tags.iter().any(|t| t.to_lowercase().contains(&q_lower)) {
                matched_in.push("tag".into());
            }
            if let Some(map) = item.extra.as_object() {
                let field_hit = map.iter().any(|(k, v)| {
                    if k == FILE_LINKS_KEY {
                        return false;
                    }
                    let Ok(def) = field_def_by_id(&conn, k) else {
                        return false;
                    };
                    def.name.to_lowercase().contains(&q_lower)
                        || serde_json::to_string(v)
                            .map(|s| s.to_lowercase().contains(&q_lower))
                            .unwrap_or(false)
                });
                if field_hit {
                    matched_in.push("field".into());
                }
            }
            hits.push(SearchHit { item, matched_in });
        }
        Ok(hits)
    }

    // ------------------------------------------------------------------
    // 更新 / 删除
    // ------------------------------------------------------------------

    /// 部分更新。类型不可变（无入口；DB trigger 兜底）。
    /// status = done 自动写 completed_at，其他状态置 NULL（双向封闭）。
    /// 两条母条目不变量（FILTER-SPEC §3，P1 统一写路径）：
    /// - task + recurrence ⇒ due_at 必填（含 clear_due_at，对齐 complete_task 报错）；
    /// - 经 update_item 直改 status 的「取消」对齐 [`Store::uncomplete_task`]：
    ///   重复待办 done→todo 回拨到最近一次推进前的存储值（记账键消费即清）。
    pub fn update_item(&self, id: &str, patch: ItemPatch) -> Result<Item> {
        {
            let conn = self.lock()?;
            let (existing_type, existing_note, existing_due, existing_recurrence, existing_status, existing_extra): (
                String,
                Option<String>,
                Option<String>,
                Option<String>,
                Option<String>,
                String,
            ) = conn
                .query_row(
                    "SELECT type, note, due_at, recurrence, status, extra FROM items WHERE id = ?1",
                    params![id],
                    |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
                    },
                )
                .optional()?
                .ok_or_else(|| MyDayError::NotFound(format!("item {id} not found")))?;
            let item_type = ItemType::parse(&existing_type)
                .ok_or_else(|| MyDayError::Internal(format!("unknown type {existing_type}")))?;

            // 取消完成统一写路径：重复待办经 update_item 直改 todo 时，
            // 先走 uncomplete_task 的回拨（恢复最近一次推进前的存储值），
            // 再应用补丁中其余修改（status 已是 todo，从补丁中剥离）。
            if item_type == ItemType::Task
                && patch.status == Some(ItemStatus::Todo)
                && existing_status.as_deref() == Some("done")
                && existing_recurrence.is_some()
            {
                let has_ledger = serde_json::from_str::<serde_json::Value>(&existing_extra)
                    .ok()
                    .and_then(|v| v.get(RECURRED_DONE_KEY).cloned())
                    .is_some();
                if has_ledger {
                    drop(conn);
                    self.uncomplete_task(id)?;
                    let rest = ItemPatch { status: None, ..patch };
                    let baseline = ItemPatch::default();
                    let is_empty = serde_json::to_value(&rest).ok()
                        == serde_json::to_value(&baseline).ok();
                    if is_empty {
                        return self.get_item(id);
                    }
                    return self.update_item(id, rest);
                }
            }

            // 标题可空渲染（§5.7）：log 允许清标题，但 note 必须兜底；
            // event / task 标题必填
            if let Some(t) = patch.title.as_deref() {
                if t.trim().is_empty() {
                    let note_after = patch
                        .note
                        .as_deref()
                        .map(str::trim)
                        .filter(|n| !n.is_empty())
                        .or_else(|| {
                            if patch.note.is_none() {
                                existing_note
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|n| !n.is_empty())
                            } else {
                                None
                            }
                        });
                    if item_type != ItemType::Log || note_after.is_none() {
                        return Err(MyDayError::Invalid("标题与备注不能同时为空".into()));
                    }
                }
            }

            if patch.status.is_some() && item_type != ItemType::Task {
                return Err(MyDayError::Invalid("只有待办有状态".into()));
            }
            if let Some(occ) = patch.occurred_at {
                if item_type != ItemType::Log {
                    return Err(MyDayError::Invalid("只有记录有发生时间".into()));
                }
                if occ.trunc_subsecs(0) > Utc::now().trunc_subsecs(0) {
                    return Err(MyDayError::Invalid("记录的发生时间不能晚于现在".into()));
                }
            }
            if item_type == ItemType::Event {
                if patch.clear_start_at && patch.start_at.is_none() {
                    return Err(MyDayError::Invalid("日程必须带开始时间，不能清空".into()));
                }
                if patch.clear_end_at && patch.end_at.is_none() {
                    return Err(MyDayError::Invalid("日程必须带结束时间，不能清空".into()));
                }
            }
            if item_type == ItemType::Log
                && (patch.clear_start_at
                    || patch.clear_end_at
                    || patch.clear_due_at
                    || patch.start_at.is_some()
                    || patch.end_at.is_some()
                    || patch.due_at.is_some())
            {
                return Err(MyDayError::Invalid("记录没有开始/结束/截止时间".into()));
            }
            // 重复规则（SPRINT-SPEC §2.1）：仅 event / task；严格解析；待办须带截止
            if (patch.recurrence.is_some() || patch.clear_recurrence) && item_type == ItemType::Log {
                return Err(MyDayError::Invalid("记录不支持重复规则".into()));
            }
            // 母条目不变量（FILTER-SPEC §3）：行内 due 恒为当前期——
            // 重复待办清掉截止（且同一补丁不清规则）= 破坏不变量，拒绝。
            if item_type == ItemType::Task
                && existing_recurrence.is_some()
                && !patch.clear_recurrence
                && patch.recurrence.is_none()
                && patch.clear_due_at
            {
                return Err(MyDayError::Invalid("重复待办必须带截止时间".into()));
            }
            if let Some(spec) = patch.recurrence.as_deref() {
                crate::recurrence::Recurrence::parse(spec)?;
                if item_type == ItemType::Task {
                    let has_due =
                        patch.due_at.is_some() || (!patch.clear_due_at && existing_due.is_some());
                    if !has_due {
                        return Err(MyDayError::Invalid("重复待办必须带截止时间".into()));
                    }
                }
            }
            if let Some(extra) = &patch.extra {
                let Some(map) = extra.as_object() else {
                    return Err(MyDayError::Invalid("extra 必须是 JSON 对象".into()));
                };
                Self::validate_extra(&conn, map, item_type)?;
            }

            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE items SET
                    title        = COALESCE(?2, title),
                    note         = CASE WHEN ?3 THEN ?4 ELSE note END,
                    start_at     = CASE WHEN ?5 THEN ?6 WHEN ?20 THEN NULL ELSE start_at END,
                    end_at       = CASE WHEN ?7 THEN ?8 WHEN ?21 THEN NULL ELSE end_at END,
                    all_day      = COALESCE(?9, all_day),
                    due_at       = CASE WHEN ?10 THEN ?11 WHEN ?22 THEN NULL ELSE due_at END,
                    due_all_day  = COALESCE(?12, due_all_day),
                    occurred_at  = COALESCE(?13, occurred_at),
                    status       = COALESCE(?14, status),
                    completed_at = CASE WHEN ?15 THEN ?16 ELSE completed_at END,
                    updated_at   = ?17,
                    extra        = CASE WHEN ?18 THEN ?19 ELSE extra END,
                    recurrence   = CASE WHEN ?23 THEN ?24 WHEN ?25 THEN NULL ELSE recurrence END
                 WHERE id = ?1",
                params![
                    id,
                    patch.title.as_deref().map(str::trim).filter(|t| !t.is_empty()),
                    patch.note.is_some(),
                    patch.note.as_deref().map(str::trim).filter(|n| !n.is_empty()),
                    patch.start_at.is_some(),
                    opt_dt(patch.start_at),
                    patch.end_at.is_some(),
                    opt_dt(patch.end_at),
                    patch.all_day,
                    patch.due_at.is_some(),
                    opt_dt(patch.due_at),
                    patch.due_all_day,
                    opt_dt(patch.occurred_at.map(|t| t.trunc_subsecs(0))),
                    patch.status.map(|s| s.as_str()),
                    patch.status.is_some(),
                    patch
                        .status
                        .filter(|s| *s == ItemStatus::Done)
                        .map(|_| dt(Utc::now())),
                    dt(Utc::now()),
                    patch.extra.is_some(),
                    patch.extra.as_ref().map(|v| v.to_string()),
                    patch.clear_start_at,
                    patch.clear_end_at,
                    patch.clear_due_at,
                    patch.recurrence.is_some(),
                    patch.recurrence.as_deref(),
                    patch.clear_recurrence,
                ],
            )?;
            if let Some(tags) = patch.tags.as_deref() {
                tx.execute("DELETE FROM item_tags WHERE item_id = ?1", params![id])?;
                self.attach_tags(&tx, id, tags)?;
            }
            if patch.clear_reminders {
                tx.execute("DELETE FROM reminders WHERE item_id = ?1", params![id])?;
            }
            if let Some(reminders) = &patch.reminders {
                for r in reminders {
                    validate_reminder_spec(&r.spec)?;
                }
                tx.execute("DELETE FROM reminders WHERE item_id = ?1", params![id])?;
                for r in reminders {
                    tx.execute(
                        "INSERT INTO reminders (item_id, spec, channel) VALUES (?1, ?2, ?3)",
                        params![id, r.spec, r.channel],
                    )?;
                }
            }
            tx.commit()?;
        }
        self.get_item(id)
    }

    /// 删除条目，级联清理数据库关联行与附件文件。
    /// 返回被删除的条目（供撤销 / 提示）。
    pub fn delete_item(&self, id: &str) -> Result<Item> {
        let item = self.get_item(id)?;
        let removed_paths: Vec<String> = {
            let conn = self.lock()?;
            let tx = conn.unchecked_transaction()?;
            let paths: Vec<String> = {
                let mut stmt = tx.prepare("SELECT rel_path FROM attachments WHERE item_id = ?1")?;
                let rows = stmt.query_map(params![id], |r| r.get(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            tx.execute("DELETE FROM items WHERE id = ?1", params![id])?;
            tx.commit()?;
            paths
        };
        // 数据库提交成功后再清理文件，失败仅告警（孤儿文件可被扫描清理）
        for rel in removed_paths {
            if let Err(e) = std::fs::remove_file(self.root.join(&rel)) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    eprintln!("myday: warning: failed to remove attachment {rel}: {e}");
                }
            }
        }
        attachment::prune_item_dir(self, &item.id);
        Ok(item)
    }

    /// 完成待办：置 done + 完成时间。
    /// 重复待办（SPRINT-SPEC §2.3）不同：完成 = 截止推进到下一期（status 保持
    /// todo，系列不结束），`extra.recurred_done_at` 记下完成前的截止供回拨。
    pub fn complete_task(&self, id: &str) -> Result<Item> {
        let item = self.get_item(id)?;
        if item.item_type != ItemType::Task {
            return Err(MyDayError::Invalid(format!("{id} is not a task")));
        }
        if item.status == Some(ItemStatus::Done) {
            return Ok(item); // 幂等
        }
        let now = Utc::now();
        if let Some(spec) = item.recurrence.as_deref() {
            let rec = crate::recurrence::Recurrence::parse(spec)?;
            let Some(due) = item.due_at else {
                return Err(MyDayError::Invalid("重复待办必须有截止时间".into()));
            };
            // 逾期完成也推进到「现在之后」的下一期
            let Some(next) = rec.next_after(due, now.max(due)) else {
                return Err(MyDayError::Invalid("无法计算重复待办的下一期".into()));
            };
            let delta = next - due;
            let mut extra = item.extra.clone();
            if let Some(obj) = extra.as_object_mut() {
                obj.insert(RECURRED_DONE_KEY.into(), serde_json::json!(dt(due)));
            }
            {
                let conn = self.lock()?;
                conn.execute(
                    "UPDATE items SET due_at=?2, start_at=?3, extra=?4, updated_at=?5 WHERE id=?1",
                    params![
                        id,
                        dt(next),
                        opt_dt(item.start_at.map(|s| s + delta)),
                        extra.to_string(),
                        dt(now),
                    ],
                )?;
            }
            return self.get_item(id);
        }
        {
            let conn = self.lock()?;
            conn.execute(
                "UPDATE items SET status='done', completed_at=?2, updated_at=?2 WHERE id=?1",
                params![id, dt(now)],
            )?;
        }
        self.get_item(id)
    }

    /// 取消完成：回到 todo（completed_at 随 CHECK 双向封闭自动置 NULL）。
    /// 重复待办若带完成记账（`extra.recurred_done_at`）= 回拨到完成前的那一期。
    pub fn uncomplete_task(&self, id: &str) -> Result<Item> {
        let item = self.get_item(id)?;
        if item.item_type == ItemType::Task {
            if let Some(spec) = item.recurrence.as_deref() {
                crate::recurrence::Recurrence::parse(spec)?;
                let prev = item
                    .extra
                    .get(RECURRED_DONE_KEY)
                    .and_then(|v| v.as_str())
                    .and_then(parse_dt);
                if let Some(prev) = prev {
                    // 回拨：due 回到完成前值，start 保持与 due 的差值
                    let delta = item.due_at.map(|d| d - prev);
                    let new_start = match (item.start_at, delta) {
                        (Some(s), Some(d)) => Some(s - d),
                        _ => None,
                    };
                    let mut extra = item.extra.clone();
                    if let Some(obj) = extra.as_object_mut() {
                        obj.remove(RECURRED_DONE_KEY);
                    }
                    let now = Utc::now();
                    let conn = self.lock()?;
                    conn.execute(
                        "UPDATE items SET due_at=?2, start_at=?3, extra=?4, updated_at=?5 WHERE id=?1",
                        params![id, dt(prev), opt_dt(new_start), extra.to_string(), dt(now)],
                    )?;
                    drop(conn);
                    return self.get_item(id);
                }
            }
        }
        self.update_item(id, ItemPatch { status: Some(ItemStatus::Todo), ..Default::default() })
    }

    /// 待办转日程（SPRINT2-SPEC §7）：新建日程承接标题/备注/标签/字段/附件，
    /// 提醒 `@due±` 映射为 `@start±`，其余原样；原待办随后删除（附件行已迁移，
    /// 不受级联影响）。开始 = 原截止（保留钟点），无截止则取下一整点；结束 = +1h。
    pub fn convert_task_to_event(&self, id: &str) -> Result<Item> {
        let task = self.get_item(id)?;
        if task.item_type != ItemType::Task {
            return Err(MyDayError::Invalid(format!("{id} is not a task")));
        }
        let now_local = chrono::Local::now().trunc_subsecs(0);
        // 开始缺省 = 今天下一整点（与前端 defaultEventStart 一致）
        let next_hour = {
            let d = now_local.date_naive();
            let t = now_local.time() + Duration::minutes(60 - now_local.minute() as i64);
            crate::tpltime::local_to_utc(d.and_hms_opt(t.hour(), t.minute(), 0).unwrap())
        };
        let start = task.due_at.unwrap_or(next_hour);
        let end = start + Duration::hours(1);
        let reminders = task
            .reminders
            .iter()
            .map(|r| NewReminder {
                spec: r.spec.replace("@due", "@start"),
                channel: r.channel.clone(),
            })
            .collect::<Vec<_>>();
        let extra = {
            let conn = self.lock()?;
            extra_for_type(&conn, &task.extra, ItemType::Event)?
        };
        if let Some(spec) = task.recurrence.as_deref() {
            // 待办重复规则依赖 due；转日程后锚点变 start，规则本身语义不变
            crate::recurrence::Recurrence::parse(spec)?;
        }
        let event = self.add_item(NewItem {
            item_type: Some(ItemType::Event),
            title: task.title.clone(),
            note: task.note.clone(),
            start_at: Some(start),
            end_at: Some(end),
            all_day: false,
            due_at: None,
            due_all_day: false,
            occurred_at: None,
            status: None,
            recurrence: task.recurrence.clone(),
            template_id: None,
            anchor_day: None,
            reminders,
            tags: task.tags.clone(),
            idempotency_key: None,
            extra,
        })?;
        self.move_item_attachments(id, &event.id)?;
        {
            let conn = self.lock()?;
            conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
        }
        attachment::prune_item_dir(self, id);
        self.get_item(&event.id)
    }

    /// 日程生成记录（需求 §5 / SPRINT2-SPEC §7）：新记录 occurred_at = 原开始
    /// （不能在未来），标题/备注/标签/字段复制，附件复制为独立副本；
    /// 原日程保留（需求语义：日程完成后一键生成记录）。
    pub fn event_to_log(&self, id: &str) -> Result<Item> {
        let ev = self.get_item(id)?;
        if ev.item_type != ItemType::Event {
            return Err(MyDayError::Invalid(format!("{id} is not an event")));
        }
        let Some(start) = ev.start_at else {
            return Err(MyDayError::Invalid("日程缺少开始时间".into()));
        };
        if start > Utc::now() {
            return Err(MyDayError::Invalid("日程还未开始，不能生成记录".into()));
        }
        let log = self.add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: ev.title.clone(),
            note: ev.note.clone(),
            occurred_at: Some(start),
            tags: ev.tags.clone(),
            extra: {
                let conn = self.lock()?;
                extra_for_type(&conn, &ev.extra, ItemType::Log)?
            },
            ..Default::default()
        })?;
        self.copy_item_attachments(id, &log.id)?;
        self.get_item(&log.id)
    }

    /// 稍后提醒：spec 不可变（改条目时间 / 每日规则必须保持），
    /// snooze 以一条绝对 spec 的覆盖行实现「到 until 再响一次」；
    /// 已发时刻已在 reminder_log 中，不会重复触发。
    pub fn snooze(&self, id: &str, until: DateTime<Utc>) -> Result<Item> {
        {
            let conn = self.lock()?;
            let n = conn.execute(
                "INSERT INTO reminders (item_id, spec, channel)
                 SELECT id, ?2, 'notify' FROM items WHERE id = ?1",
                params![id, dt(until)],
            )?;
            if n == 0 {
                return Err(MyDayError::NotFound(format!("item {id} not found")));
            }
        }
        self.get_item(id)
    }

    // ------------------------------------------------------------------
    // 提醒 / 模板 / 设置（供 GUI 提醒循环与 CLI 使用）
    // ------------------------------------------------------------------

    /// 冲突检测（需求 §5 / SPRINT-SPEC §3.2）：与 [start, end] 时间相交的其他日程
    /// （重复日程按窗口展开后判定；跨天日程两头都算）。
    pub fn conflicting_events(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        exclude_id: Option<&str>,
    ) -> Result<Vec<Item>> {
        let candidates = self.list_items_window(start, end, Some(ItemType::Event))?;
        let mut out = Vec::new();
        for it in candidates {
            if exclude_id == Some(it.id.as_str()) {
                continue;
            }
            let overlaps = if it.recurrence.is_some() {
                crate::recurrence::occurrences_between(&it, start, end)
                    .iter()
                    .any(|occ| {
                        let (s, e) = (
                            occ.start.unwrap_or(start),
                            occ.end.unwrap_or(occ.start.unwrap_or(start)),
                        );
                        s < end && e > start
                    })
            } else {
                match (it.start_at, it.end_at) {
                    (Some(s), Some(e)) => s < end && e > start,
                    _ => false,
                }
            };
            if overlaps {
                out.push(it);
            }
        }
        out.sort_by_key(|i| i.start_at);
        Ok(out)
    }

    /// 提醒环扫描（§5.4 / INTERACTION §6）：展开全部提醒 spec 的发生时刻，
    /// 返回 <= now 且 (reminder_id, 时刻) 未在 reminder_log 中的三元组（按时刻升序）。
    pub fn due_reminder_occurrences(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<crate::reminder::DueOccurrence>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, item_id, spec, channel FROM reminders ORDER BY id")?;
        let reminders: Vec<Reminder> = stmt
            .query_map([], |r| {
                Ok(Reminder {
                    id: r.get(0)?,
                    item_id: r.get(1)?,
                    spec: r.get(2)?,
                    channel: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut out = Vec::new();
        for rem in reminders {
            let Ok(mut item) = conn.query_row(
                "SELECT * FROM items WHERE id = ?1",
                params![rem.item_id],
                item_mapper,
            ) else {
                continue;
            };
            Self::hydrate(&conn, &mut item)?;
            let logged: std::collections::HashSet<String> = {
                let mut stmt =
                    conn.prepare("SELECT remind_at FROM reminder_log WHERE reminder_id = ?1")?;
                let rows = stmt.query_map(params![rem.id], |r| r.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            }
            .into_iter()
            .collect();
            for at in crate::reminder::occurrences(&rem.spec, &item) {
                if at <= now && !logged.contains(&dt(at)) {
                    out.push(crate::reminder::DueOccurrence {
                        reminder: rem.clone(),
                        at,
                        item: item.clone(),
                    });
                }
            }
        }
        out.sort_by_key(|o| o.at);
        Ok(out)
    }

    /// 标记已发送（按 (reminder_id, remind_at) 复合键去重）。
    pub fn mark_reminded(&self, reminder_id: i64, remind_at: DateTime<Utc>) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT OR IGNORE INTO reminder_log (reminder_id, remind_at, sent_at) VALUES (?1, ?2, ?3)",
            params![reminder_id, dt(remind_at), dt(Utc::now())],
        )?;
        Ok(())
    }

    /// 提醒中心历史（SPRINT2-SPEC §5）：最近已处理 occurrence，倒序、去重同条目同时刻。
    pub fn reminder_history(&self, limit: i64) -> Result<Vec<ReminderHistoryEntry>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT rl.remind_at, i.id FROM reminder_log rl \
             JOIN reminders r ON r.id = rl.reminder_id \
             JOIN items i ON i.id = r.item_id \
             ORDER BY rl.remind_at DESC LIMIT ?1",
        )?;
        let rows: Vec<(String, String)> = stmt
            .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);
        let mut out = Vec::with_capacity(rows.len());
        for (at, item_id) in rows {
            let Some(at) = parse_dt(&at) else { continue };
            let Ok(item) = self.get_item(&item_id) else { continue };
            out.push(ReminderHistoryEntry { remind_at: at, item });
        }
        Ok(out)
    }

    /// 提醒中心未读数：remind_at 晚于 seen_at 的已处理 occurrence 条数。
    /// seen_at 缺省（首次使用）= 0 条未读。
    pub fn reminder_unread_count(&self, seen_at: Option<DateTime<Utc>>) -> Result<i64> {
        let Some(seen) = seen_at else { return Ok(0) };
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*) FROM reminder_log WHERE remind_at > ?1",
            params![dt(seen)],
            |r| r.get::<_, i64>(0),
        )
        .map_err(Into::into)
    }

    pub fn list_templates(&self) -> Result<Vec<Template>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields \
             FROM templates ORDER BY sort ASC, name ASC",
        )?;
        let rows = stmt.query_map([], template_mapper)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn get_template(&self, id: &str) -> Result<Template> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields \
             FROM templates WHERE id = ?1",
            params![id],
            template_mapper,
        )
        .optional()?
        .ok_or_else(|| MyDayError::NotFound(format!("template {id} not found")))
    }

    /// 新建模板（§2.6）：defaults 双命名空间在保存时即校验——
    /// 列白名单成员须对该 item_type 有意义，其余 key 必须是活跃字段 id
    /// （scope 匹配或全局），值按 kind 校验。
    #[allow(clippy::too_many_arguments)]
    pub fn add_template(
        &self,
        name: &str,
        tag: Option<&str>,
        icon: Option<&str>,
        item_type: ItemType,
        defaults: &serde_json::Value,
        fields: &serde_json::Value,
        note: Option<&str>,
    ) -> Result<Template> {
        let name = name.trim();
        if name.is_empty() {
            return Err(MyDayError::Invalid("模板名不能为空".into()));
        }
        if !defaults.is_object() {
            return Err(MyDayError::Invalid("模板 defaults 必须是 JSON 对象".into()));
        }
        if !fields.is_array() {
            return Err(MyDayError::Invalid("模板 fields 必须是 JSON 数组".into()));
        }
        self.validate_template_defaults(item_type, defaults)?;
        let id = format!("tpl_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO templates (id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields)
             VALUES (?1, ?2, ?3, ?4, ?5, (SELECT COALESCE(MAX(sort), 0) + 1 FROM templates), 0, ?6, ?7, 1, ?8)",
            params![
                id,
                name,
                tag.map(str::trim).filter(|t| !t.is_empty()),
                defaults.to_string(),
                note.map(str::trim).filter(|n| !n.is_empty()),
                icon.map(str::trim).filter(|i| !i.is_empty()),
                item_type.as_str(),
                fields.to_string(),
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                MyDayError::Conflict(format!("模板「{name}」已存在"))
            }
            other => MyDayError::Storage(other),
        })?;
        drop(conn);
        self.materialize_template_fields_conn(&id)?;
        self.get_template(&id)
    }

    /// 更新模板（可编辑字段整体替换）。fields 物化只增不改（不回写已启用字段）。
    #[allow(clippy::too_many_arguments)]
    pub fn update_template(
        &self,
        id: &str,
        name: &str,
        tag: Option<&str>,
        icon: Option<&str>,
        item_type: ItemType,
        defaults: &serde_json::Value,
        fields: &serde_json::Value,
        note: Option<&str>,
    ) -> Result<Template> {
        let name = name.trim();
        if name.is_empty() {
            return Err(MyDayError::Invalid("模板名不能为空".into()));
        }
        if !defaults.is_object() {
            return Err(MyDayError::Invalid("模板 defaults 必须是 JSON 对象".into()));
        }
        if !fields.is_array() {
            return Err(MyDayError::Invalid("模板 fields 必须是 JSON 数组".into()));
        }
        self.get_template(id)?;
        self.validate_template_defaults(item_type, defaults)?;
        let conn = self.lock()?;
        conn.execute(
            "UPDATE templates SET name = ?2, tag = ?3, icon = ?4, item_type = ?5,
                                  defaults = ?6, fields = ?7, note = ?8 WHERE id = ?1",
            params![
                id,
                name,
                tag.map(str::trim).filter(|t| !t.is_empty()),
                icon.map(str::trim).filter(|i| !i.is_empty()),
                item_type.as_str(),
                defaults.to_string(),
                fields.to_string(),
                note.map(str::trim).filter(|n| !n.is_empty()),
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                MyDayError::Conflict(format!("模板「{name}」已存在"))
            }
            other => MyDayError::Storage(other),
        })?;
        drop(conn);
        self.materialize_template_fields_conn(id)?;
        self.get_template(id)
    }

    /// defaults 保存时校验（§2.6）：列对该 item_type 有意义且值可解析
    /// （时间列可为 `@` token，按 tpltime 文法 + 列合法性校验），
    /// 或活跃字段 id（scope 匹配 / 全局）且值按 kind 合法。
    fn validate_template_defaults(&self, item_type: ItemType, defaults: &serde_json::Value) -> Result<()> {
        let Some(map) = defaults.as_object() else {
            return Ok(());
        };
        for (k, v) in map {
            if COLUMN_DEFAULT_KEYS.contains(&k.as_str()) {
                let allowed = match k.as_str() {
                    "title" | "note" => true,
                    "start_at" | "end_at" => matches!(item_type, ItemType::Event | ItemType::Task),
                    "all_day" => item_type == ItemType::Event,
                    "due_at" | "due_all_day" | "status" => item_type == ItemType::Task,
                    "occurred_at" => item_type == ItemType::Log,
                    _ => false,
                };
                if !allowed {
                    return Err(MyDayError::Invalid(format!(
                        "模板 defaults 的列 \"{k}\" 对 {} 模板无意义",
                        item_type.as_str()
                    )));
                }
                match k.as_str() {
                    "start_at" | "end_at" | "due_at" | "occurred_at" => {
                        // 模板存意图：时间默认只接受 @ 占位，应用时按锚点日解析
                        let Some(s) = v.as_str().filter(|s| crate::tpltime::is_token(s)) else {
                            return Err(MyDayError::Invalid(format!(
                                "模板 defaults 的 \"{k}\" 必须是 @ 时间占位（如 @d0T09:00）"
                            )));
                        };
                        crate::tpltime::validate_for_column(s, k).map_err(MyDayError::Invalid)?;
                        // 记录的发生时间不允许未来：token 只收 now 族过去方向
                        if k == "occurred_at" && !crate::tpltime::is_past_now_token(s) {
                            return Err(MyDayError::Invalid(
                                "occurred_at 的占位只允许 @now / @now-…（记录不能发生在未来）"
                                    .into(),
                            ));
                        }
                    }
                    "all_day" | "due_all_day" => {
                        if !v.is_boolean() {
                            return Err(MyDayError::Invalid(format!(
                                "模板 defaults 的 \"{k}\" 必须是布尔值"
                            )));
                        }
                    }
                    _ => {}
                }
                continue;
            }
            let def = self.get_field_def(k).map_err(|_| {
                MyDayError::Invalid(format!(
                    "模板 defaults 的键 \"{k}\" 既不是列白名单成员，也不是已定义字段 id"
                ))
            })?;
            if def.scope.is_some_and(|s| s != item_type) {
                return Err(MyDayError::Invalid(format!(
                    "字段「{}」不适用于 {} 模板",
                    def.name,
                    item_type.as_str()
                )));
            }
            validate_value_by_kind(k, &def, v)?;
        }
        Ok(())
    }

    /// 读取模板 fields 并物化（创建 / 更新模板后调用）。
    fn materialize_template_fields_conn(&self, id: &str) -> Result<()> {
        let tpl = self.get_template(id)?;
        let Some(list) = tpl.fields.as_array() else {
            return Ok(());
        };
        if list.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        for f in list {
            let Some(fid) = f.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(name) = f.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(kind) = f.get("kind").and_then(|v| v.as_str()).and_then(FieldKind::parse)
            else {
                continue;
            };
            let options = f.get("options").cloned().unwrap_or(serde_json::json!({}));
            let scope = f
                .get("scope")
                .and_then(|v| v.as_str())
                .and_then(ItemType::parse)
                .map(|t| t.as_str())
                .unwrap_or("all");
            let n = conn.execute(
                "INSERT OR IGNORE INTO field_defs (id, name, kind, options, scope, sort, builtin)
                 VALUES (?1, ?2, ?3, ?4, ?5,
                         (SELECT COALESCE(MAX(sort), 0) + 1 FROM field_defs), 1)",
                params![fid, name, kind.as_str(), options.to_string(), scope],
            )?;
            if n == 0 {
                eprintln!("myday: 字段 {fid}({name}) 已存在或撞名，跳过物化");
            }
        }
        Ok(())
    }

    /// 上移 / 下移模板（与相邻模板交换 sort）。
    pub fn move_template(&self, id: &str, up: bool) -> Result<()> {
        let current = self.get_template(id)?;
        let conn = self.lock()?;
        let neighbor: Option<(String, i64)> = if up {
            conn.query_row(
                "SELECT id, sort FROM templates WHERE sort < ?1 ORDER BY sort DESC LIMIT 1",
                params![current.sort],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
        } else {
            conn.query_row(
                "SELECT id, sort FROM templates WHERE sort > ?1 ORDER BY sort ASC LIMIT 1",
                params![current.sort],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
        };
        let Some((nid, nsort)) = neighbor else {
            return Ok(()); // 已在端点，视为成功
        };
        conn.execute("UPDATE templates SET sort = ?1 WHERE id = ?2", params![nsort, current.id])?;
        conn.execute("UPDATE templates SET sort = ?1 WHERE id = ?2", params![current.sort, nid])?;
        Ok(())
    }

    /// 启用 / 禁用模板（§6：禁用 = pinned 0 + UI 隐藏，不删行不删字段）。
    pub fn set_template_pinned(&self, id: &str, pinned: bool) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE templates SET pinned = ?1 WHERE id = ?2",
            params![pinned as i64, id],
        )?;
        Ok(())
    }

    /// 删除模板。条目不受影响（默认值已写入条目），悬挂的 template_id 引用一并清除。
    pub fn delete_template(&self, id: &str) -> Result<Template> {
        let existing = self.get_template(id)?;
        let conn = self.lock()?;
        conn.execute("DELETE FROM templates WHERE id = ?1", params![id])?;
        conn.execute(
            "UPDATE items SET template_id = NULL WHERE template_id = ?1",
            params![id],
        )?;
        Ok(existing)
    }

    // ------------------------------------------------------------------
    // 字段定义（key = field_defs.id；软删保留 extra 历史数据）
    // ------------------------------------------------------------------

    /// 列出活跃字段定义：`scope = Some(t)` 返回全局 + 该类型；`None` 返回全部。
    pub fn list_field_defs(&self, scope: Option<ItemType>) -> Result<Vec<FieldDef>> {
        let conn = self.lock()?;
        let mut stmt;
        let rows = match scope {
            Some(t) => {
                stmt = conn.prepare(
                    "SELECT id, name, kind, options, scope, sort, builtin FROM field_defs \
                     WHERE deleted_at IS NULL AND (scope = 'all' OR scope = ?1) \
                     ORDER BY sort ASC, name ASC",
                )?;
                stmt.query_map(params![t.as_str()], field_def_mapper)?
            }
            None => {
                stmt = conn.prepare(
                    "SELECT id, name, kind, options, scope, sort, builtin FROM field_defs \
                     WHERE deleted_at IS NULL ORDER BY sort ASC, name ASC",
                )?;
                stmt.query_map([], field_def_mapper)?
            }
        };
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        items.sort_by_key(|d| d.sort);
        Ok(items)
    }

    pub fn add_field_def(
        &self,
        name: &str,
        kind: FieldKind,
        options: &serde_json::Value,
        scope: Option<ItemType>,
    ) -> Result<FieldDef> {
        let name = name.trim();
        if name.is_empty() {
            return Err(MyDayError::Invalid("字段名不能为空".into()));
        }
        if !options.is_object() {
            return Err(MyDayError::Invalid("字段 options 必须是 JSON 对象".into()));
        }
        let id = format!("fld_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let conn = self.lock()?;
        // 同名软删字段：复活而不是新建 id——extra 值按字段 id 键存储，
        // 复用旧 id 即自动重挂历史值（「删了又加回来」= 数据回来，§2.5）
        let scope_key = scope.map(|t| t.as_str()).unwrap_or("all");
        let revived: Option<String> = conn
            .query_row(
                "SELECT id FROM field_defs \
                 WHERE name = ?1 AND scope = ?2 AND deleted_at IS NOT NULL \
                 ORDER BY deleted_at DESC LIMIT 1",
                params![name, scope_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old_id) = revived {
            conn.execute(
                "UPDATE field_defs SET kind = ?2, options = ?3, deleted_at = NULL, \
                                        sort = (SELECT COALESCE(MAX(sort), 0) + 1 FROM field_defs) \
                 WHERE id = ?1 AND deleted_at IS NOT NULL",
                params![old_id, kind.as_str(), options.to_string()],
            )?;
            drop(conn);
            return self.get_field_def(&old_id);
        }
        conn.execute(
            "INSERT INTO field_defs (id, name, kind, options, scope, sort, builtin)
             VALUES (?1, ?2, ?3, ?4, ?5, (SELECT COALESCE(MAX(sort), 0) + 1 FROM field_defs), 0)",
            params![
                id,
                name,
                kind.as_str(),
                options.to_string(),
                scope.map(|t| t.as_str()).unwrap_or("all")
            ],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                MyDayError::Conflict(format!("字段「{name}」已存在"))
            }
            other => MyDayError::Storage(other),
        })?;
        drop(conn);
        self.get_field_def(&id)
    }

    /// 活跃字段定义（软删的视为不存在）。
    pub fn get_field_def(&self, id: &str) -> Result<FieldDef> {
        let conn = self.lock()?;
        field_def_by_id(&conn, id)
    }

    /// 改名 / options / 排序。extra 的 key 是字段 id，改名零成本（不重写条目）。
    pub fn update_field_def(
        &self,
        id: &str,
        new_name: Option<&str>,
        new_options: Option<&serde_json::Value>,
        new_sort: Option<i64>,
    ) -> Result<FieldDef> {
        self.get_field_def(id)?;
        if let Some(n) = new_name {
            if n.trim().is_empty() {
                return Err(MyDayError::Invalid("字段名不能为空".into()));
            }
        }
        if let Some(o) = new_options {
            if !o.is_object() {
                return Err(MyDayError::Invalid("字段 options 必须是 JSON 对象".into()));
            }
        }
        let conn = self.lock()?;
        conn.execute(
            "UPDATE field_defs SET name = COALESCE(?2, name), options = COALESCE(?3, options),
                                   sort = COALESCE(?4, sort)
             WHERE id = ?1 AND deleted_at IS NULL",
            params![id, new_name.map(str::trim).filter(|n| !n.is_empty()), new_options.map(|o| o.to_string()), new_sort],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                MyDayError::Conflict("同名字段已存在".into())
            }
            other => MyDayError::Storage(other),
        })?;
        drop(conn);
        self.get_field_def(id)
    }

    /// 删除（软删）：所有字段一律走软删——不占名字位，extra 中的历史数据保留
    /// 不清理，读取时忽略；同名重建时复活原行（历史值自动重挂）。内置字段 = 禁用。
    pub fn delete_field_def(&self, id: &str) -> Result<FieldDef> {
        let existing = self.get_field_def(id)?;
        let conn = self.lock()?;
        conn.execute(
            "UPDATE field_defs SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
            params![id, dt(Utc::now())],
        )?;
        Ok(existing)
    }

    /// 删除字段前的提示用：多少条目带着这个字段的值（按 id）。
    pub fn count_items_with_field(&self, id: &str) -> Result<i64> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*) FROM items WHERE json_type(extra, ?1) IS NOT NULL",
            params![format!("$.\"{id}\"")],
            |r| r.get(0),
        )
        .map_err(Into::into)
    }

    /// 已软删的字段定义（展示层用：旧条目仍需带出历史值的字段名）。
    pub fn list_deleted_field_defs(&self) -> Result<Vec<FieldDef>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, options, scope, sort, builtin FROM field_defs \
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at ASC, name ASC",
        )?;
        let rows = stmt.query_map([], field_def_mapper)?;
        let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        items.sort_by_key(|d| d.sort);
        Ok(items)
    }

    /// 彻底清理全部软删字段（不可逆）：行删除后同名重建 = 全新字段（不复活），
    /// 条目历史值失去名字映射（展示层不再显示）。
    pub fn purge_deleted_field_defs(&self) -> Result<i64> {
        let conn = self.lock()?;
        let n = conn.execute("DELETE FROM field_defs WHERE deleted_at IS NOT NULL", [])?;
        Ok(n as i64)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.lock()?;
        get_setting_on(&conn, key)
    }

    /// 连接级 setting 读取（view 模块播种/迁移在持锁环境下使用）。
    #[doc(hidden)]
    pub fn get_setting_via(conn: &Connection, key: &str) -> Result<Option<String>> {
        get_setting_on(conn, key)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 统计汇总（SPRINT-SPEC §6 / SPRINT2-SPEC §8 / FILTER-SPEC §12）：
    /// 热力图 + pinned log 模板连续天数 + number 字段取值序列，一次调用全取。
    /// `days` = 统计窗口天数（含今天），实际生效范围 7–1095，缺省 365。
    /// P3 起由挂件求值实现（`stats_summary` 直接 SQL 实现已退役），输出信封不变；
    /// 固定按内置 seed 配置求值，不随 UI 定制漂移（§12）。
    pub fn stats_summary(&self, days: i64) -> Result<StatsSummary> {
        crate::view::stats_summary_via_widgets(self, days)
    }

    // ------------------------------------------------------------------
    // 内部工具
    // ------------------------------------------------------------------

    fn attach_tags(&self, tx: &rusqlite::Transaction, item_id: &str, tags: &[String]) -> Result<()> {
        for raw in tags {
            let tag = raw.trim().trim_start_matches('#');
            if tag.is_empty() {
                continue;
            }
            tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![tag])?;
            let tag_id: i64 =
                tx.query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| r.get(0))?;
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
                params![item_id, tag_id],
            )?;
        }
        Ok(())
    }

    /// 填充 tags / attachments / reminders。
    fn hydrate(conn: &Connection, item: &mut Item) -> Result<()> {
        let mut stmt = conn.prepare(
            "SELECT t.name FROM item_tags it JOIN tags t ON t.id = it.tag_id \
             WHERE it.item_id = ?1 ORDER BY t.name",
        )?;
        let rows = stmt.query_map(params![item.id], |r| r.get::<_, String>(0))?;
        item.tags = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT id, item_id, rel_path, mime, size, created_at FROM attachments \
             WHERE item_id = ?1 ORDER BY created_at",
        )?;
        let rows = stmt.query_map(params![item.id], attachment_mapper)?;
        item.attachments = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT id, item_id, spec, channel FROM reminders \
             WHERE item_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![item.id], |r| {
            Ok(Reminder {
                id: r.get(0)?,
                item_id: r.get(1)?,
                spec: r.get(2)?,
                channel: r.get(3)?,
            })
        })?;
        item.reminders = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        Ok(())
    }
}

// ----------------------------------------------------------------------
// 行映射与序列化工具
// ----------------------------------------------------------------------

/// 活跃字段定义（软删的视为不存在）。持锁环境可安全调用的自由函数。
fn field_def_by_id(conn: &Connection, id: &str) -> Result<FieldDef> {
    field_def_any(conn, id)
        .transpose()?
        .filter(|(_, active)| *active)
        .map(|(def, _)| def)
        .ok_or_else(|| MyDayError::NotFound(format!("field {id} not found")))
}

/// 任意删除状态的字段定义，返回 (定义, 是否活跃)。软删历史值校验用。
fn field_def_any(conn: &Connection, id: &str) -> Option<Result<(FieldDef, bool)>> {
    let row: std::result::Result<Option<(String, String, String, String, Option<String>, i64, i64, Option<String>)>, _> =
        conn.query_row(
            "SELECT id, name, kind, options, scope, sort, builtin, deleted_at \
             FROM field_defs WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional();
    match row {
        Ok(None) => None,
        Err(e) => Some(Err(e.into())),
        Ok(Some(tuple)) => {
            let def = field_def_tuple(tuple.0, tuple.1, tuple.2, tuple.3, tuple.4, tuple.5, tuple.6);
            Some(Ok((def, tuple.7.is_none())))
        }
    }
}

fn field_def_tuple(
    id: String,
    name: String,
    kind: String,
    options: String,
    scope: Option<String>,
    sort: i64,
    builtin: i64,
) -> FieldDef {
    FieldDef {
        id,
        name,
        kind: FieldKind::parse(&kind).unwrap_or(FieldKind::Text),
        options: serde_json::from_str::<serde_json::Value>(&options)
            .ok()
            .filter(|v: &serde_json::Value| v.is_object())
            .unwrap_or(serde_json::json!({})),
        scope: scope
            .as_deref()
            .and_then(|s| if s == "all" { None } else { ItemType::parse(s) }),
        sort,
        builtin: builtin != 0,
    }
}

fn get_setting_on(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| {
        r.get(0)
    })
    .optional()
    .map_err(Into::into)
}

fn template_by_name_or_id(conn: &Connection, name_or_id: &str) -> Result<Option<Template>> {
    conn.query_row(
        "SELECT id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields \
         FROM templates WHERE id = ?1 OR name = ?1",
        params![name_or_id],
        template_mapper,
    )
    .optional()
    .map_err(Into::into)
}

/// 模板 defaults 的时间列值 → 具体时刻：只接受 @ token（模板存意图，条目存事实），
/// 按 anchor_day 解析；其余值在保存模板时已被拒绝。
fn resolve_template_token(
    v: &serde_json::Value,
    now: DateTime<Utc>,
    anchor_day: NaiveDate,
    start: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let s = v.as_str()?;
    if !crate::tpltime::is_token(s) {
        return None;
    }
    let ctx = crate::tpltime::ResolveCtx::new(now, anchor_day).with_start(start);
    crate::tpltime::resolve(s, &ctx).ok()
}

/// 提醒 spec 校验：@token（提醒收窄的文法）或 RFC3339 绝对时刻。
fn validate_reminder_spec(spec: &str) -> Result<()> {
    if crate::tpltime::is_token(spec) {
        crate::tpltime::validate_for_column(spec, "reminder")
            .map_err(MyDayError::Invalid)
    } else if parse_dt(spec).is_some() {
        Ok(())
    } else {
        Err(MyDayError::Invalid(format!(
            "提醒 spec 非法：\"{spec}\"（应为 @token 或 RFC3339）"
        )))
    }
}

/// extra 值按字段 kind 严格校验（§5.1）：一律不做自动纠偏。
fn validate_value_by_kind(key: &str, def: &FieldDef, v: &serde_json::Value) -> Result<()> {
    let bad = |msg: String| MyDayError::Invalid(format!("字段 {key}: {msg}"));
    let choices = || -> Vec<&str> {
        def.options
            .get("choices")
            .and_then(|c| c.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default()
    };
    match def.kind {
        FieldKind::Text => {
            if !v.is_string() {
                return Err(bad("文本字段的值必须是字符串".into()));
            }
        }
        FieldKind::Number => {
            if !v.is_number() {
                return Err(bad("数字字段的值必须是数字".into()));
            }
        }
        FieldKind::Bool => {
            if !v.is_boolean() {
                return Err(bad("开关字段的值必须是布尔值".into()));
            }
        }
        FieldKind::Select => {
            let Some(s) = v.as_str() else {
                return Err(bad("单选字段的值必须是字符串".into()));
            };
            if !choices().contains(&s) {
                return Err(bad(format!("值「{s}」不在可选范围内")));
            }
        }
        FieldKind::MultiSelect => {
            let Some(list) = v.as_array() else {
                return Err(bad("多选字段的值必须是数组".into()));
            };
            for x in list {
                match x.as_str() {
                    Some(s) if choices().contains(&s) => {}
                    Some(s) => return Err(bad(format!("值「{s}」不在可选范围内"))),
                    None => return Err(bad("多选字段的值必须是字符串数组".into())),
                }
            }
        }
        FieldKind::Date => {
            let Some(s) = v.as_str() else {
                return Err(bad("日期字段的值必须是字符串".into()));
            };
            let ok =
                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() || parse_dt(s).is_some();
            if !ok {
                return Err(bad(format!("\"{s}\" 不是合法日期")));
            }
        }
        FieldKind::Url => {
            let Some(s) = v.as_str() else {
                return Err(bad("链接字段的值必须是字符串".into()));
            };
            if !s.contains("://") || s.starts_with("://") {
                return Err(bad(format!("\"{s}\" 缺少 scheme（如 https://）")));
            }
        }
    }
    Ok(())
}

/// 本地日期 d 的 [零点, 次日零点) 对应的 UTC 时刻（RFC3339 字符串）。
/// DST 极端缺口时退化为当前时刻（个人应用可接受的兜底）。
fn local_day_bounds_utc(day: NaiveDate) -> (String, String) {
    use chrono::TimeZone;
    let to_utc = |n: chrono::NaiveDateTime| -> DateTime<Utc> {
        chrono::Local
            .from_local_datetime(&n)
            .single()
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_else(Utc::now)
    };
    let start = day.and_hms_opt(0, 0, 0).unwrap();
    let end = (day + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap();
    (dt(to_utc(start)), dt(to_utc(end)))
}

fn item_mapper(row: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get("id")?,
        item_type: match row.get::<_, String>("type")?.as_str() {
            "event" => ItemType::Event,
            "task" => ItemType::Task,
            _ => ItemType::Log,
        },
        title: row.get::<_, Option<String>>("title")?,
        note: row.get("note")?,
        start_at: row.get::<_, Option<String>>("start_at")?.as_deref().and_then(parse_dt),
        end_at: row.get::<_, Option<String>>("end_at")?.as_deref().and_then(parse_dt),
        all_day: row.get::<_, i64>("all_day")? != 0,
        due_at: row.get::<_, Option<String>>("due_at")?.as_deref().and_then(parse_dt),
        due_all_day: row.get::<_, i64>("due_all_day")? != 0,
        occurred_at: row
            .get::<_, Option<String>>("occurred_at")?
            .as_deref()
            .and_then(parse_dt),
        status: row
            .get::<_, Option<String>>("status")?
            .as_deref()
            .and_then(ItemStatus::parse),
        recurrence: row.get::<_, Option<String>>("recurrence")?,
        template_id: row.get("template_id")?,
        completed_at: row
            .get::<_, Option<String>>("completed_at")?
            .as_deref()
            .and_then(parse_dt),
        reminders: Vec::new(),
        tags: Vec::new(),
        attachments: Vec::new(),
        idempotency_key: row.get("idempotency_key")?,
        created_at: parse_dt(&row.get::<_, String>("created_at")?).unwrap_or_else(Utc::now),
        updated_at: parse_dt(&row.get::<_, String>("updated_at")?).unwrap_or_else(Utc::now),
        extra: row
            .get::<_, Option<String>>("extra")?
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|v: &serde_json::Value| !v.is_null())
            .unwrap_or(serde_json::json!({})),
    })
}

fn attachment_mapper(row: &Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        item_id: row.get(1)?,
        rel_path: row.get(2)?,
        mime: row.get(3)?,
        size: row.get::<_, i64>(4)? as u64,
        created_at: parse_dt(&row.get::<_, String>(5)?).unwrap_or_else(Utc::now),
    })
}

/// templates 行映射（列序：id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields）
fn template_mapper(row: &Row) -> rusqlite::Result<Template> {
    Ok(Template {
        id: row.get(0)?,
        name: row.get(1)?,
        tag: row.get(2)?,
        defaults: row
            .get::<_, Option<String>>(3)?
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|v: &serde_json::Value| v.is_object())
            .unwrap_or(serde_json::json!({})),
        note: row.get(4)?,
        sort: row.get(5)?,
        builtin: row.get::<_, i64>(6)? != 0,
        icon: row.get(7)?,
        item_type: row
            .get::<_, Option<String>>(8)?
            .as_deref()
            .and_then(ItemType::parse)
            .unwrap_or(ItemType::Log),
        pinned: row.get::<_, i64>(9)? != 0,
        fields: row
            .get::<_, Option<String>>(10)?
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|v: &serde_json::Value| v.is_array())
            .unwrap_or(serde_json::json!([])),
    })
}

/// field_defs 行映射（列序：id, name, kind, options, scope, sort, builtin）
fn field_def_mapper(row: &Row) -> rusqlite::Result<FieldDef> {
    Ok(FieldDef {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: FieldKind::parse(&row.get::<_, String>(2)?).unwrap_or(FieldKind::Text),
        options: row
            .get::<_, Option<String>>(3)?
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
            .filter(|v: &serde_json::Value| v.is_object())
            .unwrap_or(serde_json::json!({})),
        scope: row
            .get::<_, Option<String>>(4)?
            .as_deref()
            .and_then(|s| if s == "all" { None } else { ItemType::parse(s) }),
        sort: row.get(5)?,
        builtin: row.get::<_, i64>(6)? != 0,
    })
}

pub(crate) fn dt(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 类型转换时的 extra 过滤：只保留目标类型可用的字段（全局或同 scope）
/// 与保留键（文件链接）。原类型专属字段（如待办的优先级）不入新条目。
fn extra_for_type(
    conn: &Connection,
    extra: &serde_json::Value,
    target: ItemType,
) -> Result<serde_json::Value> {
    let mut allowed: std::collections::HashSet<String> = [FILE_LINKS_KEY.to_string()].into();
    {
        let mut stmt =
            conn.prepare("SELECT id, scope FROM field_defs WHERE deleted_at IS NULL")?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        for (id, scope) in rows.flatten() {
            if scope == "all" || scope == target.as_str() {
                allowed.insert(id);
            }
        }
    }
    let mut out = serde_json::json!({});
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            if allowed.contains(k) {
                out[k] = v.clone();
            }
        }
    }
    Ok(out)
}

pub(crate) fn opt_dt(t: Option<DateTime<Utc>>) -> Option<String> {
    t.map(dt)
}

pub(crate) fn parse_dt(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn empty_to_none(s: Option<&str>) -> Option<&str> {
    match s {
        Some(v) if !v.trim().is_empty() => Some(v.trim()),
        _ => None,
    }
}

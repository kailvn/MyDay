# MyDay Schema 重设计（定稿）

> 状态：定稿，可按 §8 实施步骤开工。
> 修订历史见 §10。本文档自包含，实施无需参照讨论过程。

## 1. 设计原则

- 不考虑历史数据，DROP 重建（实施前备份旧库一次）
- 一张 `items` 表统一 event / task / log
- 核心字段只留查询、排序、索引需要的列；类型专属、用户可管理字段进 `extra` JSON
- 自定义字段由 `field_defs` 定义，`extra` 用 `field_defs.id` 作 key（改名零成本）
- 能下放 DB 的约束一律写成 CHECK，绕过应用层也保证一致性
- 提醒 1:N；删除即物理删除（需要回收站再加）
- log 必须有 `occurred_at`，与 `created_at` 严格分离
- **类型创建即定，三种类型不互相转换**：类型由录入时的自动识别一次性确定，保存后锁定。识别规则确定性、校验从严——宁可报错，不猜测、不静默改写，避免用户认知负担
- 字段不常驻播种（历史教训：常驻播种会把用户删除的字段复活）；字段随模板启用
- **时间格式约定：全库所有 `*_at` 列统一 UTC RFC3339 字符串**（如 `2026-09-17T06:00:00Z`）。Rust 侧 `DateTime<Utc>` 序列化已满足。items 的 `end_at >= start_at` CHECK 是字符串比较，依赖此约定。

---

## 2. 表结构

### 2.1 items

```sql
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
  CHECK (title IS NOT NULL OR note IS NOT NULL)
);
```

即：task 的 `completed_at` 非空 ⟺ `status = 'done'`，双向封闭。

**类型不可变**：`type` 创建后不允许修改，DB 层用 trigger 兜底强制（CHECK 无法跨行比较 OLD/NEW）：

```sql
CREATE TRIGGER trg_items_type_immutable
BEFORE UPDATE OF type ON items
WHEN OLD.type != NEW.type
BEGIN
  SELECT RAISE(ABORT, 'items.type is immutable');
END;
```

应用层创建 / 编辑 API 均不暴露类型变更入口；建错类型 → 删除重录（低频路径）。

### 2.2 tags / item_tags

```sql
CREATE TABLE tags (
  id   INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE item_tags (
  item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (item_id, tag_id)
);
```

### 2.3 reminders / reminder_log（提醒 1:N，存相对意图）

```sql
CREATE TABLE reminders (
  id        INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id   TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  spec      TEXT NOT NULL,   -- @token（@start-1h / @due-1d / @dailyT09:00）或 RFC3339
  channel   TEXT NOT NULL DEFAULT 'notify' CHECK (channel IN ('notify','sound','popup'))
);

CREATE TABLE reminder_log (
  reminder_id INTEGER NOT NULL REFERENCES reminders(id) ON DELETE CASCADE,
  remind_at   TEXT NOT NULL,   -- spec 展开出的「发生时刻」（非存储时刻）
  sent_at     TEXT NOT NULL,
  PRIMARY KEY (reminder_id, remind_at)
);
```

- 同一条目可挂多条提醒；spec 存相对意图，发生时刻由提醒环运行时展开
  （`reminder.rs::occurrences`），条目改开始/截止后提醒自动跟随
- 触发判断：展开出的时刻 `<= now` 且 `(reminder_id, 时刻)` 不在 `reminder_log`
- Snooze：不动 spec，插入一条绝对 spec 覆盖行
- reminders.spec 形状属 SCHEMA_VERSION 2：旧库（remind_at）不兼容，
  打开时自动备份后整体重建，不做单表迁移

### 2.4 attachments

```sql
CREATE TABLE attachments (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id    TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  rel_path   TEXT NOT NULL,
  mime       TEXT NOT NULL,
  size       INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
```

### 2.5 field_defs

```sql
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

CREATE UNIQUE INDEX idx_field_defs_name_scope
  ON field_defs(name, scope) WHERE deleted_at IS NULL;
```

- `scope = 'all'` 表示全局（哨兵值，不用 NULL——SQLite 的 UNIQUE 把 NULL 视为互不相等）
- 唯一性用**部分唯一索引**而非表级 UNIQUE：软删的字段不占用名字位，同名可重建；活跃字段仍然唯一
- `builtin = 1` 为内置字段，用户可禁用（即软删：置 `deleted_at`）、可改名改选项，不可物理删除
- 用户自建 `builtin = 0`，可物理删除（同样走软删，以保留 `extra` 中的历史数据）
- 软删字段读取时忽略，不主动清理 `extra`

### 2.6 templates

```sql
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
```

**`defaults` 规则（双命名空间）：**

key 分两类校验，两者都不是才拒绝：

1. 命中 items 列白名单 → 写入对应列。白名单：`title`、`note`、`start_at`、`end_at`、`all_day`、`due_at`、`due_all_day`、`occurred_at`（仅 log 模板，值须为 @now / @now-…）、`status`。**时间列只接受 @ 占位 token**（模板存意图，条目存事实；绝对值在保存时即拒绝）。
2. 是合法的 `field_defs.id`（存在、未软删、`scope` 匹配模板 `item_type` 或为 `'all'`）→ 写入 `extra`，值按 `kind` 校验。**字段默认值走这里，这是模板预填数值能力的核心。**

**时间占位 token（Interaction §5）**：时间列的值只能为 `@` 占位符（文法见 `core/src/tpltime.rs`，如 `@d0T09:00`、`@d+1Tend`、`@start+30m`、`@due-1d`、`@dailyT09:00`——后两个仅提醒 spec），模板存「意图」，应用时按锚点日（缺省今天）解析为 UTC RFC3339；解析权威在 core（一键记录 / CLI 路径），前端有同文法镜像供面板预览。保存模板时即校验 token 语法与列合法性，绝对时间值直接拒绝。

附加规则：

- **模板保存时即校验** `defaults`（列对该 `item_type` 是否有意义：如 `status` 仅 task 模板可用；字段 scope 是否匹配），不要等应用模板时才报错。写入条目时若值仍违反 CHECK，DB 兜底拒绝。
- 内置模板升级时，按 `field_defs.id` 覆盖 `name` / `kind` / `options` / `scope`，不动 `deleted_at`（用户已禁用的保持禁用）、不删用户数据。

**`fields` 物化语义：**

- 启用模板时，`fields` 中的定义**一次性写入 `field_defs`**（`builtin = 1`）
- 之后修改模板不回写已启用的字段
- 升级内置模板按 id 覆盖字段定义时，若新 `name` 与其他活跃字段撞名，会被部分唯一索引拒绝——升级逻辑须处理冲突（跳过该字段并记录警告，或自动改名，二选一，实现时定）

### 2.7 settings

```sql
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

---

## 3. 索引

```sql
CREATE INDEX idx_items_type_start     ON items(type, start_at);
CREATE INDEX idx_items_type_due       ON items(type, due_at);
CREATE INDEX idx_items_type_occurred  ON items(type, occurred_at);
CREATE INDEX idx_items_type_status    ON items(type, status);

CREATE INDEX idx_items_task_open ON items(due_at)
  WHERE type = 'task' AND status = 'todo';

CREATE INDEX idx_item_tags_tag  ON item_tags(tag_id);
CREATE INDEX idx_attach_item    ON attachments(item_id);
CREATE INDEX idx_reminders_item ON reminders(item_id);
CREATE INDEX idx_reminders_at   ON reminders(remind_at);
```

部分索引 `idx_items_task_open` 的 status 集合必须与 §5.2 视图查询保持一致。

---

## 4. 种子数据

`field_defs` 只播种一条（优先级随 task 默认可用，对齐需求文档 §7.1）：

```sql
INSERT INTO field_defs (id, name, kind, options, scope, sort, builtin) VALUES
  ('fd_priority', '优先级', 'select',
   '{"choices":["低","中","高"]}', 'task', 0, 1);
```

内置模板 3 个，均 `pinned: 1`——跳过首次引导的用户，记录页至少有这三个入口：

```json
[
  {
    "id": "tpl_health", "name": "服药", "item_type": "log", "icon": "pill",
    "builtin": 1, "pinned": 1, "defaults": { "title": "服药" },
    "fields": [
      { "id": "fd_med_name", "name": "药品", "kind": "text", "scope": "log" },
      { "id": "fd_med_dose", "name": "剂量", "kind": "text", "scope": "log" }
    ]
  },
  {
    "id": "tpl_run", "name": "跑步", "item_type": "log", "icon": "run",
    "builtin": 1, "pinned": 1, "defaults": { "title": "跑步" },
    "fields": [
      { "id": "fd_run_km",    "name": "距离", "kind": "number", "scope": "log" },
      { "id": "fd_run_min",   "name": "时长", "kind": "number", "scope": "log" },
      { "id": "fd_run_level", "name": "强度", "kind": "select", "scope": "log",
        "options": { "choices": ["轻松","中等","力竭"] } }
    ]
  },
  {
    "id": "tpl_weight", "name": "体重", "item_type": "log", "icon": "scale",
    "builtin": 1, "pinned": 1, "defaults": { "title": "体重" },
    "fields": [
      { "id": "fd_weight_kg", "name": "体重(kg)", "kind": "number", "scope": "log" }
    ]
  }
]
```

`defaults` 中的 key 要么是列白名单成员（如 `title`），要么是本模板 `fields` 内的字段 id——两条种子规则覆盖所有合法写法。

---

## 5. 应用层规则

### 5.1 写入规则（按 type）

| 字段 | event | task | log |
|---|---|---|---|
| `title` / `note` | title 必填 | title 必填 | 二者至少一项 |
| `start_at` / `end_at` | 必填 | 可选 | NULL |
| `all_day` | 有效 | 忽略 | 忽略 |
| `due_at` / `due_all_day` | NULL | 可选 | NULL |
| `occurred_at` | NULL | NULL | 必填，默认 now |
| `status` | NULL | 必填，默认 `todo` | NULL |
| `completed_at` | NULL | status=done 时自动写入，其他状态置 NULL | NULL |

CHECK 已在 DB 层强制以上大部分规则；应用层负责默认值填充与报错友好化。

**内容校验从严**（宁可报错，不猜测、不静默改写）：

- 时间文本必须能唯一解析为 UTC RFC3339；解析失败直接报错并高亮输入，不猜测格式、不静默丢弃
- `title` / `note` 按 trim 后非空判断
- log 的 `occurred_at` 不允许晚于现在——未来的"记录"是语义错误，报错并提示改建日程或待办
- `extra` 值按 `kind` 严格校验：`number` 必须可解析为数字；`select` / `multiselect` 值必须在 `choices` 内；`date` 必须为合法日期；`url` 必须带 scheme——一律不做自动纠偏

### 5.2 视图查询

```sql
-- Today / Upcoming / All（未完成）
WHERE type = 'task' AND status = 'todo'

-- Done（已完成）
WHERE type = 'task' AND status = 'done'
ORDER BY COALESCE(completed_at, updated_at) DESC
```

### 5.3 搜索

```sql
SELECT i.* FROM items i
WHERE (?2 IS NULL OR i.type = ?2)
  AND (
    i.title LIKE ?1 OR i.note LIKE ?1
    OR EXISTS (
      SELECT 1 FROM json_each(i.extra) e
      JOIN field_defs f ON f.id = e.key AND f.deleted_at IS NULL
      WHERE f.name LIKE ?1 OR CAST(e.value AS TEXT) LIKE ?1
    )
    OR EXISTS (
      SELECT 1 FROM item_tags it JOIN tags t ON t.id = it.tag_id
      WHERE it.item_id = i.id AND t.name LIKE ?1
    )
  )
ORDER BY i.updated_at DESC
LIMIT 50;
```

- 字段名命中：`f.name LIKE`（经 `e.key = f.id` 关联，条目必须真的用了该字段，不会全表命中）
- 字段值命中：`CAST(e.value AS TEXT) LIKE`
- 标签命中：独立 EXISTS
- 不需要 LEFT JOIN / DISTINCT

### 5.4 提醒触发

```sql
SELECT r.* FROM reminders r
WHERE r.remind_at <= ?now
  AND NOT EXISTS (
    SELECT 1 FROM reminder_log l
    WHERE l.reminder_id = r.id AND l.remind_at = r.remind_at
  );
```

触发后写 `reminder_log(reminder_id, remind_at, now)`；Snooze 更新 `reminders.remind_at`，旧日志不动。

### 5.5 extra 读写

- key 必须是 `field_defs.id`
- 读取时按 `scope IN ('all', item.type)` 过滤
- 已软删字段读取时忽略，不主动清理 `extra`
- 类型校验按 `kind` 执行

### 5.6 时间线查询（log）

```sql
SELECT * FROM items
WHERE type = 'log'
ORDER BY COALESCE(occurred_at, created_at) DESC;
```

### 5.7 title 可空渲染

- 列表显示：`title ?? note 截断(40 字) ?? '无标题'`
- Rust 模型 `title: Option<String>`
- log 详情页 title 为空时，以 note 首行为默认标题

### 5.8 快速添加与类型自动识别

快速添加（快捷键 / 托盘 / CLI 唤起，统一面板）**自动识别类型，用户无需选择**；识别是创建期的一次性决定，保存后类型锁定（§2.1）。

**识别规则（确定性，按优先级命中即定）：**

1. 显式指定优先：CLI `--type`、记录页模板按钮（模板自带 `item_type`，记录类模板即 log）
2. 填了截止（`due_at`）→ task。截止语义优先于开始：任务允许带开始时间，日程不允许有截止，同时填了两者只能是 task
3. 填了开始（`start_at` / `end_at`）→ event
4. 填了 log 范围的字段值（如体重、咖啡）→ log
5. 都没有 → task（裸文本默认为待办）

**回显与改口**：识别结果以类型徽标在保存前回显，用户可在保存前手动改一次；保存后不再提供转换。

**幂等**：客户端生成 `idempotency_key`，格式 `quickadd-{YYYYMMDDHHmm}-{hash}`；冲突时 `INSERT ... ON CONFLICT(idempotency_key) DO NOTHING`，再 SELECT 返回已有条目。

---

## 6. 首次启动

1. 建库 + 写入 §4 种子
2. 检测到首次启动（settings 无标记）→ 弹出「选择启用模板」引导
3. 用户勾选的模板置 `pinned = 1`；跳过引导则维持种子默认（三个内置模板已 pinned）
4. 禁用模板 = 置 `pinned = 0` 并从 UI 隐藏，不删模板行、不删字段

---

## 7. 与现网 schema 的主要差异

- 加 `occurred_at`，log 的发生时间不再复用 `start_at`
- `status` 只服务 task，仅两态（`todo` / `done`；v3 起删除 `doing` / `cancelled`）；event / log 强制 NULL
- `reminders` 去 UNIQUE（1:N），`kind` 改为 `channel`
- `reminder_log` 复合主键，按 `(reminder_id, remind_at)` 去重，支持 snooze 留痕
- `field_defs` 部分唯一索引 + 软删；`extra` 的 key 用字段 id 而非名字（改名零成本，删 `rename_item_field` 全表重写）
- `templates` 增 `fields`（字段随模板启用）、`defaults` 双命名空间（列白名单 + 字段 id）
- 索引：`occurred_at`、`(type, status)`、task 未完成部分索引、reminder 相关
- 9 条 CHECK 下放 DB；时间格式统一 UTC RFC3339
- 类型创建即定、不可互转：DB trigger 禁止 `type` 变更，录入靠确定性自动识别（§5.8）
- 彻底 DROP 重建，删除 `MIGRATIONS` 迁移链

---

## 8. 实施步骤

1. 备份旧库
2. DROP 所有旧表
3. 建表（§2）、索引（§3）
4. 写入种子：`fd_priority` + 3 个 `pinned=1` 内置模板（§4）
5. **Rust 层联动改造：**
   - 删 `MIGRATIONS`、`SEED_FIELD_DEFS`、`SEED_TEMPLATES`、`presets_seeded` 标记
   - 删 `rename_item_field` 全表重写接口
   - `TaskView` 查询改 `status IN`（§5.2）
   - 搜索改 §5.3 SQL
   - 提醒到期查询改复合键判断（§5.4）
   - `ItemFilter.from/to`：log 走 `occurred_at`，event 走 `start_at`
   - `model.rs` `title: Option<String>`
   - `reminder.rs` `kind` → `channel`
   - 创建 / 编辑 API 移除类型变更入口（DB trigger 兜底，§2.1）
   - 模板保存时校验 `defaults`（§2.6 双命名空间）
6. **前端联动改造：**
   - `fields.svelte.ts` 的 `fieldBadges` 改为 id → `field_defs.name` 查名
   - `TemplateForm` / `ItemPanel` / `QuickAdd` 适配
   - `ItemPanel` 移除编辑态类型切换（现为「改选即转换」），创建态保留识别徽标 + 保存前改口
   - 首次启动「选择启用模板」引导（§6）
7. 校验（§9）

---

## 9. 验收清单

- [ ] 软删字段后同名重建成功；活跃同名字段仍被拒
- [ ] 直接 UPDATE 修改 `type` 被 trigger 拒绝
- [ ] 识别规则按优先级命中：模板 → 模板类型；截止 → task；开始 → event；log 字段值 → log；裸文本 → task
- [ ] log `occurred_at` 为未来时间被拒
- [ ] `select` 字段写入 `choices` 之外的值被拒
- [ ] 搜索命中且**只命中相关条目**；搜字段名能命中 `extra` 值
- [ ] 同名全局字段不可重复创建
- [ ] 完成后的任务出现在 Done，排序正确；不再出现在未完成视图
- [ ] 重开（uncomplete）回到 todo，completed_at 清空
- [ ] snooze 后新时间点正常触发，旧日志保留
- [ ] 新建 log 默认带 `occurred_at`
- [ ] 跳过引导的新用户，记录页有三个内置模板按钮
- [ ] 模板带字段默认值：应用模板直接保存，字段值预填成功
- [ ] 模板保存时非法 `defaults`（未知列、非字段 id、scope 不匹配）被拒
- [ ] task 重开后 `completed_at` 为 NULL

---

## 10. 修订历史

| 版本 | 要点 |
|---|---|
| v1 激进版 | 初稿：统一 items、extra JSON、提醒 1:N、字段随模板 |
| v2 修订版 | 修 scope NULL 唯一性 bug（'all' 哨兵）、cancelled/doing 语义缺口、保留 fd_priority、CHECK 下放等 |
| v3 | 修搜索 SQL 全表命中、部分唯一索引替代表级 UNIQUE、种子补 options、反向 CHECK、pinned 显式置 1 等 |
| 定稿 | `defaults` 改双命名空间（列白名单 + 字段 id 走 extra），修复字段默认值被误禁的回归；模板保存时校验；内置模板升级撞名处理策略 |
| spec 化 | SCHEMA_VERSION 2（不兼容重建）：`reminders.remind_at` → `spec`（@token 或 RFC3339，INTERACTION §6）；模板时间默认值只收 @ 占位，`occurred_at` 入白名单（仅 log） |
| v3 两态 | SCHEMA_VERSION 3（不兼容重建）：待办状态收敛 todo/done，删 `doing` / `cancelled`；模板 defaults 移除 `status`；「完成待办自动写记录」删除并清理历史影子记录 |
| 定稿 r2 | 移除类型互转：类型不可变 + trigger 强制，编辑面板去掉类型切换；新增确定性自动识别规则（§5.8）与保存前回显改口；内容校验收紧（时间解析失败报错、log 不允许未来时间、extra 按 kind 严格校验） |

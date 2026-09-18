# MyDay 架构说明

对应《需求文档 v3.0》§九。本文记录分层、数据流、关键决策与理由，供后续迭代对照。

## 1. 分层总览

```
┌──────────────────────────────────────────────────┐
│  GUI  Tauri 2 + Svelte 5（apps/desktop）          │
│  主窗口 / 快速弹窗 / 托盘 / 提醒循环 / IPC 服务    │
├──────────────────────────────────────────────────┤
│  CLI  clap 4（crates/myday-cli，二进制 myday）     │
├──────────────────────────────────────────────────┤
│  核心库  myday-core（crates/myday-core）           │
│  model 数据模型 │ store SQLite │ view 视图引擎      │
│  reminder 调度 │ search 搜索 │ ipc 协议与传输      │
├──────────────────────────────────────────────────┤
│  SQLite（rusqlite bundled, WAL）+ 附件文件目录     │
└──────────────────────────────────────────────────┘
```

Cargo workspace，三个成员共用 workspace 依赖版本。GUI 与 CLI 都只依赖
`myday-core`，不存在第二套数据逻辑（需求 §0.2、§0.3）。

## 2. 模块职责

### myday-core（无 UI 依赖，可独立测试）

| 模块 | 职责 |
|---|---|
| `model` | 三类统一条目 `Item`（`type` 区分）、`NewItem`、`ItemPatch`、`ListFilter`、模板 / 附件 / 提醒模型、短 ID 生成（`evt_` / `tsk_` / `log_` 前缀） |
| `error` | `MyDayError` + 稳定错误码 `ErrorCode`，同时驱动 CLI 退出码与 JSON `error.code` |
| `paths` | 数据目录约定（`~/.local/share/myday`），`MYDAY_DATA_DIR` / `MYDAY_SOCKET_PATH` 覆盖 |
| `store` | SQLite 打开（定稿 Schema，v5 起含 `view_defs`；v4+ 附加式迁移，更旧备份后重建）/ 事务写入；CRUD、类型识别、extra 严格校验、幂等键、待办四视图、搜索、提醒去重查询；`Mutex<Connection>` 保证线程安全（内部辅助函数以 `&Connection` 传递避免重入死锁） |
| `view` | 视图模型与过滤器引擎（《FILTER-SPEC》v1）：条件 AST（两层 AND/OR）、`view_defs` 表与内置视图种子、求值管线（keyword 编译 → ListFilter 预筛下推取超集 → AST 内存终审 → 排序 → limit）、统计挂件三段管线（聚合 → 结果表 buckets/points → 派生指标）与容器模型（页面 = 容器列表，容器 = 横/纵布局 + 挂件列表；预置容器是开库铺好的普通容器，与自建同权可删，「恢复默认统计页」完全重置；`myday stats --json` 按代码预设定义求值，不读页面行）；`stats_summary` 由挂件求值实现，信封不变 |
| `attachment` | 附件字节流落盘 + 登记；相对路径存储；删除条目级联清理文件 |
| `reminder` | `Notifier` trait 抽象 + `tick_once`（查到期 → 通知 → 写 `reminder_log` 去重） |
| `ipc` | 请求 / 响应协议（`IpcRequest` / `IpcResponse`）、Unix socket 服务端（`bind` / `serve`）与客户端（`send` / `is_gui_running`） |

### myday-cli（二进制 `myday`）

- `main.rs`：clap 命令树（item / field / quick-add / search / ping / reveal）。
  CLI 定位为平直的数据通道：不做模板预填、快速创建等交互组装（前端职责）
- `output.rs`：人类可读输出与 `--json` 信封
- `util.rs`：时间参数严格解析（RFC3339 / `Y-m-dTHH:M` / `Y-m-d`，不做自然语言解析）

### apps/desktop（Tauri 2）

- `src-tauri/src/lib.rs`：启动装配——Store、单实例插件、托盘、IPC 服务线程、提醒循环线程、窗口关闭转隐藏
- `commands.rs`：Tauri 命令，全部薄封装到 `Store`（另有文件链接的 `open_file_path` / `reveal_file_path`：`xdg-open` 与 `FileManager1.ShowItems` 定位，路径只记在条目 `extra["文件"]` 不复制文件）
- `ipc_bridge.rs`：实现 `IpcHandler`，处理 CLI 转发来的写操作与窗口控制，广播 `data-changed`
- `reminder_loop.rs`：30 秒循环，`notify-rust` 发 GNOME 通知
- 前端（Svelte 5）：`Shell`（侧边栏 + 六视图 + 统一面板弹层 + 首启引导）、
  `ItemPanel`（**创建 / 编辑统一面板**：类型自动识别 + 保存前改口一次、字段按当前
  类型范围以 id 动态渲染、模板双命名空间应用、log 用发生时间；编辑支持清空时间 /
  删除撤销，显示创建 / 修改时间；类型不可变）、`QuickAddWindow`（快速添加窗口根，
  复用 ItemPanel）、`panel.svelte.ts`（面板全局状态）、
  `fields.svelte.ts`（字段定义全局 store，extra 键 = 字段 id 查名，data-changed 自动刷新）、
  `api.ts`（类型化命令封装）、`deletion.svelte.ts`（延迟删除 + 撤销）、
  `Onboarding.svelte`（首启「选择启用模板」）、设置页字段 / 模板管理、记录页模板按钮

## 3. 关键数据流

### 3.1 快速添加（场景 B，类型自动识别）

```
Ctrl+N / 托盘 / GNOME 快捷键（myday quick-add [--type …]）
  → 统一弹窗：识别规则（§5.8，确定性，按优先级命中即定）：
       1. 显式指定优先：CLI --type / 模板按钮（模板自带 item_type）
       2. 填了截止 → task（截止语义优先于开始）
       3. 填了开始 → event
       4. 填了 log 范围字段值 → log
       5. 都没有 → task（裸文本默认为待办）
  → 识别结果以类型徽标回显，保存前可手动改口一次
  → 保存后类型锁定（items.type 不可变，DB trigger 兜底）
```

core 的 `Store::infer_type` 实现规则 2-5（`add_item` 在 item_type 缺省时调用）；
CLI 的 `--at` 因语义明确在命令层即视为 log。

### 3.2 CLI 写操作（场景 E）

```
myday task add …
  ├─ is_gui_running() = true → IPC AddItem → GUI 的 Store 执行
  │                        → 广播 data-changed → 各视图刷新 → 响应回 CLI
  └─ is_gui_running() = false → CLI 进程 Store 直写 → GUI 下次启动可见
```

单一事实来源是 SQLite 文件；GUI 运行时它同时是"单一写入口"，避免视图缓存失效问题。
GUI 本进程内的变更命令（add/update/delete/complete/snooze/附件）同样广播
`data-changed`，与 IPC 桥共用同一频道 —— 前端不区分变更来源。

### 3.2.1 删除与撤销（P1.8 提前实现）

```
GUI 列表行 ✕（两步确认）→ deletions.request(item)
  → 视图立即隐藏（pendingIds 过滤），数据仍在库中
  → 5s 后 commit：delete_item（事务 + 附件文件清理）→ 广播 data-changed
  → 宽限期内「撤销」：仅移除标记，条目原地恢复（ID 不变、附件未动）
```

延迟提交而非"删除后重建"：重建会改变 ID、附件已被物理删除无法恢复；
宽限期内退出 GUI 则不提交（宁漏删、可重删，不可误删）。CLI 删除为立即提交，
但子命令先做类型校验（`event delete` 拒绝非日程 ID，退出码 2）。

### 3.3 提醒（场景 C）

```
GUI 常驻（托盘 / 隐藏主窗口）
  └─ 每 30s tick_once：
       due_reminders(now)   -- remind_at 到期 且 未在 reminder_log
       → Notifier.notify()  -- DBus org.freedesktop.Notifications
       → mark_reminded()    -- reminder_log 去重
稍后提醒：直接改 reminders.remind_at，旧日志保留，新时间点自然重新触发
```

## 4. 数据模型（SCHEMA-REDESIGN 定稿）

统一一张 `items` 表（event / task / log 三类），9 条 CHECK + 类型不可变 trigger
下放 DB，绕过应用层也保证一致性：

- **公共字段**：title（可空，log 允许只有 note）/ note / tags / attachments / extra / created_at / updated_at
- **类型约束**（CHECK 强制）：event 必有 start/end 且无 due/status/occurred；
  task status ∈ todo/doing/done/cancelled 且 completed_at ⟺ done（双向封闭）；
  log 必有 occurred_at（与 created_at 严格分离）且无 start/end/due/status
- **类型创建即定**：录入时自动识别（§3.1），保存前可改口一次；DB trigger 拒绝
  `UPDATE items SET type=…`；建错类型删除重录（低频路径）
- **字段扩展**：`items.extra` JSON，键 = `field_defs.id`（改名零成本）；
  保留键 `文件`（文件链接路径数组）是唯一例外；patch 时整对象替换

### 4.1 字段系统（Notion 式）

- `field_defs(id PK, name, kind, options, scope, sort, builtin, deleted_at)`：
  kind = text/number/select/multiselect/bool/date/url；options 放类型配置
  （number 的 unit、select 的 choices）；scope 用哨兵 `'all'` 表示全局
  （SQLite 的 UNIQUE 把 NULL 视为互不相等，不用 NULL）
- **部分唯一索引** `idx_field_defs_name_scope (name, scope) WHERE deleted_at IS NULL`：
  软删字段不占名字位（同名可重建），活跃字段仍唯一
- **删除 = 软删**（置 deleted_at）：历史 extra 值保留不清理（写入校验对软删键
  放行透传）；内置字段删除即禁用；种子只播一次（`settings.seed_version`），不复活
- extra 值按 kind 严格校验（number 可解析、select/multiselect 在 choices 内、
  date 合法、url 带 scheme），宁可报错不自动纠偏
- 阶段二（未实现）：字段条件筛选、视图列配置、按字段统计

### 4.2 模板（双命名空间 defaults + fields 物化）

- `templates(id, name UNIQUE, item_type, icon, tag, note, defaults, fields, sort, pinned, builtin)`
- `defaults` 的 key 二选一（保存时即校验）：items 列白名单成员（title/note/start_at/
  end_at/all_day/due_at/due_all_day/status，列须对该 item_type 有意义），
  或活跃字段 id（scope 匹配，值按 kind 校验）→ 写入 extra
- `fields` 数组：启用模板时一次性物化进 field_defs（builtin = 1，id 已存在或
  撞名则跳过告警）；之后修改模板不回写已启用字段
- 内置种子（§4）：`fd_priority` + 服药/跑步/体重三个 pinned 模板；升级按 id 覆盖，
  不动 deleted_at、不复活已删模板

### 4.3 视图与查询

- 待办视图（§5.2）：未完成 = `status IN ('todo','doing')`（doing 同权）；
  Done = `IN ('done','cancelled')` 混排，`COALESCE(completed_at, updated_at)` 倒序，
  cancelled 渲染删除线 + 降透明度
- `ListFilter.from/to` 锚点随类型：log → `occurred_at`，其余 → `start_at`；
  `due_from/due_to` 恒看 `due_at`；`changed_on` 按本地日界匹配 created/updated
- 搜索（§5.3）：title/note LIKE + extra 经 `json_each` 关联 field_defs（字段名或
  字段值命中，要求条目真的用了该字段）+ 标签独立 EXISTS；`文件` 键不参与
- 提醒触发（§5.4）：`remind_at <= now AND (reminder_id, remind_at) 不在 reminder_log`；
  1:N 提醒，snooze 只改 remind_at

### 4.4 Schema（定稿）

```
items(id PK, type CHECK, title, note, start_at, end_at, all_day,
      due_at, due_all_day, occurred_at, status, completed_at,
      template_id, idempotency_key UNIQUE, extra, created_at, updated_at,
      recurrence)
  + 9 条类型 CHECK + trg_items_type_immutable
field_defs(id PK, name, kind, options, scope 'all', sort, builtin, deleted_at)
  + 部分唯一索引 (name, scope) WHERE deleted_at IS NULL
templates(id PK, name UNIQUE, item_type, icon, tag, note, defaults, fields, sort, pinned, builtin)
tags(id, name UNIQUE)          item_tags(item_id, tag_id)  -- 级联删除
attachments(id INTEGER PK, item_id FK, rel_path, mime, size, created_at)
reminders(id INTEGER PK, item_id FK, remind_at, spec, channel CHECK)
reminder_log(reminder_id, remind_at, sent_at, PK(reminder_id, remind_at))
view_defs(id PK, name, panel, config, config_user, builtin, sort, …)
settings(key PK, value)
索引：idx_items_type_{start,due,occurred,status}、idx_items_task_open（部分索引，
  status 集合与未完成视图一致）、idx_item_tags_tag、idx_attach_item、
  idx_reminders_{item,at}
```

要点：

- **时间统一 UTC RFC3339 秒级**存储，展示层转本地时区；写入前截断亚秒，保证数据库往返相等；
  `end_at >= start_at` 的 CHECK 依赖该约定的字符串比较
- **迁移（1.0 数据承诺）**：`PRAGMA user_version` 判定版本，v4 起只走 `MIGRATIONS`
  附加式迁移链（每级一步、SQL 幂等）；**缺迁移步骤 = 报错拒绝打开**，绝不重建丢数据。
  仅 v4 以前的 1.0 前遗留库走「`VACUUM INTO` 备份到 `backups/` → DROP 重建」

## 5. IPC 协议

传输：Unix domain socket（`$XDG_RUNTIME_DIR/myday.sock`），JSON Lines，
一连接一请求一响应。请求以 `cmd` 标签区分（kebab-case）：

`ping` | `show-quick-add` | `add-item` | `update-item` | `delete-item`
| `complete-task` | `snooze` | `refresh` | `reveal`（`convert-item` 已随类型不可变移除）

响应信封与 CLI `--json` 完全一致（`{ok, data}` / `{ok:false, error{code, message}}`），
错误码同源（`myday_core::ErrorCode`）。协议字段名即 API，保持向后兼容（需求 §七.9）。

**选择 UDS 而非 DBus 的理由**：无类型系统 / 服务名注册开销，协议自包含且可跨
进程测试（`tests/store_test.rs::ipc_server_client_end_to_end`）；DBus 通知仅用于
GNOME 通知（notify-rust），职责分离。

## 6. 关键决策记录

| 决策 | 理由 |
|---|---|
| rusqlite(bundled) 而非 sqlx | 同步 API 匹配个人应用规模；bundled 免系统 SQLite 版本漂移；避免为存储引入 async 运行时 |
| `Mutex<Connection>` 而非连接池 | 单用户本地库，写冲突极低；简单且事务安全 |
| 幂等键命中返回已有条目（非报错） | agent 重试语义（需求 §2.5）|
| 时间严格格式解析，无自然语言 | 需求 §1.3、§八.6 明确不做；UI 全点选 |
| 提醒为"绝对时间快照"（创建时换算提前量） | 避免"提前 10 分钟"依赖原始时间的重复推导；修改时间会重设提醒 |
| 附件存文件、库存相对路径 | 需求 §4；数据库轻量、目录可整体迁移 |
| Wayland 快捷键走 GNOME 自定义快捷键 + CLI | 需求 §9.4；Tauri global-shortcut 在 Wayland 不可靠 |
| GUI 关闭 = 隐藏 | 提醒服务需常驻（需求 §9） |
| CLI 错误信封与 IPC 响应同构 | 一套解析代码覆盖两个通道 |
| 删除 = 前端延迟提交（5s）+ 撤销，而非软删除/回收站表 | 撤销零成本（数据未动）；个人应用可接受"退出未提交即未删除" |
| 类型不可变（trigger 兜底）+ 确定性自动识别 | 消除"转换补缺"的猜测与认知负担（SCHEMA-REDESIGN §1、§5.8）；宁报错不静默改写 |
| extra 键 = 字段 id 而非名字 | 改名零成本，删除 rename 全表重写；软删字段的历史值自然保留 |
| CHECK/trigger 下放 DB | 绕过应用层（CLI 直写 / sqlite3 手改）也保证一致 |
| core 内部辅助函数传 `&Connection` | `Mutex<Connection>` 不可重入：持锁时再走会加锁的公开方法会死锁 |

## 7. 已知限制（1.0）

- 重复规则 =「修改全部」语义，无单次例外 / until / 次数结束（README 路线图 P2）
- 重复待办的取消完成旁路：`update_item` 直改 status 不触发回拨（FILTER-SPEC §3
  的写路径统一待办项）
- 今天页 / 日历视图暂不接视图模型（实例模式求值，P5 评估）
- 日历按本地时区聚合，跨时区显示未处理
- 内置种子数据（内置模板名 / 优先级选项值）为中文数据值，不随界面语言切换
  （数据与展示分离的既定决策；NL 时间词解析同）
- 提醒循环依赖 GUI 常驻进程（托盘保活 + 可选开机自启），无独立 systemd 守护（P2）

## 7.1 后续方向（SCHEMA-REDESIGN 落地后）

已落地：field_defs 注册表（软删 + 部分唯一索引）+ extra 键 = 字段 id +
模板双命名空间 defaults 与 fields 物化 + 首启模板引导 + 类型不可变与自动识别 +
提醒 1:N（复合键去重）+ CLI `field` 子命令与 `--field 字段id=值`。
方向：

- `ListFilter` 增加基于 `json_extract(extra, '$."field_id"')` 的条件（等值/范围）
- 视图列配置：列表可选显示字段列，按字段筛选排序
- 统计按字段聚合（体重趋势 = number 字段 + 时间的通用化）
- 提醒 1:N 的 UI（当前创建路径单条默认提醒，schema 已支持多条）

## 8. 测试策略

- `myday-core` 集成测试（`tests/store_test.rs`，对照 SCHEMA-REDESIGN §9 验收清单）：
  种子（fd_priority + 三个 pinned 模板 + 字段物化）、类型化默认值、自动识别优先级、
  类型不可变 trigger、软删字段同名重建、extra 严格校验、模板 defaults 双命名空间与
  非法拒绝、doing/cancelled 视图、snooze 留痕、幂等键、旧库备份重建、ListFilter
  部分反序列化、IPC 协议稳定性与端到端
- `myday-cli` 集成测试（`tests/cli_test.rs`）：运行真实二进制，覆盖统一 item 子命令、
  类型识别、严格校验退出码 2、字段 id 写值、删除/幂等/完成链路
- 每个用例独立临时目录，无共享状态
- CLI 冒烟：`--json` 信封、退出码（2 参数 / 3 未找到）、stdin JSON、模板补全
- GUI ↔ CLI 协同已在真实桌面会话验证（ping / IPC 写入 / 直读一致 / 弹窗唤起）

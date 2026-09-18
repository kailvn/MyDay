[English](README.md) · [中文](README.zh-CN.md)

# MyDay

![CI](https://github.com/kailvn/MyDay/actions/workflows/ci.yml/badge.svg)

个人日程 / 待办 / 记录管理工具 —— 离线优先，Ubuntu GNOME 优先，Tauri + Rust 实现。
当前版本 **1.0.0**（更新日志见 [CHANGELOG.md](CHANGELOG.md)）。

- **快**：全局快捷键唤起快速弹窗，输入标题回车即保存，时间智能默认
- **三类对象统一模型**：日程 Event（要发生的事）、待办 Task（要完成的事）、记录 Log（已发生的事，用于跟踪）
- **提醒可靠**：后台常驻，GNOME 系统通知，去重，稍后提醒，错过聚合
- **可编程**：完整 CLI + `--json` 稳定输出，可被脚本 / agent 调用
- **离线可控**：SQLite + 本地附件目录，数据目录整体复制即可迁移；v4 起只走附加式迁移，升级不丢数据
- **中英双语**：设置页切换界面语言（托盘 / 系统通知同步），重要机制说明收在应用内「帮助」页

| | |
|---|---|
| ![今天](docs/screenshots/today-en.png) | ![月视图](docs/screenshots/calendar-month-en.png) |
| ![快速添加](docs/screenshots/quick-add-en.png) | ![统计](docs/screenshots/stats-en.png) |

更多界面：[周视图时间网格](docs/screenshots/calendar-week-en.png)。

## 项目结构

```
MyDay/
├── crates/
│   ├── myday-core/        # 核心库：数据模型、SQLite 存储、附件、搜索、提醒、IPC
│   └── myday-cli/         # CLI 二进制（myday 命令）
├── apps/desktop/          # Tauri 2 GUI 应用
│   ├── src/               # Svelte 5 前端
│   └── src-tauri/         # Rust 后端（commands / IPC 桥 / 提醒循环 / 托盘）
├── docs/ARCHITECTURE.md   # 架构说明
└── 需求.md                # 需求文档 v3.0
```

GUI 与 CLI 共享 `myday-core`，保证数据模型、存储、提醒逻辑唯一。

## 开发环境

- Rust 1.98+（rustup）
- Node 22 + pnpm
- Linux：`webkit2gtk-4.1`、`gtk3`、`libsoup3`（Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev`）

## 快速开始

```bash
# 构建 CLI 与核心库
cargo build

# 运行测试（core 集成测试 + CLI 测试 + 桌面端单测）
cargo test

# 启动 GUI（开发模式，含热重载）
cd apps/desktop
pnpm install
pnpm tauri dev

# 发布构建（deb 包输出到 apps/desktop/src-tauri/target/release/bundle/）
pnpm tauri build
```

CLI 二进制位于 `target/debug/myday`（或安装到 PATH 后直接 `myday`）。

## CLI 用法

CLI 是平直的数据通道（条目 / 字段完整读写 + 唤起弹窗）；模板预填、快速创建等
交互组装全部在前端。三类对象统一走 `item` 子命令，类型由 `--type` 指定或按内容
自动识别（截止→待办、开始→日程、记录字段→记录、`--at`→记录、裸文本→待办）：

```bash
myday item add --type event --title "项目评审" --start "2026-09-16T14:30" --end "2026-09-16T15:30"
myday item add --title "买牛奶" --due 2026-09-16               # 裸文本默认待办
myday item add --title "上线" --field fd_priority=高           # 键 = 字段 id
myday item add --type log --title "称重" --at "2026-09-16T08:00"
myday item add --title "交周报" --due 2026-09-23 --recurse @weekly:3   # 重复规则
myday item add --template tpl_weight --json                    # 模板自带类型与默认值
myday item list --view today                                   # 待办四视图
myday view list                                                # 视图列表（内置标「默认」，带定制标志）
myday view show view_builtin_tasks_today                       # 生效配置（--seed 看 seed 原文）
myday item query --view view_builtin_tasks_today --json        # 按视图查询（与 GUI 同 id 同结果）
myday item query --view view_builtin_search_all --keyword 牛   # 关键词编译进视图过滤
myday item list --type log --from 2026-09-01 --to 2026-09-15   # 记录时间线（occurred_at）
myday item complete <id>                                       # 重复待办 = 推进到下一期
myday item update <id> --recurse @monthly:15                   # 改重复规则；--recurse none 清除
myday item convert <id> --to event                             # 待办转日程（附件/标签/提醒迁移）
myday item convert <id> --to log                               # 日程生成记录（原日程保留）
myday item snooze <id> --until "2026-09-16T10:00"
myday item delete <id>
myday search "客户沟通"
myday stats --json                                             # 热力 / 打卡连续 / 数值趋势（固定预设定义求值，不随页面定制漂移）
myday stats --days 30 --json                                   # 统计窗口 7–1095 天可调
myday reminders --json                                         # 提醒中心历史（最近已处理提醒）
myday export ics --output ~/calendar.ics                       # 导出日历（RRULE + VALARM）
myday backup                                                   # 一键备份 zip（db + 附件，留 7 份）
myday quick-add --type event --title "临时会议"                 # 仅唤起 GUI 弹窗预填
myday field list                                               # 字段 id 在第一列
myday field add --name 心情 --kind select --choice 好 --choice 差 --scope log
myday field update <id> --name 情绪                            # 改名零成本（值按 id 存）
myday field delete <id>                                        # 软删：同名可重建
echo '{"title":"买牛奶","due_at":"2026-09-16T00:00:00Z"}' | myday item add --stdin --json
myday item add ... --dry-run --idempotency-key agent-1
```

- 视图模型（FILTER-SPEC）：面板展示 = 视图求值结果；内置视图 seed 进库可定制
  （GUI 编辑 = 写 `config_user`，`view_reset` 恢复），`item query --view` 与 GUI 同一求值引擎
- `--field <字段id>=值` 可重复，值自动推断 bool / number / string；extra 的键就是字段 id
- `--json` 输出稳定信封：`{"ok":true,"data":…}` / `{"ok":false,"error":{"code","message"}}`
- 退出码：`0` 成功，`1` 一般错误，`2` 参数错误，`3` 未找到，`4` 冲突
- 时间参数严格解析（`2026-09-16T14:30` / `2026-09-16` / RFC3339），解析失败直接报错
- 写操作优先通过本地 IPC 由运行中的 GUI 执行（视图实时刷新）；GUI 未运行时直接写库

## 快速添加入口

统一无类型标签页：**类型由填写内容推断** —— 填开始时间 → 日程（结束默认 +1 小时）、填截止 → 待办、只填数值 → 记录、只填标题 → 待办（收件箱，稍后整理）。推断结果在保存按钮旁实时显示，可随时编辑纠正。

| 入口 | 操作 |
|---|---|
| 主窗口侧边栏 | 「＋ 快速添加」按钮，快捷键 `Ctrl+N` |
| 系统托盘菜单 | 「快速添加…」 |
| GNOME 全局快捷键 | 绑定 `myday quick-add --type event\|task\|log`（类型仅作预填提示） |
| CLI / agent | `myday quick-add --title "临时会议"` 唤起弹窗预填；直接建条目用 `myday item add` |

GUI 单实例运行；`quick-add` 弹窗路径需 GUI 在运行，直接建条目用 `item add`（GUI 未运行时直写库）。

## 删除与撤销

- **GUI**：所有列表行尾有 ✕ 删除按钮 —— 第一次点击变为「确认？」，2.5 秒内再点才执行（防误触、不弹对话框）
- **5 秒撤销**：删除后底部浮出提示条，宽限期内数据并未真正删除，点「撤销」即恢复（提前实现需求 P1.8）
- **CLI**：`myday item delete <id>`（物理删除；需要回收站再加）
- 删除条目时数据库关联行（提醒 / 标签 / 附件）与附件文件级联清理

## 数据模型：字段系统 + 类型标签

三类对象（日程/待办/记录）统一存一张 `items` 表：

- **结构化列只留查询/排序需要的字段**（开始/结束/截止/发生/状态/完成），其余一切都是**字段**
- **字段系统**（Notion 式）：`field_defs` 注册表定义字段（名称 / 类型 / 单位与选项 / 范围），值存每个条目的 `extra` JSON，**键 = 字段 id**
  - 内置字段：优先级（待办，单选 低/中/高）+ 随内置模板物化的记录字段（药品/剂量/距离/时长/强度/体重）
  - 自定义字段：设置 → 字段管理，或 `myday field add`；支持 文本/数字/单选/多选/开关/日期/链接
  - **改名零成本**（键是 id，无需迁移）；**删除 = 软删**：同名可重建，条目历史值保留但不再展示，内置字段删除即禁用
- **类型创建即定，不可互转**（DB trigger 兜底）：录入时自动识别、保存前可改口一次；建错类型删除重录
- **视图按字段过滤**：日历显示所有带开始时间的条目；记录时间线按 `occurred_at`（与 created_at 严格分离）
- **模板 = 类型 + 列/字段默认值**（`defaults` 双命名空间，保存时校验）+ 随模板启用的字段
  - 记录页按钮 = 已启用的记录类模板；带字段默认值可一键记录，仅列默认则开面板现场补值
- **文件链接**：编辑/快速添加面板把文件拖进窗口（或粘路径回车）即记为链接——**只记路径不复制文件**，存于条目 `extra["文件"]`（字段 id 之外的保留键）；列表行内点击 📎 打开，📁 定位

## 统一条目面板（创建 = 编辑）

创建与编辑是**同一个组件**（快速添加窗口、主窗口弹层、日历格子创建共用），填好的信息保存后再打开编辑，看到的就是同一张表单：

- 类型创建即定：创建时按内容识别（截止→待办、开始→日程、记录字段→记录），保存前可改口一次；编辑面板只显示类型徽标，不可更换
- 「＋ 字段」只列出**当前类型范围**的字段，切类型即刷新；编辑时已有值的字段自动展开
- 创建模式：时间快捷片（下一小时 / 明天 9:00）、模板一键应用、Ctrl+V 粘贴截图
- 编辑模式：显示创建 / 修改时间、完成态切换、删除（5 秒可撤销）
- 待办时间字段支持清空；日程开始/结束必填（结束缺省 +1 小时）；记录用「发生时间」（不允许未来）

## 数据位置

```
~/.local/share/myday/
├── myday.db          # SQLite（WAL）
├── attachments/      # 图片附件（按条目分目录，数据库存相对路径）
└── backups/          # schema 重建时旧库一次性备份
```

环境变量 `MYDAY_DATA_DIR` 可整体重定向数据目录（测试 / 便携部署）。
Schema 重设计（docs/SCHEMA-REDESIGN.md）不保留历史数据：检测到旧结构库会先备份一次再 DROP 重建。

## 已实现（P0 骨架 + 体验冲刺）

- 统一数据模型 + 定稿 Schema v4（items 统一三类、CHECK + 类型不可变 trigger、字段软删、提醒 1:N、**recurrence 重复规则列**；无迁移链，旧库备份后重建）
- CLI（item / field / quick-add / search / **stats** / ping / reveal，`--json`、`--dry-run`、`--stdin`、幂等键、**`--recurse` 重复规则**）
- 本地 IPC（Unix socket + JSON Lines）：CLI ↔ GUI 协同已端到端验证
- GUI 主窗口：今天 / 日历（**月 | 周 | 日**三态，周/日为时间网格）/ 待办（四视图 + **批量操作**）/ 记录时间线 / **统计**（容器 + 挂件：预设热力图 / 打卡连续 / 数值趋势，可增删改、可重命名、可恢复默认）/ 搜索 / 设置
- **重复日程 / 待办**（`@daily` / `@weekly:n` / `@monthly:d`，修改全部语义；重复待办完成 = 推进到下一期，取消完成回拨；月视图 / 周网格 / 今天页均按窗口展开渲染）
- **提醒可靠**：相对 spec 存意图（改期自动跟随）、reminder_log 去重、**补发窗口 + 错过聚合摘要**（设置可调，默认 120 分钟）、**通知按钮直达完成 / 稍后提醒 / 打开定位**
- **冲突检测**：保存日程时与现有日程（含重复展开）相交 → 非阻断 toast 提示
- 统一条目面板：创建 / 编辑同一组件；**时间选择器五行面板**（日期快捷 / 迷你月历 / 时刻快捷+最近使用 / 小时×分钟网格 / 键盘导航）；**重复 chip 行**；**NL 时间提示 chip**（敲「明天9点」Tab 应用，绝不静默改写）
- **Ctrl+K 命令面板**：动作 + 搜索结果混排，「待办 买牛奶」直达预填创建（两段确认）
- **拖拽改期**（对标飞书）：周/日网格块移动（15 分钟吸附、可跨列）+ 边缘调时长 + 空白拖选创建；月视图行拖到日期格平移/改截止；重复块跨星期/跨日 = 规则智能改写；松手才写库，toast 一键撤销
- **法定节假日**：JSON 数据源不硬编码——内置 2026 国务院安排（`public/holidays.json`），设置页可导入自定义 JSON（存数据目录、优先于内置，校验通过才生效），月历/周视图 休/班 角标，今天页假期进度/倒数
- **提醒中心**：侧栏铃铛 + 未读角标，应用内提醒历史可定位/完成/稍后提醒
- **数据出口**：ICS 导出（RRULE + VALARM）+ 一键备份 zip（保留 7 份），设置页与 CLI 双入口
- **类型转换**：待办 ⇄ 日程 → 记录（详情面板两步确认，附件/标签/提醒跟随）
- 批量操作：待办视图多选 → 批量完成 / 改期（明天 / +1 天 / 下周一 / 自定义，时刻保留）/ 删除（5 秒内一键全部撤销）
- 系统托盘 + 单实例 + 关闭隐藏保活；30 秒提醒循环（DBus 通知）；可选开机自启
- 删除：全视图两步确认 + 5 秒撤销；GUI 内变更统一广播刷新；首次启动「选择启用模板」引导
- 文件日志 + panic 落盘（`<数据目录>/logs/`，设置页可一键打开日志目录）
- 界面中英双语（设置页切换，`?lang=en|zh` 可强制）；「帮助」页集中收纳机制说明

## 待完善（对照需求）

- P2：重复单次例外修改（当前重复 = 修改全部 + 拖拽智能改写规则）、systemd 独立提醒守护、本地 ICS 订阅、云同步（明确不做多人协作）
- 节假日按年更新：官方每年 11 月前后发布次年安排，拿到 JSON 在设置页导入即可（数据格式见 `apps/desktop/public/holidays.json`，多年份数据可合并后整体导入）

## 今日视图说明

- **今日安排**：当日的日程（按开始时间升序）+ 待办（今天到期或逾期、未完成）
- **今日活动**：今天**创建或修改过**的所有条目（三类都算，含明天的日程只要今天建/改过），按修改时间倒序，标注「新建 / 修改」

详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

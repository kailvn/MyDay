# 体验冲刺 Spec — 可对外展示版本

> 状态：**已实施并通过双 Agent 实机验收**（2026-09-17 夜：Agent A 创建/录入流、
> Agent B 浏览/操作流，截图+UI 操作全项 PASS 后验收；过程中发现并修复
> 「面板入口失效、NL chip 不应用、命令面板 Enter、Esc 误杀、日视图固定周一、
> 今天过滤 UTC 时区、记录页 UTC 分组、批量撤销 toast 遮挡」等问题，详见
> INTERACTION v1.3 修订）。目标：一晚迭代出「拿得出手给别人用」的版本。
> 范围 = 已批准的路线 1+2+3；协作 / 权限 / 云明确不做。
> 三个设计决策已拍板（2026-09-17）：
> ① 周视图 = **时间网格**；② 重复 = **只支持「修改全部」**（无单次例外）；
> ③ 受控 NL 时间提示 = **纳入，排最后、可砍**。
> 本文自包含；交互基调沿《INTERACTION》定稿，schema 沿《SCHEMA-REDESIGN》。

## 0. 破坏性变更与总原则

- **`SCHEMA_VERSION` 3 → 4**（`store.rs`）：打开旧库先 VACUUM INTO 备份到 `backups/` 再 DROP 重建——**本地现有数据会被清空**（项目既定约定，无迁移链）。演示机需知晓。
- **确定性原则不动摇**：宁可报错不猜测、不静默改写；NL 解析只出「待确认 chip」，保存永远要过 Enter。
- **提醒存意图**（spec）机制零改动，所有新功能（重复推进、批量改期）都搭它的便车。
- 新前端组件一律 Svelte 5 runes（`$state/$derived/$props`），风格随现有代码；颜色只用现有 CSS 变量。

本轮**不做**（勿扩散范围）：协作/权限/云、子任务、分组与文件夹、重复的单次例外、重复的 until/次数结束、四象限、周视图内拖拽改期/拖拽创建、统计自定义时间范围、通知中心、systemd 独立提醒守护（P2）、ICS、自然语言自动改写标题。

---

## 1. 提醒补硬伤

### 1.1 补发窗口 + 错过聚合（修 store.rs:1384 无下限问题）

**行为定义**
- 新设置键 `reminder_catchup_minutes`（settings 表，默认 **120**，范围 0–1440），SettingsView 暴露「提醒补发窗口（分钟）」。
- 提醒环每轮把未发 occurrence 分两桶（以 `at <= now` 为前提）：
  - **补发**：`now - window < at` —— 正常弹通知（现有路径）；
  - **错过**：`at <= now - window` —— **不弹**，直接 `mark_reminded` 静默入 `reminder_log`；若本轮错过桶非空，追加发**一条**聚合摘要通知。
- 摘要通知：标题「MyDay · 错过 N 条提醒」，body 最多列 3 行「标题 · 原定时刻」，余下以「…等 N 条」收尾。摘要本身不写 reminder_log（它不是提醒），错过项已记录，下轮天然不重发。
- `window = 0` 时全部错过只进摘要（机器长期关机场景的兜底）。

**改动点**
- `store::due_reminder_occurrences(now, window)` 返回 `Catchup { to_fire, missed }`（两桶都按时刻升序）。
- `Notifier` trait 增 `notify_missed(count, lines: &[(String, String)])`；`LogNotifier` / `DesktopNotifier` / 测试收集器同步实现。
- `reminder::tick_once` 分桶处理；GUI 侧从 settings 读 window（每轮读取，改设置即时生效）。
- 边界：新建/编辑条目时 spec 落在过去时刻（如老条目补 `@start` 提醒）→ 落错过桶被聚合，不会风暴。

**测试**：窗口内补发；窗口外静默且计数正确；下一轮不再出现（两桶皆然）；`window=0` 全聚合。

### 1.2 通知按钮直达（完成 / 稍后提醒 / 打开）

现状：`reminder_loop.rs` 的 `DesktopNotifier` 丢弃 `actions`（参数名 `_actions`），点击只能打开定位。

**行为定义**（action id 复用 `tick_once` 已构造的）
| 类型 | 按钮 | 行为 |
|---|---|---|
| task | 完成 | `store.complete_task(item_id)`（重复待办按 §2 推进）+ emit `data-changed` 刷新前端 |
| task/event/log | 稍后提醒 | `store.snooze(item_id, now + 10min)`（与 CLI `item snooze` 同一函数；固定 10 分钟，本版不做时长菜单） |
| 全部（含通知本体点击 = default action） | 打开 | 唤起主窗口（show + set_focus，托盘路径已有）+ emit `reminder-fired` {id}；前端监听该事件 → 打开该条目只读详情面板（`panel.svelte` 的 rowDetail 路径） |

- 分发函数 `handle_action(app, store, action, item_id)` 放 `reminder_loop.rs`，dbus 回调线程只做转发；单元测试用 mock 覆盖三种 action（dbus 本身无法 CI）。
- GNOME 勿扰/锁屏时按钮不可用 → 降级为现状（点开定位），不阻塞发布。
- **手动验收**：GNOME 真机各点一遍三按钮 + 通知本体。

---

## 2. 重复日程 / 待办（「修改全部」语义）

### 2.1 Schema 与文法

- `items` 新列 `recurrence TEXT NULL`；CHECK：`recurrence IS NULL OR type IN ('event','task')`（记录不重复）。类型不可变 trigger 已有，无需新增。
- 文法（独立命名空间，不复用 tpltime 的时刻 token）：
  ```
  @daily          每天
  @weekly:<n>     每周，n=1..7（1=周一 … 7=周日）
  @monthly:<d>    每月，d=1..31（超当月天数 → 取当月最后一天）
  ```
  core 严格校验，非法值报错不入库；CLI 非法值 exit 2。

### 2.2 核心语义（`crates/myday-core/src/recurrence.rs`，新建）

- 锚点：event = `start_at`；task = `due_at`。
- **展开** `occurrences_between(item, from, to) -> Vec<Occurrence>`：
  - event：每个 occurrence = (锚点平移到 spec 时刻，`end_at` 按 `end-start` 时长同步平移；all_day 对同样整体平移)；
  - task：occurrence = spec 时刻作为 due；`start_at` 若有，随 due 保持原差值平移；
  - 月末钳制（31 → 2 月 28/29）；单次查询窗口展开上限 **400**（与提醒环同款防御）。
- **推进** `next_after(item, t) -> Option<DateTime>`：严格大于 t 的下一次。
- TS 镜像 `lib/recurrence.ts` + golden（沿 `tpltime.golden.ts` 模式，用例与 Rust 测试向量一致锁定）。

### 2.3 完成语义（重复待办）

- `complete_task` 遇 `recurrence` 非空：写完成记录（现有逻辑）→ `new_due = next_after(max(now, due_at))`；`start_at` 平移相同 delta；**status 保持 todo**（系列不结束）；`extra` 保留键 `recurred_done_at` = 推进前的 due_at（ISO）。next 计算失败（理论不达）→ 退化 `status=done`。
- 取消完成：`extra.recurred_done_at` 存在 → due_at 回拨到该值（start_at 同步平移）、删键、status=todo；不存在（旧数据/多轮完成）→ 仅回状态，行 title 提示「重复任务完成不回退截止日」。
- 提醒联动零改动：`@due` / `@start` spec 自动跟随新锚点，`reminder_log` 历史时刻不重发；`@dailyT09:00` 的期间随 due 延伸（语义 = 系列每天 9 点提醒，可解释即可）。

### 2.4 渲染层展开（不物化实例）

- `list_items_window` 等查询**不改**；前端统一经 `recurrence.ts` 展开成虚拟 occurrence 后进：月历圆点与当天面板三节、周/日网格、今天视图「今日安排」。
- 虚拟 occurrence 点击 = 打开该条目（编辑即改系列，「修改全部」决策）；行/块上标 🔁 每天 / 🔁 每周三 / 🔁 每月 15 日。

### 2.5 UI（ItemPanel）与 CLI

- 面板：event 激活时间组、task 激活截止后出现「重复」chip 行：`不重复 / 每天 / 每周… / 每月…`；「每周」展开周一..周日单选 chips；「每月」= 数字输入(1–31) + 「月末」快捷。编辑态显示当前值，可清除（清除 = NULL，历史本就不物化，零副作用）。
- CLI：`item add --recurse <spec>`；`item update <id> --recurse <spec|none>`；`item list --json` 与信封输出带 `recurrence` 字段。
- 不出现飞书式「此事件 / 全部事件」弹窗——只有一个系列，改即全部。

**测试**：spec 校验（DB CHECK + core）、展开/月末钳制/400 上限、推进与取消回拨、TS golden 对齐、CLI 出入参。

---

## 3. 周视图（时间网格）+ 冲突检测

### 3.1 周视图

- CalendarView 顶部 segmented：**月 | 周 | 日**（侧栏入口不变）；周/日共用同一网格组件，日 = 单列。
- 网格：7 列 × 24 小时行（44px/小时，容器内滚动；挂载时滚到 `now - 2h`）；列头「一 15」，今天高亮 accent；顶栏 `‹ 今天 ›` + 范围标题（9月14 – 9月20）；当前时刻红线（每分钟刷新，离开视图清除）。
- 事件块：按本地时刻绝对定位（最小可视高度 22px）；同日重叠**贪心分列并排**（冲突天然可见）；all_day 进顶部「全天」行。
- 到期待办：列头下「到期」行 chips（红点 + 标题），不进网格（due 主语义是日）；记录不进周/日网格。
- 交互：单击空白 = 选中时段高亮；**双击空白 = 创建**（锚点日=该列、开始=该小时整点、结束=+1h，走现有 openCreate 带锚点路径）；点事件块 = 只读详情；Esc 取消选中。
- 数据：`list_items_window(周一起 00:00, 周日 23:59)` + `recurrence.ts` 展开。

### 3.2 冲突检测（需求 §5：提示但不阻止）

- core 新增 `conflicts_for(start, end, exclude_id) -> Vec<Item>`（仅 event，含重复展开入窗）；IPC 命令 `check_conflict`。
- ItemPanel 保存 event 时调用：命中 → 非阻断 toast「与 N 个日程时间冲突：A、B…」，照常保存。
- 周视图并排即视觉冲突，不另加标记。

---

## 4. 批量操作（TasksView 四视图）

- 列表头「多选」toggle；多选态每行前置 checkbox，底部浮动条：`已选 N | 全选 | 完成 | 改期 ▾ | 删除 | 取消`。
- **完成**：逐条 `complete_task`（重复待办各自推进，§2.3）。
- **改期**菜单：明天 / +1 天 / 下周一 / 自定义（date input 确认）——只平移 due 的日期部分、保留时刻；无 due 的行不参与（菜单项禁用）。`@due` 提醒自动跟随，零改动。
- **删除**：`deletions` 模块扩展 `requestMany(items)` / `undoAll()`（共享一个 5 秒 timer），toast「已删 N 条 · 撤销」，一键全部恢复。
- 键盘：Esc 退出多选，Ctrl+A 全选可见；切换 tab 清空选择。
- 范围仅 TasksView 四视图（今天/搜索不做）；改期与完成对**已完成 tab 的行**不提供（无意义）。

---

## 5. 时间选择器五行面板（落地 INTERACTION §3.2–3.5）

- 权威交互 = 《INTERACTION》§3.1–3.5，此处只写落地决策：
  - 新组件 `TimePopover.svelte`，ItemPanel 时间行（开始/结束/截止/发生时间）点击值区弹出；快速窗口与主窗口共用 ItemPanel，自动同享。
  - 行构成按 §3.4 变体：event 开始弹窗含日期快捷/迷你月历/时刻快捷/小时×分钟网格/**时长行**（半小时后/一小时后/两小时后/自定义结束）+ **全天时段标签行**；结束弹窗以已填开始为基准（`@start+` 族标签）；task = 今天/明天/本周五/无截止 + 折叠「具体时间」进网格；log = 现在/5 分钟前/30 分钟前/自定义。
  - **移除 datetime-local 直排输入**：面板网格已覆盖任意 HH:MM（分钟档 00/15/30/45），自由文本不入主路径（不依赖解析原则）。
  - 最近使用：localStorage `myday.recent_times` 存 HH:MM 去重取前 3，显示在时刻快捷行尾。
  - 点快捷即应用即关；点日只改日、点时刻只改时刻（§3.3）；方向键移动、Enter 应用、Esc 关、焦点回触发钮（§3.5）。
- 验收沿用需求 §10.3：常用时间 **3 次点击内**完成。

---

## 6. 统计视图（飞书没有的主战场）

- 侧栏新增「统计」，`ViewId` 增 `"stats"`（今天/日历/待办/记录/统计/搜索/模板/设置）。
- core 新增 `stats_summary()`（IPC `stats_summary` + CLI `myday stats --json`），一次调用返回：
  - `heatmap`：近 365 天 `[{day:"YYYY-MM-DD", count}]`，按 `occurred_at` 本地日聚合所有 log；
  - `streaks`：每个 **pinned log 模板** `{name, current, longest, last30}`——current = 自然日连续（今天或昨天起算向后），longest = 历史最长，last30 = 最近 30 天次数；
  - `series`：每个 number 类字段（值数 ≥2）`{field_id, name, unit, points:[{t,v}]}`，升序、近 90 天（extra JSON 解析在 Rust 侧做）。
- StatsView 三区（纯 SVG/CSS，无图表库）：
  1. **热力图**：GitHub 式 5 档深浅，hover 显「9月14日 · 3 条」，今天描边；
  2. **模板连续卡片行**：各 pinned 模板「连续 x 天 · 最长 y · 30 天 z 次」；
  3. **趋势折线**：字段 chips 切换，hover 点 tooltip，摘要「min / max / 最新」。
- 空态引导去记录页；配色只用现有 CSS 变量（深浅主题自动兼容）。

---

## 7. Ctrl+K 命令面板（替换跳搜索页）

- Shell 层新组件 `CommandPalette.svelte`：Ctrl+K 开 / 再按或 Esc 关；遮罩居中小窗。侧栏「搜索」页保留不动。
- 结构：输入框 + 连续列表（动作区 / 结果区），↑↓ 跨区移动，Enter 执行。
- **动作集**：新建日程 / 待办 / 记录（openCreate 预设类型）、打开快速添加窗口、切换到 8 个视图、搜索「q」（跳搜索页带词）。
- **动词快路径**：输入以 `待办 `、`日程 `、`记录 ` 开头 → 置顶动作「创建待办「xxx」」（Enter 打开预填面板，**再 Enter 才保存**——两段确认，绝不静默写入）。
- **结果区**：`searchItems` 前 8 条，标类型徽标，Enter 开只读详情。
- 空输入态：全部动作 + 「今天改过」最近 5 条作快速跳转（`changed_on` 查询已有）。

---

## 8. 受控 NL 时间提示（压轴，最先砍）

- `lib/timewords.ts` + golden（`timeword.golden.ts`）：扫描创建态标题 → 至多 1 个候选，出 **chip**「明天 09:00 ⿻Tab 应用」，悬于标题下方。
- 词表与**定死规则**（全部进 golden，不做聪明推断）：
  - 日期：今天 / 明天 / 后天 / 大后天 / 下周X / 周X（X=一二三四五六日天）；
  - 时刻：`N点` / `N点半` / `N:MM` / `早上N点` / `下午N点` / `晚上N点` / `中午12点半`；
    **裸 `N点` = 24 小时制 N:00**（不因「现在 15 点」把「9点」猜成明天上午）；`下午/晚上 N点` = N<12 ? N+12 : N；
  - 相对：N分钟后 / N小时后 / 半小时后 / 一小时后。
- Tab 或点击 chip → 按**激活模型**（INTERACTION §2）写入对应字段：时刻词 → 开始（激活时间组→日程）或与日期词组合写入；纯日期词且类型推断为待办时写截止。chip 消失；字段已有手填值则不再出 chip（防覆盖）。
- **不改写标题文本**（本版不做剔除）；不自动保存，主流程零依赖——解析只是加速键。

---

## 9. 实施顺序与砍线

每步以 `cargo test` + 前端构建全绿为门槛再进下一步；golden 文件随实现同步更新。

| # | 内容 | 线 |
|---|---|---|
| 1 | §1 提醒（1.1 补发窗口 + 1.2 通知按钮） | 必达 |
| 2 | §2 重复（schema bump → core+golden → CLI → 面板 → 展开接入三视图） | 必达 |
| 3 | §3 周视图 + 冲突检测 | 必达 |
| 4 | §4 批量操作 | 必达 |
| 5 | §6 统计 | 必达 |
| 6 | §5 时间选择器面板 | 加强 |
| 7 | §7 命令面板 | 加强 |
| 8 | §8 NL 时间提示 | 冲刺（**砍线 = 这里**） |

- **必达线 1–5 = 「拿得出手」的下限**；加强线 6–7 强烈建议；时间不够从 8 开始砍、其次 7。
- 步骤 2 是唯一动 schema 的一步，最先做以免夜间返工。

## 10. 总验收清单

1. 机器休眠 1 天后唤醒：只弹窗口内提醒 + 一条「错过 N 条」摘要，无风暴；重启同样。
2. GNOME 通知上「完成」→ 列表即时少一条；「稍后提醒」→ 10 分钟后复响；点本体 → 主窗口定位并开详情。
3. 建重复待办「每周三 交周报」，完成 → 截止自动到下周三、留完成记录、未完成状态；取消勾选 → 回拨本周三。
4. 月历/周网格/今天页均能看到重复日程的每次发生；点任一次编辑改的是整个系列。
5. 周视图当前时刻红线正确；重叠日程并排显示；保存冲突日程出 toast 但保存成功。
6. 待办多选批量完成/改期/删除；批量删除 5 秒内一键全恢复。
7. 时间选择器：从空到「明天 14:30」≤3 次点击；快捷即点即生效。
8. 统计页三区出数：热力图与记录时间线一致；连续天数手动可验证；体重趋势点带 tooltip。
9. Ctrl+K 输「待办 买牛奶」Enter → 预填面板；再 Enter 才入库；输入普通词出搜索结果。
10. 敲「明天9点开会」出「明天 09:00」chip，Tab 后时间组激活为日程；标题原样未动。
11. 需求 §10 原有 16 条验收不回退（尤其：快捷键弹窗 <300ms、Enter 保存、CLI `--json`、单实例）。

## 11. 风险与备注

- **notify-rust action 在 Wayland/GNOME 的真机表现**是最大不确定项：失败降级 = 现状（打开定位），不阻塞发布；X11/Wayland 各验一遍。
- **SCHEMA_VERSION 4 触发重建**：演示机如已有数据，先确认 `backups/` 备份生成。
- 周视图无虚拟化：窗口条目数 < 数千无压力；超千条再谈优化。
- 收尾文档（实现完成后）：README「已实现 / 待完善」清单、INTERACTION 修订历史 v1.3（重复、周视图、批量、时间面板、统计、面板、NL）。

# 体验冲刺 2 Spec —— 直接操控 & 本地深度（对标飞书二轮）

> 状态：实施中（2026-09-17 启动）。一轮见 SPRINT-SPEC.md（已验收）。
> 目标：在与飞书日历 / 待办、Google Calendar、Todoist 的同类功能对比中，
> 把「一轮还明显落后」的交互补齐，并把离线个人工具的独特优势做深。
> 协作 / 权限 / 云 / 多人继续明确不做。

## 0. 差距分析（二轮选题依据）

| 能力 | 飞书 / Google 等成熟软件 | MyDay 一轮末现状 | 结论 |
|---|---|---|---|
| 拖拽改期 / 拖拽调时长 | 周视图拖块移动、边缘拉伸、拖选创建，月视图拖到别的日子 | 一轮明确砍线，全部没有 | **最大体验缺口，本轮必达** |
| 法定节假日（休 / 班） | 飞书日历直接标注 | 无 | 必达（离线内置国务院安排表，本地工具反超点） |
| 通知中心 / 提醒历史 | 飞书有通知中心 | 只有系统通知 + 错过摘要一条 | 必达（应用内提醒中心） |
| 导出 / 备份 | 日历导出 ics、数据可迁移 | 无 | 必达（ICS 导出 + 一键 zip 备份，P1 清偿） |
| 待办 ⇄ 日程联动 | 飞书无此打通 | 需求 P0 列了「转为日程 / 生成记录」未做 | 加强（超越点） |
| 统计自定义范围 | —（飞书没有统计） | 固定 365/90 天 | 加强 |
| 重复规则 | 强（含单次例外、until） | @daily/@weekly/@monthly 修改全部 | 保持；但拖拽重复块 = 智能改写规则，性价比高 |
| 自然语言输入 | Google「明天9点」直填 | 受控 chip（一轮已做） | 已有优势，保持 |
| 时间选择器 | 各家都是下拉 / 滚轮 | 五行面板（一轮已做） | 已有优势，保持 |

**本轮不做**（勿扩散）：子任务、重复单次例外 / until、农历、ICS 订阅导入、
全天行拖拽、通知声音通道、多人 / 权限 / 云、飞书群聊场景。

## 1. 总原则

- **零 schema 变更**：SCHEMA_VERSION 保持 4，不触发重建，现有演示数据安全。
  所有新能力建立在既有表（items / reminder_log / attachments / settings）之上。
- 确定性原则延续：拖拽 = 显式改期（松手才写库，Esc 可取消），15 分钟吸附，
  拖拽结果用 toast 明说改成了什么，并可一键撤销。
- 提醒「存意图」机制不动：拖拽改 start/due 后 @start/@due 提醒自动跟随。
- 前端 Svelte 5 runes，颜色只用现有 CSS 变量；新增 TS 纯函数配 node 自检
  （沿 golden 模式）。

## 2. 拖拽改期（周 / 日网格）— 必达

WeekGrid 增加 pointer 自绘拖拽（不用 HTML5 DnD，保证跨列 + 吸附可控）：

- **移动**：按住事件块主体拖动 → 半透明 ghost 跟随指针（15 分钟吸附、
  可跨列换日）；松手 = `update_item` 平移 start/end（时长不变）。
- **调时长**：块上下边缘 6px 命中区 → 上/下边缘 resize（15 分钟吸附，
  最短 15 分钟；保持另一端不动）。
- **拖选创建**：空白处按下拖动 ≥15 分钟 → 高亮选区，松手直接
  `openCreate(presetStart, presetEnd)`（类型自动=日程）；单击选中 / 双击创建
  语义保留（拖动距离 <5px 视同点击）。
- **取消**：拖拽中 Esc 放弃（不写库）。
- **重复日程**：拖的是虚拟 occurrence，改的是系列 ——
  - 无规则 / @daily：整体平移（daily 跨日只应用钟点差，toast 说明）；
  - @weekly:n 拖到别的星期几 → 改写为 @weekly:<目标>，锚点移到目标日同时刻；
  - @monthly:d 拖到别的日 → 改写为 @monthly:<目标日>；
  - toast 注明「整个系列已改」。
- **撤销**：toast 支持 action 按钮（`toast.show(msg, {action})`），
  拖拽改期后 6 秒内可恢复原 {start,end,recurrence}。
- all_day 块在顶部行，不参与网格拖拽；跨天日程拖可见段 = 平移整个日程。

## 3. 月视图拖拽改期 — 必达

- 当天面板「日程 / 到期」行的按下拖动（阈值 5px，之内仍是点击开详情）：
  - 日程行 → 拖到月历某格 = 整体平移天数（重复规则改写同 §2）；
  - 到期行（待办）→ 拖到某格 = due 改到目标日（保留原钟点；
    无 due 的待办不提供拖拽）。
- ghost = 跟随指针的小标签（标题 + 目标日期实时更新）；
  松手 toast「已改期 / 已改截止 …」+ 撤销。
- 月历格子本身仍是单击选中 / 双击创建，不变。

## 4. 节假日（休 / 班角标）— 必达

- `lib/holidays.ts`：内置 2026 年国务院办公厅放假安排（来源：国办发明电
  〔2025〕，2025-11-04 发布）：
  - 休：元旦 1/1–1/3；春节 2/15–2/23；清明 4/4–4/6；劳动 5/1–5/5；
    端午 6/19–6/21；中秋 9/25–9/27；国庆 10/1–10/7。
  - 班：1/4（周日）、2/14（周六）、2/28（周六）、5/9（周六）、
    9/20（周日）、10/10（周六）。
  - 数据年之外一律无角标（优雅降级）；每年官方发布后追加一段即可。
- 展示：月历格子右上角「休」（绿）/「班」（红）小角标 + tooltip 节日名；
  周视图列头节假日名 + 角标；今天页头部显示「今天 · 假期第 N 天」或
  「距下一个假期（XX）还有 N 天」。
- 纯前端展示层，core 不参与；`holidays.golden.ts` 自检数据完整性
  （每个区间合法、休/班不重叠、查询函数抽查）。
- **二轮修订（JSON 导入，不硬编码）**：数据文件化——内置默认 `public/holidays.json`
  随应用分发；用户在设置页导入自定义 JSON（存数据目录 `holidays.json`，
  Tauri 命令 load/save/reset 读写，优先于内置，删除即回退）。
  `holidays.ts` = 纯逻辑（parseHolidaysJson 严格校验：区间合法 / 不重叠 /
  班日不在假期内 / 日期格式，导入与装载共用），`holidays.svelte.ts` =
  响应式包装（$state 版本号，异步装载完成后视图查询点自动重渲，不阻塞首帧）。
  官方每年发布次年安排后导入 JSON 即可，无需改代码。

## 5. 提醒中心（应用内提醒历史）— 必达

- core `reminder_history(limit)`：`reminder_log JOIN reminders JOIN items`，
  近 N 条倒序（remind_at、条目类型 / 标题 / 状态、spec、锚点时间）；
  条目随 reminders 级联删除，历史天然只留现存条目。
- 未读：设置键 `reminder_seen_at`（RFC3339），未读数 =
  count(remind_at > seen_at)。提醒环每轮触发后 GUI emit 现有
  `reminder-fired`，前端据此刷新角标。
- UI：侧栏品牌行右侧铃铛按钮 + 未读数字角标；点开右下浮层列表：
  每行「时间 · 类型徽标 · 标题 · 状态」，操作：定位（打开详情）、
  待办「完成」、任意「稍后 10 分钟」（复用 snooze）。
  打开即标记已读（seen_at = now）。
- 定位飞书「通知中心」的心智：错过 / 关机丢通知的兜底入口。

## 6. ICS 导出 + 一键备份 — 必达

- core `export_ics()`：VEVENT（DTSTART/DTEND UTC、RRULE 映射：
  @daily→FREQ=DAILY，@weekly:n→FREQ=WEEKLY;BYDAY=，@monthly:d→
  FREQ=MONTHLY;BYMONTHDAY=；VALARM 来自 @start-10m / @due-1h / 绝对时刻；
  DESCRIPTION 附 `myday://item/<id>` 回链）+ 带 due 的待办导 VTODO；
  log 不导。CRLF + PRODID `-//MyDay//Schedule//CN`。
- core `backup_zip()`：`VACUUM INTO` 临时快照 + zip（stored，零重依赖：
  `zip` crate default-features=false）打包 db + attachments/ →
  `backups/myday-backup-YYYYMMDD-HHMMSS.zip`，保留最近 7 份自动轮换。
- 出口：设置页「数据」区两按钮（导出到 `exports/myday-YYYYMMDD.ics` 并
  reveal）；CLI `myday export ics [--output]`、`myday backup`（JSON 信封同既有约定）。

## 7. 待办转日程 / 日程生成记录 — 加强

- core `convert_task_to_event(id)`（事务）：新建 event（start = 原 due
  （保留钟点）否则下一整点、end = +1h、title/note/tags/extra/附件全部迁移、
  提醒 @due±→@start± 其余原样）→ 删除原 task。返回新条目。
- core `event_to_log(id)`（事务）：新建 log（occurred_at = start、
  title/note/tags/extra 复制、附件**复制文件**为新条目所有、不带提醒），
  原日程保留（需求：日程完成后一键生成记录）。
- UI：详情面板 actions 区 —— task 显示「转日程」、event 显示「生成记录」；
  均两步确认（沿 DeleteButton 防误触模式），完成后 toast + 打开新条目详情。
- CLI：`myday item convert <id> --to event|log`。

## 8. 统计时间范围 — 加强

- core `stats_summary(days)`（默认 365）：heatmap 与 series 窗口改为
  today-days 起；streak 卡片维持 current/longest，lastN 随范围显示。
- StatsView 顶部 chips：30 / 90 / 180 / 365 天；IPC 与 CLI（`--days`）同参。

## 9. 实施顺序与砍线

| # | 内容 | 线 |
|---|---|---|
| 1 | core：reminder_history / convert / export_ics / backup_zip / stats(days) + 测试 | 必达 |
| 2 | commands + CLI 子命令 | 必达 |
| 3 | 周/日网格拖拽（移动/resize/拖选/撤销/重复改写） | 必达 |
| 4 | 月视图拖拽 + 节假日角标 | 必达 |
| 5 | 提醒中心 | 必达 |
| 6 | ICS/备份设置入口 + 转换按钮 + 统计范围 | 必达 |
| 7 | 浏览器 mock E2E 走查（`?e2e=1` 内存后端） | 验收手段 |

门槛：每步 `cargo test` + `pnpm build` 全绿；golden / 自检脚本同步更新。

## 10. 验收清单

1. 周视图拖块到别的时间/别的列 → 松手即改期成功，@start 提醒跟随，toast 可撤销。
2. 块边缘上下拉伸改时长；空白拖选 ≥15 分钟松手直达创建面板（首尾已填）。
3. 拖拽重复日程到别的星期几 → 规则变 @weekly:<新>，toast 说明系列已改。
4. 月视图从当天面板拖日程/到期待办到任意日期格 → 平移/改截止，可撤销。
5. 月历与周视图正确显示 2026 休/班角标（春节 9 天休、2/14 与 2/28 班）。
6. 铃铛角标 = 未读提醒数；打开列表可定位/完成/稍后提醒，已读即时清零。
7. 设置页导出 ICS：文件生成、含 RRULE 与 VALARM、可被日历软件导入。
8. 一键备份：zip 生成在 backups/ 且包含 db+attachments；第 8 份起自动删旧。
9. 待办详情「转日程」两步确认后：新日程带全部附件与标签，原待办消失。
10. 日程详情「生成记录」：新记录 occurred_at=原开始，附件为副本，原日程仍在。
11. 统计页 30/90/180/365 切换，热力图与趋势窗口随之变化。
12. 既有 16 + 11 条验收不回退（一轮 SPRINT-SPEC §10 + 需求 §10）。

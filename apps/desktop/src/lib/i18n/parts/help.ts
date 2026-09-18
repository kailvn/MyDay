/** 帮助页文案（HelpView.svelte）。机制说明集中此处，界面内提示保持最简。 */
export const part_help = {
  "help.title": "帮助",
  "help.concepts.title": "三类对象",
  "help.concepts.types.k": "日程 / 待办 / 记录",
  "help.concepts.types.v":
    "日程是要发生的事（开始–结束），待办是要完成的事（截止），记录是已发生的事（发生时间）。三者统一存放、类型创建即定。",
  "help.concepts.immutable.k": "类型不可改",
  "help.concepts.immutable.v":
    "保存前可改口一次，保存后类型锁定；建错类型删除重录。待办与日程可互相转换（详情面板 → 转为日程 / 生成记录）。",
  "help.concepts.fields.k": "自定义字段",
  "help.concepts.fields.v":
    "设置 → 模板页管理字段（文本 / 数字 / 单选 / 多选 / 开关 / 日期 / 链接）。改名零成本；删除为软删，同名重建即恢复。",
  "help.capture.title": "录入",
  "help.capture.quick.k": "快速添加",
  "help.capture.quick.v":
    "Ctrl+N 或全局快捷键唤起小窗：输入标题回车即存，类型按内容自动识别（填截止=待办、填时间=日程、裸文本=待办收件箱）。",
  "help.capture.nl.k": "时间输入",
  "help.capture.nl.v":
    "敲「明天9点」会出现提示 chip，按 Tab 应用；绝不静默改写你的输入。时间选择器全程点选。",
  "help.capture.paste.k": "截图",
  "help.capture.paste.v": "Ctrl+V 粘贴剪贴板图片为附件，多张可并存。",
  "help.capture.filelink.k": "文件链接",
  "help.capture.filelink.v":
    "把文件拖进面板（或粘路径回车）只记录路径、不复制文件；列表行内 📎 打开、📁 定位。",
  "help.calendar.title": "日历",
  "help.calendar.drag.k": "拖拽改期",
  "help.calendar.drag.v":
    "周 / 日视图拖块移动（15 分钟吸附）、拖边缘调时长、空白拖选创建；月视图从当天面板把条目拖到任意格。松手才写库，toast 可一键撤销。",
  "help.calendar.recur.k": "重复日程",
  "help.calendar.recur.v":
    "支持每天 / 每周 X / 每月 X 日。修改 = 修改整个系列；把重复块拖到别的星期几 / 日期会智能改写规则（toast 会注明）。",
  "help.calendar.neighbor.k": "相邻月日期",
  "help.calendar.neighbor.v": "月首尾的补位格显示相邻月，灰化但可点、可创建、可拖入（点击即跳转该月）。",
  "help.calendar.holidays.k": "法定节假日",
  "help.calendar.holidays.v":
    "内置当年国务院安排；设置页可导入自定义 JSON（存数据目录、优先于内置，校验通过才生效）。数据年之外不显示角标。",
  "help.calendar.conflict.k": "时间冲突",
  "help.calendar.conflict.v": "保存日程与现有日程相交时弹提示，但不阻止保存。",
  "help.tasks.title": "待办",
  "help.tasks.done.k": "完成",
  "help.tasks.done.v": "勾选即完成并沉底；再次勾选回退。红色只表示未完成的紧迫项。",
  "help.tasks.recur.k": "重复待办",
  "help.tasks.recur.v": "完成 = 推进到下一期；取消完成 = 回拨一期（只回一期）。",
  "help.tasks.batch.k": "批量操作",
  "help.tasks.batch.v": "待办视图多选后可批量完成 / 改期 / 删除；删除 5 秒内可一键全部撤销。",
  "help.reminders.title": "提醒",
  "help.reminders.intent.k": "提醒跟随条目",
  "help.reminders.intent.v":
    "提醒存「意图」（如开始前 10 分钟）而非固定时刻；条目改期后提醒自动跟随，无需逐条改。",
  "help.reminders.catchup.k": "错过不再轰炸",
  "help.reminders.catchup.v":
    "休眠 / 关机错过的提醒：窗口内（默认 120 分钟，可调）照常补发；窗口外静默记录并聚合成一条摘要通知。",
  "help.reminders.center.k": "提醒中心",
  "help.reminders.center.v": "侧栏 🔔 收纳历史提醒，可定位 / 完成 / 稍后提醒；系统通知丢失时这里是兜底。",
  "help.stats.title": "记录与统计",
  "help.stats.widgets.k": "统计挂件",
  "help.stats.widgets.v": "统计页由容器 + 挂件组成，可增删改、可重命名；同数据可随时换图型（折线 / 柱状 / 热力）。",
  "help.stats.reset.k": "恢复默认",
  "help.stats.reset.v": "「恢复默认统计页」完全重置为预置布局（自建容器会被清掉）。",
  "help.overlay.title": "今日悬浮窗",
  "help.overlay.lock.k": "锁定 = 点击穿透",
  "help.overlay.lock.v": "锁定后悬浮窗不拦鼠标；解锁走托盘菜单「悬浮窗锁定」。位置 / 尺寸 / 透明度自动记忆。",
  "help.overlay.readonly.k": "只读 + 勾选",
  "help.overlay.readonly.v": "悬浮窗只展示今日未完成项，唯一操作是勾选完成；编辑回主窗口。",
  "help.data.title": "数据与备份",
  "help.data.dir.k": "数据位置",
  "help.data.dir.v": "全部数据在 ~/.local/share/myday/（SQLite + 附件 + 备份），整体复制即可迁移；可用 MYDAY_DATA_DIR 重定向。",
  "help.data.backup.k": "备份",
  "help.data.backup.v": "设置页一键打包 zip（自动保留最近 7 份）；恢复 = 解压覆盖数据目录后再启动。",
  "help.data.migrate.k": "升级不丢数据",
  "help.data.migrate.v": "1.0 起结构升级只走增量迁移；若打开的是更旧的遗留库，会先自动备份到 backups/ 再重建。",
  "help.data.cli.k": "CLI / agent",
  "help.data.cli.v":
    "myday 命令与 GUI 共用同一数据库：CLI 写入 GUI 实时刷新。--json 输出稳定信封，支持幂等键与 --dry-run。",
  "help.keys.title": "快捷键",
  "help.keys.quickadd": "快速添加",
  "help.keys.palette": "命令面板（动作 + 搜索混排）",
  "help.keys.viewswitch": "日历：今天 / 日 / 周 / 月",
  "help.keys.navigate": "日历：上一页 / 下一页",
  "help.keys.enteresc": "保存 / 取消（面板与弹窗通用）",
  "help.keys.paste": "粘贴截图",
  "help.keys.tab": "应用 NL 时间提示",
} as const;

export const part_help_en: Record<string, string> = {
  "help.title": "Help",
  "help.concepts.title": "Three item types",
  "help.concepts.types.k": "Event / Task / Log",
  "help.concepts.types.v":
    "Events are things that will happen (start–end), tasks are things to finish (due), logs are things that happened (occurred at). All three share one store; the type is fixed at creation.",
  "help.concepts.immutable.k": "Types are immutable",
  "help.concepts.immutable.v":
    "You may change the type once before saving; afterwards it is locked — delete and re-create if wrong. Tasks and events can be converted (detail panel → Convert to event / Log it).",
  "help.concepts.fields.k": "Custom fields",
  "help.concepts.fields.v":
    "Manage fields under Templates (text / number / select / multi-select / switch / date / link). Renaming is free; deleting is soft — recreate the same name to restore it.",
  "help.capture.title": "Capture",
  "help.capture.quick.k": "Quick Add",
  "help.capture.quick.v":
    "Ctrl+N or a global shortcut opens a mini window: type a title, press Enter. The type is inferred (due set = task, time set = event, bare text = task inbox).",
  "help.capture.nl.k": "Typing times",
  "help.capture.nl.v":
    "Typing natural times like “tomorrow 9am” shows a suggestion chip — press Tab to apply. Nothing is rewritten silently; the picker is fully point-and-click.",
  "help.capture.paste.k": "Screenshots",
  "help.capture.paste.v": "Ctrl+V pastes a clipboard image as attachment; multiple images are kept.",
  "help.capture.filelink.k": "File links",
  "help.capture.filelink.v":
    "Drag a file into the panel (or paste a path) to record the path only — files are never copied. 📎 opens, 📁 reveals.",
  "help.calendar.title": "Calendar",
  "help.calendar.drag.k": "Drag to reschedule",
  "help.calendar.drag.v":
    "In week/day view drag blocks (15-min snap), drag edges to resize, drag on empty space to create; in month view drag rows from the day panel onto any cell. Writes happen on release — the toast offers one-click undo.",
  "help.calendar.recur.k": "Recurring events",
  "help.calendar.recur.v":
    "Daily / weekly / monthly are supported. Editing edits the whole series; dragging a recurring block to another weekday/date rewrites the rule (the toast tells you).",
  "help.calendar.neighbor.k": "Adjacent-month days",
  "help.calendar.neighbor.v":
    "Leading/trailing cells show neighboring months — dimmed but clickable, creatable and drop targets (click jumps to that month).",
  "help.calendar.holidays.k": "Public holidays",
  "help.calendar.holidays.v":
    "The current year's official schedule is built in; import custom JSON in Settings (stored in the data dir, takes priority, validated before use). Years without data simply show no badges.",
  "help.calendar.conflict.k": "Conflicts",
  "help.calendar.conflict.v": "Saving an event that overlaps existing ones shows a hint but never blocks.",
  "help.tasks.title": "Tasks",
  "help.tasks.done.k": "Completing",
  "help.tasks.done.v": "Check to complete (sinks to bottom); uncheck to revert. Red only marks urgency of unfinished items.",
  "help.tasks.recur.k": "Recurring tasks",
  "help.tasks.recur.v": "Completing advances to the next occurrence; un-completing steps back one period.",
  "help.tasks.batch.k": "Batch actions",
  "help.tasks.batch.v": "Multi-select in the Tasks view to complete / reschedule / delete in bulk; deletes can be undone for 5 seconds.",
  "help.reminders.title": "Reminders",
  "help.reminders.intent.k": "Reminders follow items",
  "help.reminders.intent.v":
    "Reminders store intent (e.g. 10 min before start) rather than fixed times; rescheduling the item updates them automatically.",
  "help.reminders.catchup.k": "No notification storms",
  "help.reminders.catchup.v":
    "Reminders missed while asleep: inside the window (default 120 min, adjustable) they fire normally; outside it they are recorded silently and summarized in one notification.",
  "help.reminders.center.k": "Reminder center",
  "help.reminders.center.v":
    "The 🔔 in the sidebar keeps reminder history — locate, complete or snooze. It is the fallback when system notifications are lost.",
  "help.stats.title": "Logs & stats",
  "help.stats.widgets.k": "Stat widgets",
  "help.stats.widgets.v":
    "The stats page is containers + widgets: add, edit, rename freely; switch chart type (line / bar / heatmap) on the same data anytime.",
  "help.stats.reset.k": "Reset",
  "help.stats.reset.v": "“Restore default stats page” resets to the preset layout (custom containers are removed).",
  "help.overlay.title": "Today overlay",
  "help.overlay.lock.k": "Lock = click-through",
  "help.overlay.lock.v":
    "When locked the overlay never intercepts the mouse; unlock from the tray menu. Position, size and opacity are remembered.",
  "help.overlay.readonly.k": "Read-only + check",
  "help.overlay.readonly.v": "The overlay lists today's unfinished items; the only action is checking them done. Edit in the main window.",
  "help.data.title": "Data & backup",
  "help.data.dir.k": "Where data lives",
  "help.data.dir.v":
    "Everything is in ~/.local/share/myday/ (SQLite + attachments + backups). Copy the folder to migrate; MYDAY_DATA_DIR redirects it.",
  "help.data.backup.k": "Backup",
  "help.data.backup.v":
    "One-click zip in Settings (keeps the latest 7). To restore: unpack over the data dir, then launch.",
  "help.data.migrate.k": "Upgrades keep data",
  "help.data.migrate.v":
    "Since 1.0 schema upgrades only run additive migrations; if an older legacy database is found, it is backed up to backups/ before rebuild.",
  "help.data.cli.k": "CLI / agents",
  "help.data.cli.v":
    "The myday CLI shares the same database as the GUI — writes show up live. Stable --json envelope, idempotency keys and --dry-run included.",
  "help.keys.title": "Shortcuts",
  "help.keys.quickadd": "Quick Add",
  "help.keys.palette": "Command palette (actions + results)",
  "help.keys.viewswitch": "Calendar: today / day / week / month",
  "help.keys.navigate": "Calendar: previous / next page",
  "help.keys.enteresc": "Save / cancel (panels and windows)",
  "help.keys.paste": "Paste screenshot",
  "help.keys.tab": "Apply natural-language time chip",
};

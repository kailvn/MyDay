/** 帮助页文案（HelpView.svelte）。机制说明集中此处，界面内提示保持最简。 */
export const part_help = {
  "help.title": "帮助",
  "help.concepts.title": "三类对象",
  "help.concepts.types.k": "日程 / 待办 / 记录",
  "help.concepts.types.v":
    "日程是计划中的事，有开始和结束；待办是欠着的事，有截止；记录是过完的事，有发生时间。三者同库同源，录入时按你填的内容自动归类。",
  "help.concepts.immutable.k": "类型不可改",
  "help.concepts.immutable.v":
    "保存前有一次改口的机会，保存后类型就锁定了——建错类型，删掉重录。待办和日程可以在详情面板互转（转为日程 / 生成记录），不必重录。",
  "help.concepts.fields.k": "自定义字段",
  "help.concepts.fields.v":
    "在「模板」页管理字段：文本、数字、单选、多选、开关、日期、链接。改名随便改；删除是软删，建回同名就原样恢复。",
  "help.capture.title": "录入",
  "help.capture.quick.k": "快速添加",
  "help.capture.quick.v":
    "Ctrl+N 或全局快捷键唤起小窗：打标题，回车，结束。类型按内容自动识别——填了截止是待办，填了时间是日程，什么都没填先进收件箱。",
  "help.capture.nl.k": "时间输入",
  "help.capture.nl.v":
    "打「明天9点」会出现提示 chip，按 Tab 才生效；MyDay 不会替你改写任何输入。想点选的话，时间选择器全程不用键盘。",
  "help.capture.paste.k": "截图",
  "help.capture.paste.v": "Ctrl+V 把剪贴板里的图存为附件，想贴几张贴几张。",
  "help.capture.filelink.k": "文件链接",
  "help.capture.filelink.v":
    "把文件拖进面板（或粘路径回车），MyDay 只记住路径，不动原文件。列表里 📎 打开、📁 定位。",
  "help.calendar.title": "日历",
  "help.calendar.drag.k": "拖拽改期",
  "help.calendar.drag.v":
    "周 / 日视图拖块移动（15 分钟吸附）、拖上下边调时长、空白处拖出一块即创建；月视图把当天面板里的条目拖进任意日期格。松手才落库，toast 里随时反悔。",
  "help.calendar.recur.k": "重复日程",
  "help.calendar.recur.v":
    "每天 / 每周 X / 每月 X 日三种规则，编辑即改整个系列；把重复块拖到另一个星期几或日期，规则会跟着改写（toast 会告诉你改成了什么）。",
  "help.calendar.neighbor.k": "相邻月的日期",
  "help.calendar.neighbor.v": "月首尾会补齐相邻月的格子，颜色淡一些，但照常点击、创建、拖入——点一下就跳到那个月。",
  "help.calendar.holidays.k": "法定节假日",
  "help.calendar.holidays.v":
    "内置当年国务院安排；设置页可导入自定义 JSON（存在数据目录，优先于内置，校验通过才生效）。没数据的年份就不显示角标，仅此而已。",
  "help.calendar.conflict.k": "时间冲突",
  "help.calendar.conflict.v": "保存的日程和已有日程撞车时会提醒一句，但不会拦着你保存——你的日程你做主。",
  "help.tasks.title": "待办",
  "help.tasks.done.k": "完成",
  "help.tasks.done.v": "勾选即完成，完成后沉底；再勾一次就回来。红色只留给没做完的紧迫事。",
  "help.tasks.recur.k": "重复待办",
  "help.tasks.recur.v": "完成一次自动排到下一期；反悔取消，只退一期。",
  "help.tasks.batch.k": "批量操作",
  "help.tasks.batch.v": "待办视图里多选后可批量完成 / 改期 / 删除；删除有 5 秒后悔药，一键全部救回。",
  "help.reminders.title": "提醒",
  "help.reminders.intent.k": "提醒跟着条目走",
  "help.reminders.intent.v":
    "提醒记住的是「开始前 10 分钟」这样的意图，不是写死的时刻。条目改期，提醒自动跟上，不用逐条重设。",
  "help.reminders.catchup.k": "错过不轰炸",
  "help.reminders.catchup.v":
    "休眠或关机错过的提醒：窗口内（默认 120 分钟，可调）照常补发；窗口外只记一笔，最后汇成一条摘要通知。",
  "help.reminders.center.k": "提醒中心",
  "help.reminders.center.v": "侧栏 🔔 收着全部提醒历史，可以定位、完成、稍后提醒——系统通知丢了，这里是兜底。",
  "help.stats.title": "记录与统计",
  "help.stats.widgets.k": "统计挂件",
  "help.stats.widgets.v": "统计页由容器和挂件拼成：随意增删改、重命名；同一份数据，折线 / 柱状 / 热力随时换。",
  "help.stats.reset.k": "恢复默认",
  "help.stats.reset.v": "「恢复默认统计页」会整体重置回预置布局，自建的容器会被清掉——动手前想一下。",
  "help.overlay.title": "今日悬浮窗",
  "help.overlay.lock.k": "锁定 = 点击穿透",
  "help.overlay.lock.v": "锁定后悬浮窗不挡鼠标，点哪穿哪；解锁走托盘菜单。位置、大小、透明度都会被记住。",
  "help.overlay.readonly.k": "只读 + 勾选",
  "help.overlay.readonly.v": "悬浮窗只列今天没做完的事，唯一的操作是勾掉它们；要编辑请回主窗口。",
  "help.data.title": "数据与备份",
  "help.data.dir.k": "数据在哪",
  "help.data.dir.v": "一切都在 ~/.local/share/myday/：SQLite 库、附件、备份。整个目录拷走就完成迁移；MYDAY_DATA_DIR 可改道。",
  "help.data.backup.k": "备份",
  "help.data.backup.v": "设置页一键打包 zip（自动留最近 7 份）；恢复就是把 zip 解回去，再打开应用。",
  "help.data.migrate.k": "升级不吞数据",
  "help.data.migrate.v": "1.0 起结构升级只做增量迁移；万一打开的是更老的遗留库，也会先备份到 backups/ 再动手。",
  "help.data.cli.k": "CLI / agent",
  "help.data.cli.v":
    "myday 命令和界面共用同一个库：命令行写入，界面即时刷新。--json 信封稳定，幂等键和 --dry-run 齐备，随便接脚本。",
  "help.keys.title": "快捷键",
  "help.keys.quickadd": "快速添加",
  "help.keys.palette": "命令面板（动作 + 搜索混排）",
  "help.keys.viewswitch": "日历：今天 / 日 / 周 / 月",
  "help.keys.navigate": "日历：上一页 / 下一页",
  "help.keys.enteresc": "保存 / 取消（面板与弹窗通用）",
  "help.keys.paste": "粘贴截图",
  "help.keys.tab": "应用自然语言时间提示",
} as const;

export const part_help_en: Record<string, string> = {
  "help.title": "Help",
  "help.concepts.title": "Three item types",
  "help.concepts.types.k": "Event / Task / Log",
  "help.concepts.types.v":
    "Events are the plan — they start and end. Tasks are what you owe — they come due. Logs are what happened — they carry a time. All three live in one store, and MyDay files each new item by what you filled in.",
  "help.concepts.immutable.k": "Types don't change",
  "help.concepts.immutable.v":
    "You get one chance to change the type before saving; afterwards it's locked. Picked wrong? Delete and re-create. Tasks and events can still be converted from the detail panel (Convert to event / Log it).",
  "help.concepts.fields.k": "Custom fields",
  "help.concepts.fields.v":
    "Manage fields on the Templates page: text, number, select, multi-select, switch, date, link. Rename freely; deleting is soft — recreate the same name and everything comes back.",
  "help.capture.title": "Capture",
  "help.capture.quick.k": "Quick Add",
  "help.capture.quick.v":
    "Ctrl+N or a global hotkey opens a bare input: type a title, press Enter, done. Filled-in content decides the type — a due date makes a task, a time makes an event, plain text waits in the inbox.",
  "help.capture.nl.k": "Typing times",
  "help.capture.nl.v":
    "Type “tomorrow 9am” and a suggestion chip appears — it applies only when you press Tab. MyDay never rewrites what you typed. Prefer clicking? The time picker never needs the keyboard.",
  "help.capture.paste.k": "Screenshots",
  "help.capture.paste.v": "Ctrl+V saves the clipboard image as an attachment, as many as you like.",
  "help.capture.filelink.k": "File links",
  "help.capture.filelink.v":
    "Drag a file onto the panel (or paste a path and hit Enter) and MyDay remembers the path — your file stays untouched. 📎 opens it, 📁 reveals it.",
  "help.calendar.title": "Calendar",
  "help.calendar.drag.k": "Drag to reschedule",
  "help.calendar.drag.v":
    "In week/day view, drag blocks to move (15-minute snap), drag the edges to resize, drag across empty space to create; in month view, drag items from the day panel onto any date. Nothing is written until you let go — the toast offers a way back.",
  "help.calendar.recur.k": "Recurring events",
  "help.calendar.recur.v":
    "Daily, weekly and monthly rules; editing means the whole series. Drag a recurring block to another weekday or date and the rule rewrites itself — the toast tells you exactly what it became.",
  "help.calendar.neighbor.k": "Adjacent-month days",
  "help.calendar.neighbor.v":
    "Leading and trailing cells show neighboring months, slightly dimmed, but you can click, create and drop into them as usual — clicking jumps to that month.",
  "help.calendar.holidays.k": "Public holidays",
  "help.calendar.holidays.v":
    "The current year's official schedule ships built in; import your own JSON in Settings (stored in the data dir, takes priority, validated before use). Years without data simply show no badges.",
  "help.calendar.conflict.k": "Conflicts",
  "help.calendar.conflict.v": "If a saved event overlaps existing ones you'll hear about it — but the choice stays yours.",
  "help.tasks.title": "Tasks",
  "help.tasks.done.k": "Completing",
  "help.tasks.done.v": "Check to finish — done items sink to the bottom. Check again to bring one back. Red is reserved for urgency that's still unfinished.",
  "help.tasks.recur.k": "Recurring tasks",
  "help.tasks.recur.v": "Complete one and the next occurrence is already queued; change your mind and it steps back — one period, no more.",
  "help.tasks.batch.k": "Batch actions",
  "help.tasks.batch.v": "Multi-select in the Tasks view to complete, reschedule or delete in bulk; deletes come with a 5-second undo that brings everything back at once.",
  "help.reminders.title": "Reminders",
  "help.reminders.intent.k": "Reminders follow their items",
  "help.reminders.intent.v":
    "A reminder remembers “10 minutes before the start”, not a frozen timestamp. Move the item and the reminder moves with it — no re-setting anything.",
  "help.reminders.catchup.k": "No notification storms",
  "help.reminders.catchup.v":
    "Reminders missed while asleep: inside the window (default 120 minutes, adjustable) they fire normally; outside it they're noted silently and condensed into one summary.",
  "help.reminders.center.k": "Reminder center",
  "help.reminders.center.v": "The 🔔 in the sidebar keeps every reminder's history — locate, complete, snooze. When a system notification gets lost, this is the safety net.",
  "help.stats.title": "Logs & stats",
  "help.stats.widgets.k": "Stat widgets",
  "help.stats.widgets.v":
    "The stats page is containers plus widgets: add, edit, rename at will; the same data renders as a line, bars or a heatmap whenever you feel like a change.",
  "help.stats.reset.k": "Reset",
  "help.stats.reset.v": "“Restore default stats page” resets everything to the preset layout — custom containers go away, so think first.",
  "help.overlay.title": "Today overlay",
  "help.overlay.lock.k": "Lock = click-through",
  "help.overlay.lock.v": "When locked, the overlay never gets in the way of a click; unlock from the tray menu. Position, size and opacity are remembered.",
  "help.overlay.readonly.k": "Read-only + check",
  "help.overlay.readonly.v": "The overlay lists today's unfinished items, and the only thing you can do is finish them. For everything else, the main window.",
  "help.data.title": "Data & backup",
  "help.data.dir.k": "Where data lives",
  "help.data.dir.v":
    "Everything is in ~/.local/share/myday/ — the SQLite database, attachments, backups. Copy the folder and you've migrated; MYDAY_DATA_DIR redirects it.",
  "help.data.backup.k": "Backup",
  "help.data.backup.v":
    "One click in Settings packs a zip (the latest 7 are kept). Restoring means unzipping it back and opening the app.",
  "help.data.migrate.k": "Upgrades keep data",
  "help.data.migrate.v":
    "Since 1.0, schema upgrades only run additive migrations. If an older legacy database ever shows up, it gets backed up to backups/ before anything else happens.",
  "help.data.cli.k": "CLI / agents",
  "help.data.cli.v":
    "The myday CLI shares the GUI's database — write from a terminal, watch the window refresh. A stable --json envelope, idempotency keys and --dry-run make it easy to automate.",
  "help.keys.title": "Shortcuts",
  "help.keys.quickadd": "Quick Add",
  "help.keys.palette": "Command palette (actions + results)",
  "help.keys.viewswitch": "Calendar: today / day / week / month",
  "help.keys.navigate": "Calendar: previous / next page",
  "help.keys.enteresc": "Save / cancel (panels and windows)",
  "help.keys.paste": "Paste screenshot",
  "help.keys.tab": "Apply natural-language time chip",
};

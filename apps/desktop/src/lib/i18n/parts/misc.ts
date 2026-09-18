/**
 * i18n 词典分片：杂项命名空间（详情面板 / 字段类型 / 改期 toast / 重复规则 /
 * 时间占位池 / 假期文案）。zh 值为基准（e2e 可访问名基于中文文案，勿改中文值），
 * en 与它同 key 集。由装配层合并进 zh / en 主词典。
 *
 * 注意：本文件是纯数据模块（零依赖），recurrence.ts / tpltime.ts 等
 * golden 直跑 node 的纯模块允许安全 import，作为取词回退的中文基准。
 */

export const part_misc = {
  // ---- ItemDetail.svelte（详情面板）----
  "detail.title": "条目详情",
  "detail.status.todo": "未完成",
  "detail.status.done": "已完成",
  "detail.meta": "创建于 {created} · 修改于 {updated}",
  "detail.recurrence_line": "🔁 重复：{rule} · 编辑 = 修改整个系列",
  "detail.image_alt": "附件图片",
  "detail.all_day": "全天",
  "detail.start": "开始",
  "detail.end": "结束",
  "detail.due": "截止",
  "detail.occurred": "发生时间",
  "detail.completed": "完成于",
  "detail.bool_yes": "是",
  "detail.field_deleted": "{name}（已删除）",
  "detail.array_sep": "、",
  "detail.tags": "标签",
  "detail.reminders": "提醒",
  "detail.files_attachments": "文件 / 附件",
  "detail.files": "文件",
  "detail.preview_alt": "图片预览",
  "detail.converted_to_event": "已转为日程{name}，附件与标签已带过去",
  "detail.convert_event_failed": "转日程失败：{error}",
  "detail.converted_to_log": "已生成记录{name}，原日程保留",
  "detail.convert_log_failed": "生成记录失败：{error}",
  "detail.confirm_convert_event": "确认转日程？",
  "detail.to_event": "转日程",
  "detail.confirm_convert_log": "确认生成记录？",
  "detail.to_log": "生成记录",
  "detail.mark_todo": "改为未完成",
  "detail.mark_done": "标记完成",

  // ---- fields.svelte.ts（字段类型显示名；kind 值本身不翻译）----
  "fields.text": "文本",
  "fields.number": "数字",
  "fields.select": "单选",
  "fields.multiselect": "多选",
  "fields.bool": "开关",
  "fields.date": "日期",
  "fields.url": "链接",

  // ---- reschedule.ts（拖拽改期 toast）----
  "reschedule.day": "{m}月{d}日",
  "reschedule.moved": "已改期：{day} {start}–{end}{series}",
  "reschedule.resized": "时长已调整：{start}–{end}",
  "reschedule.due_moved": "截止已改到 {day}{clock}{series}",
  "reschedule.series_changed": "，整个系列已改{rule}",
  "reschedule.rule_weekly": "（规则改为每周{day}）",
  "reschedule.rule_monthly": "（规则改为每月{d}日）",

  // ---- recurrence.ts（重复规则显示名；解析与规则串不动）----
  "recurrence.daily": "每天",
  "recurrence.weekly": "每周{day}",
  "recurrence.monthly": "每月{d}日",
  "recurrence.wd1": "一",
  "recurrence.wd2": "二",
  "recurrence.wd3": "三",
  "recurrence.wd4": "四",
  "recurrence.wd5": "五",
  "recurrence.wd6": "六",
  "recurrence.wd7": "日",

  // ---- tpltime.ts（时间占位池显示标签；@token 文法不动）----
  "tpltime.now": "现在",
  "tpltime.now_30m": "30分钟前",
  "tpltime.now_1h": "1小时前",
  "tpltime.next_hour": "下一小时",
  "tpltime.d0_0900": "当天 09:00",
  "tpltime.d0_1800": "当天 18:00",
  "tpltime.d0_end": "当天结束",
  "tpltime.d1_0900": "明天 09:00",
  "tpltime.d1_end": "明天结束",
  "tpltime.start_30m": "开始后30分钟",

  // ---- holidays.svelte.ts（今天页假期文案；节日名来自 JSON 不翻译）----
  "holiday.in_progress": "{name}假期 第 {index}/{total} 天",
  "holiday.last_workday": "今天是{name}前最后一个工作日",
  "holiday.countdown": "距 {name} 假期还有 {days} 天",
} as const;

export const part_misc_en: Record<string, string> = {
  // ---- ItemDetail.svelte ----
  "detail.title": "Item details",
  "detail.status.todo": "Not done",
  "detail.status.done": "Done",
  "detail.meta": "Created {created} · Updated {updated}",
  "detail.recurrence_line": "🔁 Repeats: {rule} · editing changes the whole series",
  "detail.image_alt": "Attachment image",
  "detail.all_day": "All day",
  "detail.start": "Start",
  "detail.end": "End",
  "detail.due": "Due",
  "detail.occurred": "Occurred",
  "detail.completed": "Completed",
  "detail.bool_yes": "Yes",
  "detail.field_deleted": "{name} (deleted)",
  "detail.array_sep": ", ",
  "detail.tags": "Tags",
  "detail.reminders": "Reminders",
  "detail.files_attachments": "Files / attachments",
  "detail.files": "Files",
  "detail.preview_alt": "Image preview",
  "detail.converted_to_event": "Converted to event {name}; attachments and tags carried over",
  "detail.convert_event_failed": "Convert to event failed: {error}",
  "detail.converted_to_log": "Log created as {name}; original event kept",
  "detail.convert_log_failed": "Create log failed: {error}",
  "detail.confirm_convert_event": "Convert to event?",
  "detail.to_event": "Convert to event",
  "detail.confirm_convert_log": "Create log?",
  "detail.to_log": "Create log",
  "detail.mark_todo": "Mark as not done",
  "detail.mark_done": "Mark done",

  // ---- fields.svelte.ts ----
  "fields.text": "Text",
  "fields.number": "Number",
  "fields.select": "Select",
  "fields.multiselect": "Multi-select",
  "fields.bool": "Toggle",
  "fields.date": "Date",
  "fields.url": "Link",

  // ---- reschedule.ts ----
  "reschedule.day": "{m}/{d}",
  "reschedule.moved": "Rescheduled: {day} {start}–{end}{series}",
  "reschedule.resized": "Duration adjusted: {start}–{end}",
  "reschedule.due_moved": "Due moved to {day}{clock}{series}",
  "reschedule.series_changed": ", whole series updated{rule}",
  "reschedule.rule_weekly": " (rule now weekly on {day})",
  "reschedule.rule_monthly": " (rule now monthly on day {d})",

  // ---- recurrence.ts ----
  "recurrence.daily": "Daily",
  "recurrence.weekly": "Weekly on {day}",
  "recurrence.monthly": "Monthly on day {d}",
  "recurrence.wd1": "Mon",
  "recurrence.wd2": "Tue",
  "recurrence.wd3": "Wed",
  "recurrence.wd4": "Thu",
  "recurrence.wd5": "Fri",
  "recurrence.wd6": "Sat",
  "recurrence.wd7": "Sun",

  // ---- tpltime.ts ----
  "tpltime.now": "Now",
  "tpltime.now_30m": "30 min ago",
  "tpltime.now_1h": "1 hour ago",
  "tpltime.next_hour": "Next hour",
  "tpltime.d0_0900": "Today 09:00",
  "tpltime.d0_1800": "Today 18:00",
  "tpltime.d0_end": "End of today",
  "tpltime.d1_0900": "Tomorrow 09:00",
  "tpltime.d1_end": "End of tomorrow",
  "tpltime.start_30m": "30 min after start",

  // ---- holidays.svelte.ts ----
  "holiday.in_progress": "{name} holiday: day {index} of {total}",
  "holiday.last_workday": "Today is the last workday before {name}",
  "holiday.countdown": "{days} days until {name}",
};

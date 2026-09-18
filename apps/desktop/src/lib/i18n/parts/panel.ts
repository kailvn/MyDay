/**
 * 统一条目面板（ItemPanel.svelte，ns panel.*）。
 * 共享词（common.* / type.*）直接引用，不在本分片重复。
 */
export const part_panel = {
  // ---- 时间字段 / 时间组标签（label 存 key，模板里 t(label)） ----
  'panel.time.start': '开始',
  'panel.time.end': '结束',
  'panel.time.due': '截止',
  'panel.time.occurred': '发生时间',
  'panel.group.time': '时间',

  // ---- 时间快捷 chip：开始行 ----
  'panel.start.nextHour': '下一整点',
  'panel.start.plus1h': '一小时后',
  'panel.start.tomorrow9': '明天 9:00',

  // ---- 时间快捷 chip：结束行 ----
  'panel.end.plus30m': '半小时后',
  'panel.end.plus1h': '一小时后',
  'panel.end.plus2h': '两小时后',

  // ---- 时段 chip ----
  'panel.range.allDay': '全天',
  'panel.range.work': '工作时间',
  'panel.range.lunch': '午休',
  'panel.label.ranges': '时段',

  // ---- 时间行内提示 / 操作 ----
  'panel.quick.tomorrow': '明天',
  'panel.quick.now': '现在',
  'panel.time.allDayEnd': '当天 23:59 结束',
  'panel.time.emptyEnd': '留空 = 开始 + 1 小时',
  'panel.time.deactivate': '取消激活（类型回退）',

  // ---- 头部：类型识别 / 改口 ----
  'panel.header.type': '类型',
  'panel.header.autoType': '自动：{type}',
  'panel.header.editHint': '类型创建即定 · 可用下方按钮转换',
  'panel.header.inferHint': '时间→日程 · 截止→待办 · 发生时间→记录（保存前可改一次）',

  // ---- 标题输入 ----
  'panel.title.createPlaceholder': '记点什么…',
  'panel.title.tplPlaceholder': '默认标题（空 = 应用时用模板名）',
  'panel.timeHint.tabApply': 'Tab 应用',

  // ---- 模板模式 ----
  'panel.badge.template': '模板',
  'panel.label.template': '模板',
  'panel.tpl.headerHint': '填默认值存为模板；时间存占位、应用时解析',
  'panel.tpl.namePlaceholder': '模板名（必填，如 喝水）',
  'panel.tpl.iconTitle': 'emoji 图标（可选）',
  'panel.tpl.tagPlaceholder': '#标签（可选）',
  'panel.tpl.notePlaceholder': '说明（可选）',
  'panel.tpl.showInLogPage': '在记录页显示按钮',
  'panel.tpl.noDefault': '（不设默认）',
  'panel.tpl.resolve': '应用时 ≈ {time}',
  'panel.tpl.invalidToken': '无效占位',
  'panel.tpl.endHint': '留空 = 开始 + 1 小时；相对占位可固定时长',
  'panel.tpl.removeDefault': '移除该时间默认',
  'panel.tpl.applyTitle': '应用模板{name}',
  'panel.tpl.taskHint': '新建的待办恒为未完成，完成后在待办页勾选',

  // ---- 附件 ----
  'panel.label.attachments': '附件',
  'panel.attach.pastedAlt': '粘贴的截图',
  'panel.attach.removeImage': '移除截图',
  'panel.attach.dropEdit': '拖文件进窗口',
  'panel.attach.dropCreate': '拖文件进窗口，或 Ctrl+V 粘贴截图',
  'panel.attach.pathPlaceholder': '粘贴路径回车（只记链接不复制）',
  'panel.attach.previewAlt': '截图预览',

  // ---- 更多 / 重复 ----
  'panel.more.collapse': '收起详情 ▴',
  'panel.more.expand': '＋ 时间 / 字段 / 标签 ▾',
  'panel.label.repeat': '重复',
  'panel.rec.none': '不重复',
  'panel.rec.daily': '每天',
  'panel.rec.weekly': '每周…',
  'panel.rec.monthly': '每月…',
  'panel.rec.dows': '一,二,三,四,五,六,日',
  'panel.rec.monthDay': '日（当月不足取月末）',
  'panel.rec.editAll': '编辑即改整个系列',

  // ---- 自定义字段 ----
  'panel.label.fields': '字段',
  'panel.field.unset': '{name}：未选',
  'panel.field.multiHint': '全不选 = 清除',
  'panel.field.unitPlaceholder': '{name}（{unit}）',
  'panel.fields.addOption': '＋ 添加字段…',

  // ---- 标签 ----
  'panel.label.tags': '标签',
  'panel.tags.placeholder': '#标签 回车',

  // ---- 提醒 ----
  'panel.label.reminders': '提醒',
  'panel.reminders.addOption': '＋ 提醒…',
  'panel.reminders.relativeHint': '相对提醒随开始/截止自动跟随',
  'panel.reminders.pickTime': '（选时刻）',
  'panel.reminders.custom': '自定义时刻…',
  'panel.rem.atStart': '开始时',
  'panel.rem.startBefore10m': '开始前 10 分钟',
  'panel.rem.startBefore1h': '开始前 1 小时',
  'panel.rem.startBefore1d': '开始前 1 天',
  'panel.rem.atDue': '截止时',
  'panel.rem.dueBefore1h': '截止前 1 小时',
  'panel.rem.dueBefore1d': '截止前 1 天',
  'panel.rem.daily9': '期间每天 09:00',

  // ---- 备注 / 元信息 ----
  'panel.note.tplPlaceholder': '默认备注（可选）',
  'panel.note.placeholder': '备注（可选）',
  'panel.meta.timestamps': '创建于 {created} · 修改于 {updated}',

  // ---- 底部操作 ----
  'panel.actions.editTpl': '修改模板',
  'panel.actions.newTpl': '新建模板',
  'panel.actions.tplLine': '{action}：{type}',
  'panel.actions.willCreate': '将创建{type}',
  'panel.actions.shortcutHint': 'Ctrl+Enter 保存 · Esc 取消',
  'panel.actions.saving': '保存中…',
  'panel.actions.saveTpl': '保存模板',
  'panel.actions.saveAsTpl': '存为模板',
  'panel.actions.saveEdit': '保存修改',
  'panel.convert.toEvent': '转日程',
  'panel.convert.toEventConfirm': '确认转日程？',
  'panel.convert.toLog': '生成记录',
  'panel.convert.toLogConfirm': '确认生成记录？',

  // ---- 独立窗口 / 无障碍 ----
  'panel.win.closeTitle': '关闭（Esc）',
  'panel.aria.tplEditor': '模板编辑',
  'panel.aria.editItem': '编辑条目',
  'panel.aria.newItem': '新建条目',

  // ---- 空标题自动生成 ----
  'panel.autotitle.shot': '截图 {time}',
  'panel.autotitle.files': '{name} 等 {n} 个文件',
  'panel.autotitle.file': '文件',
  'panel.autotitle.quick': '速记 {time}',

  // ---- toast ----
  'panel.toast.disabledFields': '模板有 {n} 个字段已停用，本次未启用',
  'panel.toast.quickSaved': '已保存为速记{title}',
  'panel.toast.conflict': '与 {n} 个日程时间冲突：{names}',
  'panel.toast.conflictMore': '等 {n} 个',
  'panel.toast.conflictSep': '、',
  'panel.toast.toEventDone': '已转为日程{title}（附件与标签已带过去）',
  'panel.toast.toLogDone': '已生成记录{title}，原日程保留',

  // ---- 保存前校验 ----
  'panel.validate.tplNameRequired': '模板名不能为空',
  'panel.validate.occurredToken': '发生时间占位只允许 @now / @now-…（记录不能发生在未来）',
  'panel.validate.eventExtraTimes': '日程不允许截止/发生时间，请先移除对应字段',
  'panel.validate.taskOccurred': '待办没有发生时间，请先移除',
  'panel.validate.logTimes': '记录没有开始/结束/截止时间，请先移除',
  'panel.validate.allDayDate': '全天日程需要日期',
  'panel.validate.eventStart': '日程必须带开始时间',
  'panel.validate.endAfterStart': '结束时间需晚于开始时间',
  'panel.validate.logTitleOrNote': '记录需要标题或备注至少一项',
  'panel.validate.logNotFuture': '记录的发生时间不能晚于现在',
  'panel.validate.titleRequired': '标题不能为空',
} as const;

export const part_panel_en: Record<string, string> = {
  'panel.time.start': 'Start',
  'panel.time.end': 'End',
  'panel.time.due': 'Due',
  'panel.time.occurred': 'Occurred',
  'panel.group.time': 'Time',

  'panel.start.nextHour': 'Next hour',
  'panel.start.plus1h': 'In 1 hour',
  'panel.start.tomorrow9': 'Tomorrow 9:00',

  'panel.end.plus30m': 'Start + 30 min',
  'panel.end.plus1h': 'Start + 1 hour',
  'panel.end.plus2h': 'Start + 2 hours',

  'panel.range.allDay': 'All day',
  'panel.range.work': 'Work hours',
  'panel.range.lunch': 'Lunch',
  'panel.label.ranges': 'Ranges',

  'panel.quick.tomorrow': 'Tomorrow',
  'panel.quick.now': 'Now',
  'panel.time.allDayEnd': 'Ends 23:59 same day',
  'panel.time.emptyEnd': 'Empty = start + 1 hour',
  'panel.time.deactivate': 'Deactivate (type reverts)',

  'panel.header.type': 'Type',
  'panel.header.autoType': 'Auto: {type}',
  'panel.header.editHint': 'Type is fixed at creation · convert below',
  'panel.header.inferHint': 'Time→event · Due→task · Occurred→log (changeable once before save)',

  'panel.title.createPlaceholder': 'Jot something…',
  'panel.title.tplPlaceholder': 'Default title (empty = template name on apply)',
  'panel.timeHint.tabApply': 'Tab to apply',

  'panel.badge.template': 'Template',
  'panel.label.template': 'Templates',
  'panel.tpl.headerHint': 'Save defaults as a template; times are tokens resolved on apply',
  'panel.tpl.namePlaceholder': 'Template name (required, e.g. Drink water)',
  'panel.tpl.iconTitle': 'Emoji icon (optional)',
  'panel.tpl.tagPlaceholder': '#tag (optional)',
  'panel.tpl.notePlaceholder': 'Description (optional)',
  'panel.tpl.showInLogPage': 'Show button on Logs page',
  'panel.tpl.noDefault': '(no default)',
  'panel.tpl.resolve': 'Resolves to ≈ {time}',
  'panel.tpl.invalidToken': 'invalid token',
  'panel.tpl.endHint': 'Empty = start + 1 hour; relative tokens fix the duration',
  'panel.tpl.removeDefault': 'Remove this time default',
  'panel.tpl.applyTitle': 'Apply template {name}',
  'panel.tpl.taskHint': 'New tasks always start unfinished; tick them off when done',

  'panel.label.attachments': 'Attachments',
  'panel.attach.pastedAlt': 'Pasted screenshot',
  'panel.attach.removeImage': 'Remove screenshot',
  'panel.attach.dropEdit': 'Drop files into the window',
  'panel.attach.dropCreate': 'Drop files into the window, or Ctrl+V to paste screenshots',
  'panel.attach.pathPlaceholder': 'Paste a path, press Enter (linked, not copied)',
  'panel.attach.previewAlt': 'Screenshot preview',

  'panel.more.collapse': 'Collapse details ▴',
  'panel.more.expand': '＋ Time / Fields / Tags ▾',
  'panel.label.repeat': 'Repeat',
  'panel.rec.none': 'No repeat',
  'panel.rec.daily': 'Daily',
  'panel.rec.weekly': 'Weekly…',
  'panel.rec.monthly': 'Monthly…',
  'panel.rec.dows': 'Mon,Tue,Wed,Thu,Fri,Sat,Sun',
  'panel.rec.monthDay': 'day (clamped to month end)',
  'panel.rec.editAll': 'edits apply to the whole series',

  'panel.label.fields': 'Fields',
  'panel.field.unset': '{name}: not set',
  'panel.field.multiHint': 'Deselect all to clear',
  'panel.field.unitPlaceholder': '{name} ({unit})',
  'panel.fields.addOption': '＋ Add field…',

  'panel.label.tags': 'Tags',
  'panel.tags.placeholder': '#tag, Enter',

  'panel.label.reminders': 'Reminders',
  'panel.reminders.addOption': '＋ Reminder…',
  'panel.reminders.relativeHint': 'Relative reminders follow start/due',
  'panel.reminders.pickTime': '(pick a time)',
  'panel.reminders.custom': 'Custom time…',
  'panel.rem.atStart': 'At start',
  'panel.rem.startBefore10m': '10 min before start',
  'panel.rem.startBefore1h': '1 hour before start',
  'panel.rem.startBefore1d': '1 day before start',
  'panel.rem.atDue': 'At due',
  'panel.rem.dueBefore1h': '1 hour before due',
  'panel.rem.dueBefore1d': '1 day before due',
  'panel.rem.daily9': 'Daily 09:00 in between',

  'panel.note.tplPlaceholder': 'Default note (optional)',
  'panel.note.placeholder': 'Note (optional)',
  'panel.meta.timestamps': 'Created {created} · Updated {updated}',

  'panel.actions.editTpl': 'Edit template',
  'panel.actions.newTpl': 'New template',
  'panel.actions.tplLine': '{action}: {type}',
  'panel.actions.willCreate': 'Will create {type}',
  'panel.actions.shortcutHint': 'Ctrl+Enter save · Esc cancel',
  'panel.actions.saving': 'Saving…',
  'panel.actions.saveTpl': 'Save template',
  'panel.actions.saveAsTpl': 'Save as template',
  'panel.actions.saveEdit': 'Save changes',
  'panel.convert.toEvent': 'To event',
  'panel.convert.toEventConfirm': 'Convert to event?',
  'panel.convert.toLog': 'Create log',
  'panel.convert.toLogConfirm': 'Create log copy?',

  'panel.win.closeTitle': 'Close (Esc)',
  'panel.aria.tplEditor': 'Template editor',
  'panel.aria.editItem': 'Edit item',
  'panel.aria.newItem': 'New item',

  'panel.autotitle.shot': 'Screenshot {time}',
  'panel.autotitle.files': '{name} and {n} more files',
  'panel.autotitle.file': 'file',
  'panel.autotitle.quick': 'Quick note {time}',

  'panel.toast.disabledFields': '{n} template fields are disabled and were skipped',
  'panel.toast.quickSaved': 'Saved as quick note {title}',
  'panel.toast.conflict': 'Conflicts with {n} events: {names}',
  'panel.toast.conflictMore': 'and {n} in total',
  'panel.toast.conflictSep': ', ',
  'panel.toast.toEventDone': 'Converted to event {title} (attachments and tags carried over)',
  'panel.toast.toLogDone': 'Created log {title}; the event is kept',

  'panel.validate.tplNameRequired': 'Template name is required',
  'panel.validate.occurredToken': 'Occurred token must be @now / @now-… (logs cannot be in the future)',
  'panel.validate.eventExtraTimes': 'Events cannot have due/occurred time — remove them first',
  'panel.validate.taskOccurred': 'Tasks have no occurred time — remove it first',
  'panel.validate.logTimes': 'Logs have no start/end/due time — remove them first',
  'panel.validate.allDayDate': 'All-day events need a date',
  'panel.validate.eventStart': 'Events need a start time',
  'panel.validate.endAfterStart': 'End time must be after start',
  'panel.validate.logTitleOrNote': 'Logs need a title or a note',
  'panel.validate.logNotFuture': 'Log time cannot be in the future',
  'panel.validate.titleRequired': 'Title cannot be empty',
};

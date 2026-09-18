/**
 * 日历视图（CalendarView，ns calendar.*）与周/日时间网格（WeekGrid，ns week.*）
 * 的词典分片。part_calendar 为中文基准（语义化 key），part_calendar_en 与它同
 * key 集。复用共享词典：common.*（today/undo/done/…）与 type.event/task/log。
 *
 * 星期几与月份名不建 key：一律由组件用
 * `toLocaleDateString(i18n.locale === 'en' ? 'en-US' : 'zh-CN', …)` 取词。
 */
export const part_calendar = {
  // ---- calendar.*（CalendarView：月视图 + 当天面板）----
  'calendar.title': '日历',
  'calendar.mode_month': '月',
  'calendar.mode_week': '周',
  'calendar.mode_day': '日',
  'calendar.prev_month': '上一月（←）',
  'calendar.next_month': '下一月（→）',
  'calendar.go_today': '回到今天（T）',
  'calendar.mode_tabs_tip': 'D / W / M 切换视图',
  'calendar.all_day': '全天',
  'calendar.hol_off': '休',
  'calendar.hol_work': '班',
  'calendar.cell_tip': '单击选中 · 双击或 ＋ 快速添加',
  'calendar.section_due': '到期',
  'calendar.add_new': '＋ 新建',
  'calendar.day_empty': '当天暂无条目 · 双击格子或点 {name}',
  'calendar.row_drag_tip': '点击看详情 · 按住可拖到日期格改期',
  'calendar.mark_done': '标记为已完成',
  'calendar.mark_undone': '标记为未完成',
  'calendar.reschedule_failed': '改期失败：{error}',
  'calendar.update_failed': '更新失败：{error}',
  'calendar.ghost_day': '{n} 日',

  // ---- week.*（WeekGrid：周/日时间网格）----
  'week.all_day': '全天',
  'week.hol_off': '休',
  'week.hol_work': '班',
  'week.due_n': '🔴 到期 {n}',
  'week.slot_tip': '单击选中 · 双击新建 · 拖动选时段',
  'week.event_drag_tip': '（拖动改期 / 边缘调时长）',
  'week.release_create': '松开创建 {from} – {to}',
  'week.drag_longer': '再拖长一点（≥15 分钟）',
  'week.release_apply': '{range}{day} · 松开生效',
  'week.reschedule_failed': '改期失败：{error}',
  'week.selected_prefix': '已选 {range} · 双击或',
  'week.create_here': '在此新建',
} as const;

export const part_calendar_en: Record<string, string> = {
  // ---- calendar.* ----
  'calendar.title': 'Calendar',
  'calendar.mode_month': 'Month',
  'calendar.mode_week': 'Week',
  'calendar.mode_day': 'Day',
  'calendar.prev_month': 'Previous month (←)',
  'calendar.next_month': 'Next month (→)',
  'calendar.go_today': 'Back to today (T)',
  'calendar.mode_tabs_tip': 'D / W / M to switch views',
  'calendar.all_day': 'All day',
  'calendar.hol_off': 'Off',
  'calendar.hol_work': 'Work',
  'calendar.cell_tip': 'Click to select · double-click or + to quick-add',
  'calendar.section_due': 'Due',
  'calendar.add_new': '+ New',
  'calendar.day_empty': 'Nothing scheduled this day · double-click a cell or click {name}',
  'calendar.row_drag_tip': 'Click for details · hold and drag onto a date cell to reschedule',
  'calendar.mark_done': 'Mark as done',
  'calendar.mark_undone': 'Mark as not done',
  'calendar.reschedule_failed': 'Reschedule failed: {error}',
  'calendar.update_failed': 'Update failed: {error}',
  'calendar.ghost_day': '{n}',

  // ---- week.* ----
  'week.all_day': 'All day',
  'week.hol_off': 'Off',
  'week.hol_work': 'Work',
  'week.due_n': '🔴 Due {n}',
  'week.slot_tip': 'Click to select · double-click to create · drag to pick a range',
  'week.event_drag_tip': '(drag to move; edges to resize)',
  'week.release_create': 'Release to create {from} – {to}',
  'week.drag_longer': 'Drag a bit longer (≥15 min)',
  'week.release_apply': '{range}{day} · release to apply',
  'week.reschedule_failed': 'Reschedule failed: {error}',
  'week.selected_prefix': 'Selected {range} · double-click or',
  'week.create_here': 'create here',
};

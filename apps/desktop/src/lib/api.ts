/**
 * Tauri 命令封装 —— 与 myday-core 数据模型一一对应（SCHEMA-REDESIGN 定稿）。
 * JSON 字段名即 API。
 */

import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import { t, i18n, type MessageKey } from "./i18n";

export type ItemType = "event" | "task" | "log";
/** 待办只有两态 */
export type ItemStatus = "todo" | "done";

/** 文件链接保留键：extra 中唯一不要求是字段 id 的键（值 = 路径字符串数组） */
export const FILE_LINKS_KEY = "文件";

export interface Attachment {
  id: number;
  item_id: string;
  rel_path: string;
  mime: string;
  size: number;
  created_at: string;
}

/** 提醒：存意图 spec —— @token（相对，随条目时间自动跟随）或 RFC3339（绝对一次性） */
export interface Reminder {
  id: number;
  item_id: string;
  spec: string;
  /** notify / sound / popup */
  channel: string;
}

export interface NewReminder {
  spec: string;
  channel?: string;
}

/** 字段类型（Notion 属性类型子集） */
export type FieldKind =
  | "text"
  | "number"
  | "select"
  | "multiselect"
  | "bool"
  | "date"
  | "url";

/** 字段定义：值存 Item.extra[字段id]，类型配置在 options；软删后同名可重建 */
export interface FieldDef {
  id: string;
  name: string;
  kind: FieldKind;
  options: { unit?: string; choices?: string[]; [k: string]: unknown };
  /** null = 全局 */
  scope: ItemType | null;
  sort: number;
  builtin: boolean;
}

export interface Item {
  id: string;
  type: ItemType;
  /** 可空（log 允许只有 note）；展示用 displayTitle(item) */
  title: string | null;
  note: string | null;
  start_at: string | null;
  end_at: string | null;
  all_day: boolean;
  due_at: string | null;
  due_all_day: boolean;
  /** 记录发生时间（log 必有，与 created_at 分离） */
  occurred_at: string | null;
  /** 仅 task 有值 */
  status: ItemStatus | null;
  completed_at: string | null;
  /** 重复规则（SPRINT-SPEC §2）：@daily / @weekly:n / @monthly:d；null = 不重复 */
  recurrence: string | null;
  template_id: string | null;
  reminders: Reminder[];
  tags: string[];
  attachments: Attachment[];
  idempotency_key: string | null;
  created_at: string;
  updated_at: string;
  /** 字段值（键 = 字段 id；保留键 FILE_LINKS_KEY 除外） */
  extra: Record<string, unknown>;
}

export interface NewItem {
  item_type?: ItemType | null;
  title?: string | null;
  note?: string | null;
  start_at?: string | null;
  end_at?: string | null;
  all_day?: boolean;
  due_at?: string | null;
  due_all_day?: boolean;
  occurred_at?: string | null;
  status?: ItemStatus | null;
  /** 重复规则（@daily / @weekly:n / @monthly:d），缺省不重复 */
  recurrence?: string | null;
  template_id?: string | null;
  /** 模板时间占位的解析锚点日（YYYY-MM-DD 本地日；缺省今天，一般不用传） */
  anchor_day?: string | null;
  reminders?: NewReminder[];
  tags?: string[];
  idempotency_key?: string | null;
  /** 字段值（键 = 字段 id） */
  extra?: Record<string, unknown>;
}

export interface ItemPatch {
  title?: string | null;
  note?: string | null;
  start_at?: string | null;
  end_at?: string | null;
  all_day?: boolean;
  due_at?: string | null;
  due_all_day?: boolean;
  occurred_at?: string | null;
  status?: ItemStatus;
  /** 设置 / 变更重复规则（严格校验，非法报错） */
  recurrence?: string | null;
  /** 清除重复规则 */
  clear_recurrence?: boolean;
  tags?: string[];
  /** 整体替换提醒集合 */
  reminders?: NewReminder[];
  clear_reminders?: boolean;
  /** 清空对应时间字段（event 的 start/end 不可清） */
  clear_start_at?: boolean;
  clear_end_at?: boolean;
  clear_due_at?: boolean;
  /** 字段值：整对象替换 */
  extra?: Record<string, unknown>;
}

export interface ListFilter {
  item_type?: ItemType | null;
  /** 锚点随类型：log → occurred_at，其余 → start_at */
  from?: string | null;
  to?: string | null;
  /** 列级区间（AND 语义，与 from/to 正交；日历请用 listItemsWindow 的 OR 语义） */
  start_from?: string | null;
  start_to?: string | null;
  end_from?: string | null;
  end_to?: string | null;
  occurred_from?: string | null;
  occurred_to?: string | null;
  /** todo / done（待办仅两态） */
  status?: ItemStatus | null;
  tag?: string | null;
  limit?: number | null;
  offset?: number;
  order?: "asc" | "desc";
  /** 活动过滤：该日期（YYYY-MM-DD，本地时区）创建或修改过的条目 */
  changed_on?: string | null;
  /** due_at 字段区间过滤（与 from/to 正交） */
  due_from?: string | null;
  due_to?: string | null;
}

export interface Template {
  id: string;
  name: string;
  tag: string | null;
  /** emoji 图标 */
  icon: string | null;
  /** 模板归属类型 */
  item_type: ItemType;
  /** 默认值：key = items 列白名单成员 或 字段 id */
  defaults: Record<string, unknown>;
  /** 随模板启用的字段定义（应用时一次性物化，builtin = 1） */
  fields: unknown[];
  note: string | null;
  sort: number;
  pinned: boolean;
  builtin: boolean;
}

export interface SearchHit {
  item: Item;
  matched_in: string[];
}

/** 提醒中心历史条目（SPRINT2-SPEC §5） */
export interface ReminderHistoryEntry {
  remind_at: string;
  item: Item;
}

// ----------------------------------------------------------------------
// 视图模型（FILTER-SPEC v1）：条件 AST / 数据集 / 挂件配置
// ----------------------------------------------------------------------

export type Panel = "logs" | "tasks" | "search" | "stats";
export type Logic = "and" | "or";
export type Cmp =
  | "eq" | "neq" | "contains" | "not_contains"
  | "gt" | "gte" | "lt" | "lte" | "between"
  | "any" | "all" | "none" | "is_true" | "is_false"
  | "on" | "before" | "after" | "within"
  | "empty" | "not_empty";

/** 日期值：点值 {day}|{rel} 或范围值 {rel: this_week|this_month|last_days:N} */
export type DateValue =
  | { day: string }
  | { rel: "today" | "tomorrow" | "yesterday" | "this_week" | "this_month" | `last_days:${number}` };

export type FilterValue =
  | boolean
  | number
  | string
  | string[]
  | DateValue
  | { from: DateValue; to: DateValue }
  | { from: number; to: number };

export interface Condition {
  field: string;
  cmp: Cmp;
  value?: FilterValue;
}

/** AST 节点：组（op + children）或条件（field + cmp + value） */
export type FilterNode =
  | { op: Logic; children: FilterNode[] }
  | Condition;

export interface SortSpec {
  field: string;
  dir: "asc" | "desc";
}

export interface GroupSpec {
  bucket: "day";
  field: string;
}

export interface Dataset {
  item_type: "all" | ItemType;
  filter: FilterNode;
  sort: SortSpec[];
  group?: GroupSpec | null;
  limit?: number | null;
}

export interface ViewConfig {
  dataset: Dataset;
  layout?: string | null;
  visible?: string[] | null;
}

export type Render = "bar" | "line" | "pie" | "heatmap" | "card";

export interface WidgetConfig {
  kind: "widget";
  dataset: { item_type: "all" | ItemType; filter: FilterNode };
  window?: { days: number } | { all: boolean } | null;
  agg?: {
    group?: { by: "time"; bucket: "day" | "week" | "month"; time_field: string }
        | { by: "field"; field: string }
        | null;
    metric?: { fn: "count" | "sum" | "avg" | "min" | "max" | "values"; field?: string | null } | null;
  } | null;
  derived?: { kind?: "streak" | null; goal?: { daily: number } | { weekly: number } | null } | null;
  render: Render;
  options?: { presence?: boolean; unit?: string | null; top_n?: number };
  title?: string | null;
  icon?: string | null;
}

/** view_defs 行（生效配置 = config_user ?? config） */
export interface ViewDef {
  id: string;
  name: string;
  panel: Panel;
  config: Record<string, unknown>;
  config_user: Record<string, unknown> | null;
  builtin: boolean;
  sort: number;
  created_at: string;
  updated_at: string;
}

export interface Bucket { key: string; value: number }
export interface Point { t: string; v: number }

export interface WidgetResult {
  view_id: string;
  key: string;
  title: string;
  icon?: string | null;
  render: Render;
  buckets?: Bucket[] | null;
  points?: Point[] | null;
  labels?: Record<string, string>;
  derived?: {
    streak?: { current: number; longest: number; recent: number; week_remaining?: number };
    total?: number; min?: number; max?: number; last?: number;
  };
  unit?: string | null;
  presence?: boolean;
  error?: string | null;
}

/**
 * 统计容器（v2.1）：布局 + 挂件列表。预置容器 = 开库时替用户建好的普通容器，
 * 与自建容器同权（可编辑可真删）；「恢复默认统计页」重新铺缺失的预设。
 */
export interface ContainerConfig {
  kind: "container";
  layout: "horizontal" | "vertical";
  widgets: WidgetConfig[];
}

export interface ContainerMeta {
  view_id: string;
  name: string;
  layout: "horizontal" | "vertical";
}

export interface StatsPageResult {
  evaluated_at: string;
  tz: string;
  containers: ContainerMeta[];
  widgets: WidgetResult[];
}

// ----------------------------------------------------------------------
// 今日悬浮窗（OVERLAY-SPEC v1）
// ----------------------------------------------------------------------

/** 悬浮窗单条条目（Rust 求值，只读展示用） */
export interface OverlayEntry {
  id: string;
  kind: "event" | "task";
  title: string;
  start_at: string | null;
  end_at: string | null;
  due_at: string | null;
  all_day: boolean;
  due_all_day: boolean;
  /** 日程进行中（左条高亮） */
  in_progress: boolean;
  created_at: string;
}

/** `overlay_today` 输出信封（§7） */
export interface OverlayToday {
  date: string;
  events: OverlayEntry[];
  tasks: OverlayEntry[];
  done_count: number;
  overdue_count: number;
  unscheduled_count: number;
}

/** 悬浮窗配置（settings KV 整体读写，§6） */
export interface OverlayConfig {
  enabled: boolean;
  /** "tr" | "tl" */
  corner: string;
  /** 拖动后的自定义位置（逻辑坐标） */
  custom_pos: { x: number; y: number } | null;
  /** 拖边调整后的自定义尺寸（逻辑坐标）；null = 默认 264×380（v1.1） */
  size: { w: number; h: number } | null;
  /** 0.30–1.00 */
  opacity: number;
  locked: boolean;
}

export interface ItemGroup {
  key?: string | null;
  label: string;
  items: Item[];
}

export interface ViewResult {
  view_id: string;
  name: string;
  panel: Panel;
  evaluated_at: string;
  tz: string;
  customized: boolean;
  config: ViewConfig | Record<string, unknown>;
  group?: GroupSpec | null;
  groups?: ItemGroup[] | null;
  items?: Item[] | null;
  matched?: Record<string, string[]> | null;
  widgets?: WidgetResult[] | null;
  total: number;
}

// ----------------------------------------------------------------------
// 命令封装
// ----------------------------------------------------------------------

export const api = {
  listItems: (filter: ListFilter) => invoke<Item[]>("list_items", { filter }),
  /** 窗口查询（日历）：[from,to] 内「所有内置时间字段相关」的条目（OR 语义） */
  listItemsWindow: (from: string, to: string, itemType?: ItemType | null) =>
    invoke<Item[]>("list_items_window", { from, to, itemType: itemType ?? null }),
  tasksView: (view: "today" | "upcoming" | "all" | "done") =>
    invoke<Item[]>("tasks_view", { view }),
  getItem: (id: string) => invoke<Item>("get_item", { id }),
  addItem: (newItem: NewItem) => invoke<Item>("add_item", { new: newItem }),
  updateItem: (id: string, patch: ItemPatch) => invoke<Item>("update_item", { id, patch }),
  deleteItem: (id: string) => invoke<Item>("delete_item", { id }),
  completeTask: (id: string) => invoke<Item>("complete_task", { id }),
  snooze: (id: string, until: string) => invoke<Item>("snooze", { id, until }),
  searchItems: (query: string, itemType?: ItemType | null) =>
    invoke<SearchHit[]>("search_items", { query, itemType: itemType ?? null }),
  listTemplates: () => invoke<Template[]>("list_templates"),
  // 注意：Tauri 平铺命令参数要求 camelCase 键（itemType），snake_case 会被
  // Rust 侧 Option 参数静默吞掉（曾导致模板类型永远存成 log）
  addTemplate: (
    name: string,
    tag: string | null,
    icon: string | null,
    itemType: ItemType,
    defaults: Record<string, unknown>,
    fields: unknown[],
    note: string | null,
  ) =>
    invoke<Template>("add_template", {
      name,
      tag,
      icon,
      itemType,
      defaults,
      fields,
      note,
    }),
  updateTemplate: (
    id: string,
    name: string,
    tag: string | null,
    icon: string | null,
    itemType: ItemType,
    defaults: Record<string, unknown>,
    fields: unknown[],
    note: string | null,
  ) =>
    invoke<Template>("update_template", {
      id,
      name,
      tag,
      icon,
      itemType,
      defaults,
      fields,
      note,
    }),
  deleteTemplate: (id: string) => invoke<Template>("delete_template", { id }),
  /** 上移 / 下移模板（sort 与相邻交换） */
  moveTemplate: (id: string, up: boolean) => invoke<void>("move_template", { id, up }),
  /** 启用 / 禁用模板（禁用 = pinned 0，不删行不删字段） */
  setTemplatePinned: (id: string, pinned: boolean) =>
    invoke<void>("set_template_pinned", { id, pinned }),
  /** 字段定义：全局 + scope 类型 */
  listFieldDefs: (scope?: ItemType | null) =>
    invoke<FieldDef[]>("list_field_defs", { scope: scope ?? null }),
  /** scope 缺省 = 全部字段 */
  listAllFieldDefs: () => invoke<FieldDef[]>("list_field_defs", { scope: null }),
  addFieldDef: (
    name: string,
    kind: FieldKind,
    options: Record<string, unknown>,
    scope?: ItemType | null,
  ) =>
    invoke<FieldDef>("add_field_def", {
      name,
      kind,
      options,
      scope: scope ?? null,
    }),
  updateFieldDef: (
    id: string,
    patch: { name?: string; options?: Record<string, unknown>; sort?: number },
  ) => invoke<FieldDef>("update_field_def", { id, ...patch }),
  /** 软删：同名可重建（重建即复活并恢复历史值）；内置字段 = 禁用 */
  deleteFieldDef: (id: string) => invoke<FieldDef>("delete_field_def", { id }),
  /** 删除字段前的确认提示：多少条目带着这个字段的值（按字段 id） */
  countItemsWithField: (id: string) => invoke<number>("count_items_with_field", { id }),
  /** 已软删的字段定义（展示层：旧条目带出历史值的字段名） */
  listDeletedFieldDefs: () => invoke<FieldDef[]>("list_deleted_field_defs"),
  /** 彻底清理全部软删字段（不可逆，重建同名不再复活），返回清理个数 */
  purgeDeletedFieldDefs: () => invoke<number>("purge_deleted_field_defs"),
  addAttachmentB64: (itemId: string, dataB64: string, ext: string) =>
    invoke<Attachment>("add_attachment_b64", { itemId, dataB64, ext }),
  attachmentUrl: async (relPath: string) => {
    const abs = await invoke<string>("attachment_abs_path", { relPath });
    return convertFileSrc(abs);
  },
  /** 文件链接：用系统默认程序打开 */
  openFilePath: (path: string) => invoke<void>("open_file_path", { path }),
  /** 文件链接：在文件管理器中定位（失败回退打开所在目录） */
  revealFilePath: (path: string) => invoke<void>("reveal_file_path", { path }),
  getSetting: (key: string) => invoke<string | null>("get_setting", { key }),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  /** 开机自启（Linux XDG autostart / Windows Run 键） */
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enable: boolean) => invoke<void>("set_autostart", { enable }),
  /** 切换界面语言：落库 + 托盘/副窗口标题即时跟随 */
  setUiLang: (lang: "zh" | "en") => invoke<void>("set_ui_lang", { lang }),
  /** 打开日志目录（设置 → 数据与 IPC） */
  openLogDir: () => invoke<void>("open_log_dir"),
  /** 冲突检测（提示但不阻止）：与 [start,end] 相交的其他日程 */
  checkConflict: (start: string, end: string, excludeId?: string | null) =>
    invoke<Item[]>("check_conflict", { start, end, excludeId: excludeId ?? null }),
  // ---- 视图模型（FILTER-SPEC §12）----
  viewList: (panel?: Panel | null) => invoke<ViewDef[]>("view_list", { panel: panel ?? null }),
  viewGet: (id: string) => invoke<ViewDef>("view_get", { id }),
  viewCreate: (name: string, panel: Panel, config: unknown) =>
    invoke<ViewDef>("view_create", { name, panel, config }),
  /** 内置 id = 写 config_user（定制）；用户 id = 覆盖 config */
  viewSave: (id: string, config: unknown, name?: string | null) =>
    invoke<ViewDef>("view_save", { id, config, name: name ?? null }),
  viewDelete: (id: string) => invoke<ViewDef>("view_delete", { id }),
  /** 重置内置视图 = 清空 config_user，恢复 seed */
  viewReset: (id: string) => invoke<ViewDef>("view_reset", { id }),
  /** 另存为：从任意视图复制生效配置出一张用户视图 */
  viewDuplicate: (id: string, name: string) => invoke<ViewDef>("view_duplicate", { id, name }),
  /** 视图求值：与 CLI `item query --view` 同 id 同结果 */
  queryView: (id: string, keyword?: string | null, limit?: number | null) =>
    invoke<ViewResult>("query_view", { id, keyword: keyword ?? null, limit: limit ?? null }),
  /** 统计页一次求值：全部 stats 容器展开为挂件流（窗口由各挂件配置决定） */
  queryStatsPage: () => invoke<StatsPageResult>("query_stats_page"),
  /** 恢复默认统计页：完全重置（清掉全部统计容器铺回预设），返回容器名 */
  statsRestoreDefaults: () => invoke<string[]>("stats_restore_defaults"),
  /** 统计汇总：热力图 + 模板连续 + 数值趋势；days = 窗口天数（缺省 365） */
  statsSummary: (days?: number) =>
    invoke<{
      heatmap: { day: string; count: number }[];
      streaks: { template_id: string; name: string; icon: string | null; current: number; longest: number; recent: number }[];
      series: { field_id: string; name: string; unit: string | null; points: [string, number][] }[];
    }>("stats_summary", { days: days ?? null }),
  /** 提醒中心（SPRINT2-SPEC §5） */
  reminderHistory: (limit?: number) =>
    invoke<ReminderHistoryEntry[]>("reminder_history", { limit: limit ?? null }),
  reminderUnread: () => invoke<number>("reminder_unread"),
  markRemindersSeen: () => invoke<string>("mark_reminders_seen"),
  /** 类型转换（SPRINT2-SPEC §7）：待办转日程（替换）/ 日程生成记录（保留原日程） */
  convertTaskToEvent: (id: string) => invoke<Item>("convert_task_to_event", { id }),
  eventToLog: (id: string) => invoke<Item>("event_to_log", { id }),
  /** 导出 ICS 到数据目录 exports/，返回路径 */
  exportIcs: () => invoke<string>("export_ics"),
  /** 一键备份 zip（保留最近 7 份），返回路径 */
  backupZip: () => invoke<string>("backup_zip"),
  /** 节假日 JSON（SPRINT2-SPEC §4）：用户自定义文件存数据目录，优先于内置 */
  loadHolidaysJson: () => invoke<string | null>("load_holidays_json"),
  saveHolidaysJson: (text: string) => invoke<void>("save_holidays_json", { text }),
  resetHolidaysJson: () => invoke<void>("reset_holidays_json"),
  openQuickAdd: (itemType?: ItemType | null, title?: string | null) =>
    invoke<void>("open_quick_add", { itemType: itemType ?? null, title: title ?? null }),
  // ---- 今日悬浮窗（OVERLAY-SPEC）----
  overlayToday: () => invoke<OverlayToday>("overlay_today"),
  getOverlayConfig: () => invoke<OverlayConfig>("get_overlay_config"),
  /** 整体写回；锁定 / 角落 / 位置变更由 Rust 侧直接落到真实窗口 */
  setOverlayConfig: (config: OverlayConfig) => invoke<void>("set_overlay_config", { config }),
  /** 重读配置并落到真实窗口（锁定穿透 / 吸附位置） */
  overlayApplyWindowState: () => invoke<void>("overlay_apply_window_state"),
  overlaySetVisible: (visible: boolean) => invoke<void>("overlay_set_visible", { visible }),
  /** 拖动结束后持久化当前位置（Rust 侧读窗口坐标，原生 Wayland 下为空操作） */
  overlaySaveDragPos: () => invoke<void>("overlay_save_drag_pos"),
  /** 拖边调整尺寸后持久化当前大小（Rust 侧读窗口尺寸并钳制上下限，v1.1） */
  overlaySaveResizeSize: () => invoke<void>("overlay_save_resize_size"),
  /** 悬浮窗点击条目 / 摘要行：前置主窗口 */
  overlayShowMain: () => invoke<void>("overlay_show_main"),
  appInfo: () =>
    invoke<{ version: string; data_root: string; socket_path: string; backend?: string }>("app_info"),
};

// ----------------------------------------------------------------------
// 展示工具
// ----------------------------------------------------------------------

/** 展示标题：title ?? note 截断(40 字) ?? 无标题兜底（§5.7） */
export function displayTitle(item: Pick<Item, "title" | "note">): string {
  const title = item.title?.trim();
  if (title) return title;
  const n = item.note?.trim();
  if (n) return n.length > 40 ? n.slice(0, 40) : n;
  return t("common.untitled");
}

/** 本地时区的 `YYYY-MM-DDTHH:mm`，用于 datetime-local 输入框。 */
export function toLocalInput(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `YYYY-MM-DD` */
export function toDateInput(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** RFC3339（UTC），core 端按 UTC RFC3339 字符串存库。 */
export function fromLocalInput(v: string): string {
  return new Date(v).toISOString();
}

/** 日程默认开始 = 下一个整点。 */
export function defaultEventStart(): string {
  const d = new Date();
  d.setMinutes(0, 0, 0);
  d.setHours(d.getHours() + 1);
  return toLocalInput(d.toISOString());
}

export function nowLocalInput(): string {
  return toLocalInput(new Date().toISOString());
}

export function fmtTime(iso: string | null): string {
  if (!iso) return "";
  return new Date(iso).toLocaleTimeString(i18n.locale === "en" ? "en-US" : "zh-CN", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function fmtDate(iso: string | null): string {
  if (!iso) return "";
  return new Date(iso).toLocaleDateString(i18n.locale === "en" ? "en-US" : "zh-CN", {
    month: "numeric",
    day: "numeric",
  });
}

export function fmtDateTime(iso: string | null): string {
  if (!iso) return "";
  return `${fmtDate(iso)} ${fmtTime(iso)}`;
}

/** 类型显示名（随界面语言）。 */
export function typeLabel(type: ItemType): string {
  return t(`type.${type}` as MessageKey);
}

/** 条目主时间锚点（列表/提醒展示用）：日程看开始、待办看截止、记录看发生时间 */
export function anchorTime(item: Item): string | null {
  if (item.type === "log") return item.occurred_at;
  if (item.type === "task") return item.due_at ?? item.start_at;
  return item.start_at;
}

// ----------------------------------------------------------------------
// 文件链接（extra[FILE_LINKS_KEY]，字符串数组；只记路径不复制文件）
// ----------------------------------------------------------------------

export function fileLinksFromExtra(extra: Record<string, unknown> | undefined | null): string[] {
  const v = extra?.[FILE_LINKS_KEY];
  return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
}

export function fileLinksOf(item: Item): string[] {
  return fileLinksFromExtra(item.extra);
}

export function basename(p: string): string {
  return p.split("/").filter(Boolean).pop() ?? p;
}

/** 快速添加幂等键：quickadd-{YYYYMMDDHHmm}-{hash}（§5.8） */
export function quickAddIdempotencyKey(seed: Record<string, unknown>): string {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}${pad(now.getHours())}${pad(now.getMinutes())}`;
  const json = JSON.stringify(seed);
  let h = 0;
  for (let i = 0; i < json.length; i++) {
    h = (Math.imul(31, h) + json.charCodeAt(i)) | 0;
  }
  return `quickadd-${stamp}-${Math.abs(h).toString(36)}`;
}

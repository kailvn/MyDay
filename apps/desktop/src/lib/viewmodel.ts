/**
 * 视图模型共享层（FILTER-SPEC v1 / §11「一套交互处处相同」）：
 * 字段引用元数据、运算符矩阵、值控件形态、条件 chips 文案、
 * AST ↔ 编辑器行状态互转。记录 / 待办 / 搜索 / 统计四个面板与
 * 筛选 / 排序 / 挂件编辑器全部从本模块取词表，不各写一套。
 */

import type {
  Condition,
  Cmp,
  DateValue,
  FieldDef,
  FilterNode,
  FilterValue,
  ItemType,
  SortSpec,
  ViewDef,
  WidgetConfig,
} from "./api";
import { t } from "./i18n";

/** 字段 UI 角色（决定运算符集与值控件） */
export type FieldRole =
  | "text" | "number" | "select" | "multi" | "bool" | "date"
  | "type" | "status" | "template" | "recurrence";

/**
 * 内置列（§4.1；v1 不暴露 col:id / idempotency_key）。
 * label 存 i18n key（结构不变），渲染处过 t() 出显示名。
 */
export const BUILTIN_COLUMNS: { id: string; label: string; role: FieldRole }[] = [
  { id: "col:type", label: "vm.col.type", role: "type" },
  { id: "col:status", label: "vm.col.status", role: "status" },
  { id: "col:title", label: "vm.col.title", role: "text" },
  { id: "col:note", label: "vm.col.note", role: "text" },
  { id: "col:tags", label: "vm.col.tags", role: "multi" },
  { id: "col:template_id", label: "vm.col.template", role: "template" },
  { id: "col:recurrence", label: "vm.col.recurrence", role: "recurrence" },
  { id: "col:anchor", label: "vm.col.anchor", role: "date" },
  { id: "col:start_at", label: "vm.col.startAt", role: "date" },
  { id: "col:end_at", label: "vm.col.endAt", role: "date" },
  { id: "col:due_at", label: "vm.col.dueAt", role: "date" },
  { id: "col:occurred_at", label: "vm.col.occurredAt", role: "date" },
  { id: "col:created_at", label: "vm.col.createdAt", role: "date" },
  { id: "col:updated_at", label: "vm.col.updatedAt", role: "date" },
  { id: "col:completed_at", label: "vm.col.completedAt", role: "date" },
];

export const builtinColumn = (id: string) => BUILTIN_COLUMNS.find((c) => c.id === id);

/** 自定义字段 kind → 角色 */
export function fieldRole(def: FieldDef): FieldRole {
  switch (def.kind) {
    case "number": return "number";
    case "select": return "select";
    case "multiselect": return "multi";
    case "bool": return "bool";
    case "date": return "date";
    default: return "text"; // text / url
  }
}

export function roleOf(field: string, fields: FieldDef[]): FieldRole | null {
  const col = builtinColumn(field);
  if (col) return col.role;
  const def = fields.find((f) => f.id === field);
  return def ? fieldRole(def) : null;
}

export function fieldLabel(field: string, fields: FieldDef[]): string {
  const col = builtinColumn(field);
  if (col) return t(col.label);
  return fields.find((f) => f.id === field)?.name ?? t("vm.field.deleted", { name: field });
}

/** 字段对给定条目类型是否可选（列全类型可选；自定义字段按 scope） */
export function fieldApplicable(field: string, itemType: "all" | ItemType, fields: FieldDef[]): boolean {
  const def = fields.find((f) => f.id === field);
  if (!def) return false;
  if (itemType === "all") return true;
  return def.scope == null || def.scope === itemType;
}

/** 运算符矩阵（§4.3）：每个角色可用的运算符 + 文案 key（渲染处过 t()） */
export const CMP_MATRIX: Record<FieldRole, { id: Cmp; label: string }[]> = {
  text: [
    { id: "eq", label: "vm.cmp.eq" },
    { id: "neq", label: "vm.cmp.neq" },
    { id: "contains", label: "vm.cmp.contains" },
    { id: "not_contains", label: "vm.cmp.notContains" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  number: [
    { id: "eq", label: "=" },
    { id: "neq", label: "≠" },
    { id: "gt", label: ">" },
    { id: "gte", label: "≥" },
    { id: "lt", label: "<" },
    { id: "lte", label: "≤" },
    { id: "between", label: "vm.cmp.between" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  select: [
    { id: "eq", label: "vm.cmp.eq" },
    { id: "neq", label: "vm.cmp.neq" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  multi: [
    { id: "any", label: "vm.cmp.any" },
    { id: "all", label: "vm.cmp.all" },
    { id: "none", label: "vm.cmp.none" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  bool: [
    { id: "is_true", label: "vm.cmp.isTrue" },
    { id: "is_false", label: "vm.cmp.isFalse" },
    { id: "empty", label: "vm.cmp.unset" },
  ],
  date: [
    { id: "on", label: "vm.cmp.on" },
    { id: "before", label: "vm.cmp.before" },
    { id: "after", label: "vm.cmp.after" },
    { id: "between", label: "vm.cmp.between" },
    { id: "within", label: "vm.cmp.within" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  type: [{ id: "eq", label: "vm.cmp.eq" }],
  status: [{ id: "eq", label: "vm.cmp.eq" }],
  template: [
    { id: "eq", label: "vm.cmp.eq" },
    { id: "neq", label: "vm.cmp.neq" },
    { id: "empty", label: "vm.cmp.empty" },
    { id: "not_empty", label: "vm.cmp.notEmpty" },
  ],
  recurrence: [
    { id: "empty", label: "vm.cmp.recurNone" },
    { id: "not_empty", label: "vm.cmp.recurSome" },
  ],
};

export const NO_VALUE_CMPS: Cmp[] = ["empty", "not_empty", "is_true", "is_false"];

/** 值控件形态（配对封闭 §4.2：控件随运算符切换，构造不出非法组合） */
export type ValueControl =
  | null
  | "text" | "number" | "numpair" | "select" | "multiselect"
  | "date" | "datepair" | "daterange" | "status" | "type" | "template";

export function valueControlOf(role: FieldRole, cmp: Cmp): ValueControl {
  if (NO_VALUE_CMPS.includes(cmp)) return null;
  switch (cmp) {
    case "between":
      return role === "number" ? "numpair" : "datepair";
    case "within":
      return "daterange";
    case "on": case "before": case "after":
      return "date";
    case "any": case "all": case "none":
      return role === "template" ? "multiselect" : "multiselect";
    case "gt": case "gte": case "lt": case "lte":
      return "number";
    case "eq": case "neq":
      return role === "number" ? "number"
        : role === "select" ? "select"
        : role === "status" ? "status"
        : role === "type" ? "type"
        : role === "template" ? "template"
        : "text";
    default:
      return "text";
  }
}

/** 相对日期 token（点值）菜单（label 为 key，渲染处过 t()） */
export const REL_DAY_LABELS: { id: DateValue; label: string }[] = [
  { id: { rel: "today" }, label: "vm.rel.today" },
  { id: { rel: "tomorrow" }, label: "vm.rel.tomorrow" },
  { id: { rel: "yesterday" }, label: "vm.rel.yesterday" },
];

/** 范围值预设 chips（UI 的「本周 / 本月 / 近 N 天」） */
export const REL_RANGE_LABELS: { id: DateValue; label: string }[] = [
  { id: { rel: "this_week" }, label: "vm.rel.thisWeek" },
  { id: { rel: "this_month" }, label: "vm.rel.thisMonth" },
  { id: { rel: "last_days:7" }, label: "vm.rel.last7" },
  { id: { rel: "last_days:30" }, label: "vm.rel.last30" },
];

// ----------------------------------------------------------------------
// 编辑器行状态（两层封顶：根组 + 可选子组；子组内只有条件行）
// ----------------------------------------------------------------------

export interface RuleRow {
  field: string;
  cmp: Cmp;
  value: FilterValue | null;
}

export interface RuleGroup {
  op: "and" | "or";
  rows: RuleRow[];
}

export interface FilterEditState {
  op: "and" | "or";
  rows: RuleRow[];
  /** 子组（第二层；null = 无） */
  sub: RuleGroup | null;
}

export const emptyEditState = (): FilterEditState => ({ op: "and", rows: [], sub: null });

/** AST → 编辑器状态（根必须是组；子组拍平为一层） */
export function astToEditState(node: FilterNode | undefined | null): FilterEditState {
  const st = emptyEditState();
  if (!node || !("op" in node)) return st;
  st.op = node.op;
  for (const child of node.children) {
    if ("op" in child) {
      // 子组：拍平其条件（子组内只能含条件，校验保证）
      st.sub = {
        op: child.op,
        rows: child.children
          .filter((c): c is Condition => !("op" in c))
          .map((c) => ({ field: c.field, cmp: c.cmp, value: c.value ?? null })),
      };
    } else {
      st.rows.push({ field: child.field, cmp: child.cmp, value: child.value ?? null });
    }
  }
  return st;
}

/** 编辑器状态 → AST（空 rows = 恒真） */
export function editStateToAst(st: FilterEditState): FilterNode {
  const children: FilterNode[] = st.rows.map((r) => ({
    field: r.field,
    cmp: r.cmp,
    ...(r.value != null ? { value: r.value } : {}),
  }));
  if (st.sub && st.sub.rows.length) {
    children.push({
      op: st.sub.op,
      children: st.sub.rows.map((r) => ({
        field: r.field,
        cmp: r.cmp,
        ...(r.value != null ? { value: r.value } : {}),
      })),
    });
  }
  return { op: st.op, children };
}

/** 换字段 / 换运算符时把值收敛为该组合的合法形态（宁空勿非法） */
export function defaultValueFor(field: string, cmp: Cmp, fields: FieldDef[]): FilterValue | null {
  const role = roleOf(field, fields);
  if (!role) return null;
  const control = valueControlOf(role, cmp);
  switch (control) {
    case null: return null;
    case "text": return "";
    case "number": return 0;
    case "numpair": return { from: 0, to: 0 };
    case "select": {
      const def = fields.find((f) => f.id === field);
      return def?.options?.choices?.[0] ?? "";
    }
    case "multiselect": return [];
    case "date": return { rel: "today" };
    case "datepair": return { from: { rel: "today" }, to: { rel: "tomorrow" } };
    case "daterange": return { rel: "this_week" };
    case "status": return "todo";
    case "type": return "task";
    case "template": return "";
  }
}

/** 条件 chips 用的运算符短文案（key 表） */
const CMP_SHORT: Record<Cmp, string> = {
  eq: "=", neq: "≠", contains: "vm.cmp.contains", not_contains: "vm.cmp.notContains",
  gt: ">", gte: "≥", lt: "<", lte: "≤", between: "vm.cmp.between",
  any: "vm.cmp.any", all: "vm.cmp.all", none: "vm.cmp.none",
  is_true: "vm.cmp.isTrue", is_false: "vm.cmp.isFalse",
  on: "vm.cmp.on", before: "vm.cmp.before", after: "vm.cmp.after", within: "vm.cmp.withinShort",
  empty: "vm.cmp.empty", not_empty: "vm.cmp.notEmpty",
};

export function describeDateValue(v: DateValue): string {
  if ("day" in v) return v.day;
  switch (v.rel) {
    case "today": return t("vm.rel.today");
    case "tomorrow": return t("vm.rel.tomorrow");
    case "yesterday": return t("vm.rel.yesterday");
    case "this_week": return t("vm.rel.thisWeek");
    case "this_month": return t("vm.rel.thisMonth");
    default: return t("vm.rel.lastDays", { n: String(v.rel).slice("last_days:".length) });
  }
}

function describeValue(v: FilterValue | null | undefined, fields: FieldDef[], templates: { id: string; name: string }[]): string {
  if (v == null) return "";
  if (typeof v === "boolean") return v ? t("vm.bool.yes") : t("vm.bool.no");
  if (typeof v === "number") return String(v);
  if (typeof v === "string") {
    // 模板 / 状态 / 类型的文本值按词表翻译
    const tpl = templates.find((tp) => tp.id === v);
    return tpl ? tpl.name : v;
  }
  if (Array.isArray(v)) return v.join(" / ");
  if ("from" in v && "to" in v) {
    const fv = (v as { from: unknown; to: unknown }).from;
    const tv = (v as { from: unknown; to: unknown }).to;
    const fmt = (x: unknown) =>
      typeof x === "object" && x != null ? describeDateValue(x as DateValue) : String(x);
    return `${fmt(fv)} ~ ${fmt(tv)}`;
  }
  return describeDateValue(v as DateValue);
}

/** 条件 chips 文案（工具条逐个 × 展示） */
export function describeCondition(
  c: Condition,
  fields: FieldDef[],
  templates: { id: string; name: string }[],
): string {
  const label = fieldLabel(c.field, fields);
  const op = t(CMP_SHORT[c.cmp]);
  if (c.value == null) return `${label} ${op}`;
  const role = roleOf(c.field, fields);
  let v = describeValue(c.value, fields, templates);
  if (role === "status") {
    v = c.value === "todo" ? t("vm.status.todo") : t("vm.status.done");
  } else if (role === "type") {
    v = { event: t("type.event"), task: t("type.task"), log: t("type.log") }[c.value as ItemType] ?? v;
  }
  return `${label} ${op} ${v}`;
}

/** 排序 chips 文案 */
export function describeSort(s: SortSpec, fields: FieldDef[]): string {
  return `${fieldLabel(s.field, fields)} ${s.dir === "asc" ? "↑" : "↓"}`;
}

// ----------------------------------------------------------------------
// 挂件（stats 挂件编辑器共用）
// ----------------------------------------------------------------------

export const RENDER_LABELS: { id: WidgetConfig["render"]; label: string }[] = [
  { id: "card", label: "vm.render.card" },
  { id: "bar", label: "vm.render.bar" },
  { id: "line", label: "vm.render.line" },
  { id: "pie", label: "vm.render.pie" },
  { id: "heatmap", label: "vm.render.heatmap" },
];

export const METRIC_FNS: { id: NonNullable<NonNullable<WidgetConfig["agg"]>["metric"]>["fn"]; label: string }[] = [
  { id: "count", label: "vm.metric.count" },
  { id: "sum", label: "vm.metric.sum" },
  { id: "avg", label: "vm.metric.avg" },
  { id: "min", label: "vm.metric.min" },
  { id: "max", label: "vm.metric.max" },
  { id: "values", label: "vm.metric.values" },
];

/** 打卡卡（card + streak）等新挂件的种子配置（「＋ 挂件」30 秒路径） */
export function newWidgetConfig(source: Partial<WidgetConfig> = {}): WidgetConfig {
  return {
    kind: "widget",
    dataset: {
      item_type: "log",
      filter: { op: "and", children: [] },
      ...(source.dataset ? JSON.parse(JSON.stringify(source.dataset)) : {}),
    },
    window: { days: 365 },
    agg: {
      group: { by: "time", bucket: "day", time_field: "col:occurred_at" },
      metric: { fn: "count" },
    },
    derived: { kind: "streak", goal: { daily: 1 } },
    render: "card",
    options: { presence: false, top_n: 8 },
    ...source,
    dataset: source.dataset ?? {
      item_type: "log",
      filter: { op: "and", children: [] },
    },
  };
}

/** 深拷贝配置（编辑前的工作副本） */
export function cloneConfig<T>(v: T): T {
  return JSON.parse(JSON.stringify(v));
}

/** 生效配置（config_user ?? config） */
export function effectiveConfig(view: ViewDef): Record<string, unknown> {
  return (view.config_user ?? view.config) as Record<string, unknown>;
}



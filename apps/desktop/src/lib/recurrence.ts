/**
 * 重复规则的 TS 镜像（core/src/recurrence.rs，SPRINT-SPEC §2）。
 *
 * 文法：@daily / @weekly:<1-7>（1=周一）/ @monthly:<1-31>（当月不足取月末）。
 * 展开语义与 Rust 侧一致：occurrence 保持 anchor 的本地钟点；
 * 日程按区间相交（跨天两头都算）、待办按 due 落窗；单窗口上限 400。
 * 「修改全部」：条目只存一条规则，编辑锚点 = 平移整个系列（无实例物化）。
 * golden 自检：recurrence.golden.ts（node --experimental-strip-types 运行）。
 */
import type { Item } from "./api";
import type { MessageKey } from "./i18n";
// 带显式 .ts 扩展名：golden 在 node ESM 下直跑（无扩展名解析），与 *.golden.ts 一致
import { part_misc } from "./i18n/parts/misc.ts";

// 本模块是纯模块（golden 用 node 直跑，无 runes / 无 vite 别名解析），
// 不能静态依赖 runes 版 i18n。先挂一个中文基准回退取词（值来自词典分片，
// 与界面 zh 完全一致）；浏览器内再用 top-level await 换成真正的响应式 t()，
// 该 await 先于任何组件渲染完成，无首帧语言闪烁。
type Translate = (key: MessageKey, params?: Record<string, string | number>) => string;

function fallbackT(key: MessageKey, params?: Record<string, string | number>): string {
  const tpl = part_misc[key as keyof typeof part_misc] ?? key;
  if (!params) return tpl;
  return tpl.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in params ? String(params[name]) : m,
  );
}

let t: Translate = fallbackT;
try {
  ({ t } = await import("./i18n"));
} catch {
  // node golden 环境：保持上方中文基准回退
}

/** 单次发生：该类型用到的字段有值（Date），其余 null */
export interface Occurrence {
  start: Date | null;
  end: Date | null;
  due: Date | null;
}

export type ParsedRecurrence =
  | { kind: "daily" }
  | { kind: "weekly"; n: number }
  | { kind: "monthly"; d: number };

/** 严格解析（宁报错不猜测）：非法返回 null */
export function parseRecurrence(spec: string | null | undefined): ParsedRecurrence | null {
  if (!spec) return null;
  if (spec === "@daily") return { kind: "daily" };
  const week = spec.match(/^@weekly:([1-7])$/);
  if (week) return { kind: "weekly", n: Number(week[1]) };
  const month = spec.match(/^@monthly:([1-9]|[12]\d|3[01])$/);
  if (month) return { kind: "monthly", d: Number(month[1]) };
  return null;
}

/** 规范化字符串（round-trip） */
export function recurrenceToString(p: ParsedRecurrence): string {
  if (p.kind === "daily") return "@daily";
  if (p.kind === "weekly") return `@weekly:${p.n}`;
  return `@monthly:${p.d}`;
}

/** 周几显示名（1=周一 … 7=周日，与 @weekly:n 一致；随界面语言） */
export function weekdayLabel(n: number): string {
  return t(`recurrence.wd${n}` as MessageKey);
}

/** 列表 / 详情徽标文案：每天 / 每周三 / 每月15日 */
export function recurrenceLabel(spec: string | null | undefined): string {
  const p = parseRecurrence(spec);
  if (!p) return "";
  if (p.kind === "daily") return t("recurrence.daily");
  if (p.kind === "weekly") return t("recurrence.weekly", { day: weekdayLabel(p.n) });
  return t("recurrence.monthly", { d: p.d });
}

const DAY_MS = 86_400_000;

function daysInMonth(y: number, m: number): number {
  // m: 1-12
  return new Date(y, m, 0).getDate();
}

/** 严格晚于 after 的下一次发生（保持 anchor 的本地钟点）；异常返回 null */
export function nextAfter(anchor: Date, after: Date, rec: ParsedRecurrence): Date | null {
  const h = anchor.getHours();
  const mi = anchor.getMinutes();
  const se = anchor.getSeconds();
  const base = new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate());
  const at = (y: number, m: number, d: number): Date | null => {
    const t = new Date(y, m - 1, d, h, mi, se);
    // DST 挪走/跳过的钟点视为不存在（该期顺延）
    if (t.getHours() !== h || t.getDate() !== d) return null;
    return t;
  };
  if (rec.kind === "daily") {
    let d = new Date(base);
    for (let guard = 0; guard < 500; guard++) {
      const t = at(d.getFullYear(), d.getMonth() + 1, d.getDate());
      if (t && t > after) return t;
      d = new Date(d.getTime() + DAY_MS);
    }
    return null;
  }
  if (rec.kind === "weekly") {
    // 1=周一 … 7=周日；JS getDay(): 0=周日
    const target = rec.n === 7 ? 0 : rec.n;
    let d = new Date(base);
    d.setDate(d.getDate() + ((target - d.getDay() + 7) % 7));
    for (let guard = 0; guard < 520; guard++) {
      const t = at(d.getFullYear(), d.getMonth() + 1, d.getDate());
      if (t && t > after) return t;
      d = new Date(d.getTime() + 7 * DAY_MS);
    }
    return null;
  }
  // monthly：月末钳制（31 → 2 月 28/29）
  let total = base.getFullYear() * 12 + base.getMonth(); // 0-based month index
  for (let guard = 0; guard < 1200; guard++) {
    const y = Math.floor(total / 12);
    const m0 = total % 12;
    const d = Math.min(rec.d, daysInMonth(y, m0 + 1));
    const t = at(y, m0 + 1, d);
    if (t && t > after) return t;
    total += 1;
  }
  return null;
}

/** 展开条目在窗口内的发生（镜像 Rust occurrences_between）；无规则 / 非法 → 空 */
export function occurrencesBetween(item: Item, from: Date, to: Date): Occurrence[] {
  const rec = parseRecurrence(item.recurrence);
  if (!rec) return [];
  const out: Occurrence[] = [];
  const MAX = 400;
  const date = (iso: string | null) => (iso ? new Date(iso) : null);
  if (item.type === "event") {
    const anchor = date(item.start_at);
    if (!anchor) return out;
    const end = date(item.end_at) ?? anchor;
    const dur = end.getTime() - anchor.getTime();
    let t: Date | null = anchor;
    while (t && t.getTime() <= to.getTime()) {
      const e = new Date(t.getTime() + dur);
      if (e.getTime() >= from.getTime()) {
        out.push({ start: t, end: e, due: null });
        if (out.length >= MAX) break;
      }
      t = nextAfter(anchor, t, rec);
    }
  } else if (item.type === "task") {
    const due = date(item.due_at);
    if (!due) return out;
    const shift = date(item.start_at);
    const diff = shift ? due.getTime() - shift.getTime() : null;
    let t: Date | null = due;
    while (t && t.getTime() <= to.getTime()) {
      if (t.getTime() >= from.getTime()) {
        out.push({
          start: diff != null ? new Date(t.getTime() - diff) : null,
          end: null,
          due: t,
        });
        if (out.length >= MAX) break;
      }
      t = nextAfter(due, t, rec);
    }
  }
  return out;
}

/**
 * 视图数据展开：不带规则的条目原样返回；带规则的条目替换为窗口内的
 * 虚拟实例（同 id，仅 start_at / end_at / due_at 换成本次发生的时刻）——
 * 点击打开的仍是原条目（编辑即改系列），展示层零改动。
 */
export function expandItems(items: Item[], from: Date, to: Date): Item[] {
  const out: Item[] = [];
  for (const it of items) {
    if (!parseRecurrence(it.recurrence)) {
      out.push(it);
      continue;
    }
    for (const occ of occurrencesBetween(it, from, to)) {
      out.push({
        ...it,
        start_at: occ.start ? occ.start.toISOString() : it.start_at,
        end_at: occ.end ? occ.end.toISOString() : it.end_at,
        due_at: occ.due ? occ.due.toISOString() : it.due_at,
      });
    }
  }
  return out;
}

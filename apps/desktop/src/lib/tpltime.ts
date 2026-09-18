/**
 * 模板时间占位符（@token）TS 镜像：与 core `tpltime.rs` 同一文法的解析器。
 * 面板在「激活字段瞬间」把模板 token 解析成可编辑的具体时间，无需 IPC；
 * core 仍是权威解析方（一键记录 / CLI 走 Rust 侧），两侧用同一组 golden
 * 用例锁定行为（见 tpltime.golden.ts，`node --experimental-strip-types` 可跑）。
 *
 * 文法：`@<base>[<offset>][T<time>]`
 *   base   = now | next_hour | start | d0 | d+N | d-N
 *   offset = +数字(m|h) / -数字(m|h)
 *   time   = HH:MM | end（=23:59）
 * 两族语义：clock 族（now/next_hour）相对当前时刻；day 族（d*）相对锚点日
 * （日历选中日，缺省今天）的本地时区时刻；start 基准仅 end_at 可用。
 */
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

export interface ResolveCtx {
  now: Date | string;
  /** 本地锚点日 YYYY-MM-DD（缺省今天） */
  anchorDay?: string | null;
  /** 已解析的开始时间（@start 基准） */
  start?: Date | string | null;
}

type Parsed =
  | { kind: "now"; offMin: number }
  | { kind: "nextHour" }
  | { kind: "start"; offMin: number }
  | { kind: "day"; off: number; clock: [number, number] | null };

/** `@` 开头即视为 token（区别于绝对 RFC3339 / 日期字符串） */
export function isToken(v: string): boolean {
  return v.startsWith("@");
}

/** 是否为「现在 / 现在过去方向」的 now 族 token（`@now` / `@now-30m` / `@now-1h`）。
 *  记录的 occurred_at 只允许这类占位——未来的「记录」语义不成立。
 *  注意：与 Rust 侧一致，只认 m/h；`@now-1d` 属于合法文法但不属于过去方向池。 */
export function isPastNowToken(tok: string): boolean {
  if (tok === "@now") return true;
  if (!tok.startsWith("@now-")) return false;
  const rest = tok.slice("@now-".length);
  const unit = rest.slice(-1);
  const digits = rest.slice(0, -1);
  return (
    (unit === "m" || unit === "h") && digits !== "" && /^\d+$/.test(digits)
  );
}

function findOffsetStart(s: string): number {
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if ((c === "+" || c === "-") && /\d/.test(s[i + 1] ?? "")) return i;
  }
  return s.length;
}

function err(msg: string): string {
  return msg;
}

function parseTok(tok: string, column: string): Parsed | string {
  if (!tok.startsWith("@")) return err("必须以 @ 开头");
  const body = tok.slice(1);
  if (body === "") return err("占位为空");

  if (body.startsWith("d")) {
    const rest = body.slice(1);
    const m = rest.match(/^([+-]?\d+)(?:T(\d{1,2}:\d{2}|end))?$/);
    if (!m) {
      // 细化报错：T 后格式错 vs 天数错
      const tIdx = rest.indexOf("T");
      if (tIdx >= 0) {
        const t = rest.slice(tIdx + 1);
        if (t !== "end" && !/^\d{1,2}:\d{2}$/.test(t)) {
          return err(`时刻格式应为 HH:MM，得到 "${t}"`);
        }
      }
      return err("d 基准天数无效（如 d0 / d+1）");
    }
    const off = parseInt(m[1], 10);
    if (Number.isNaN(off)) return err("d 基准天数无效（如 d0 / d+1）");
    let clock: [number, number] | null = null;
    if (m[2] && m[2] !== "end") {
      const [h, mi] = m[2].split(":").map((x) => parseInt(x, 10));
      if (h > 23 || mi > 59) return err(`时刻超出范围：${m[2]}`);
      clock = [h, mi];
    } else if (m[2] === "end") {
      clock = [23, 59];
    }
    return { kind: "day", off, clock };
  }

  const cut = findOffsetStart(body);
  const base = body.slice(0, cut);
  const offStr = body.slice(cut);
  let offMin = 0;
  if (offStr !== "") {
    const m = offStr.match(/^([+-])(\d+)(m|h|d)$/);
    if (!m) return err(`偏移无效："${offStr}"`);
    const n = parseInt(m[2], 10);
    const unit = m[3] === "h" ? 60 : m[3] === "d" ? 1440 : 1;
    offMin = (m[1] === "-" ? -n : n) * unit;
  }
  if (base === "now") return { kind: "now", offMin };
  if (base === "next_hour") {
    if (offStr !== "") return err("next_hour 不接受偏移");
    return { kind: "nextHour" };
  }
  if (base === "start") {
    if (column !== "any" && column !== "end_at") {
      return err("@start 基准仅 end_at 可用");
    }
    if (offStr === "") return err("@start 需要偏移，如 @start+30m");
    return { kind: "start", offMin };
  }
  return err(`未知基准 "${base}"（可用 now / next_hour / start / d0 / d+N）`);
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** 解析 token 为 UTC ISO 字符串；语法 / 语义错误返回 null。 */
export function resolveToken(tok: string, ctx: ResolveCtx): string | null {
  const now = ctx.now instanceof Date ? ctx.now : new Date(ctx.now);
  const p = parseTok(tok, "any");
  if (typeof p === "string") return null;
  switch (p.kind) {
    case "now":
      return new Date(now.getTime() + p.offMin * 60_000).toISOString();
    case "nextHour": {
      const d = new Date(now);
      d.setMinutes(0, 0, 0);
      d.setHours(d.getHours() + 1);
      return d.toISOString();
    }
    case "start": {
      if (!ctx.start) return null;
      const s = ctx.start instanceof Date ? ctx.start : new Date(ctx.start);
      if (Number.isNaN(s.getTime())) return null;
      return new Date(s.getTime() + p.offMin * 60_000).toISOString();
    }
    case "day": {
      const day =
        ctx.anchorDay ??
        `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
      const [y, m, d] = day.split("-").map((x) => parseInt(x, 10));
      if (!y || !m || !d) return null;
      const [h, mi] = p.clock ?? [0, 0];
      return new Date(y, m - 1, d + p.off, h, mi, 0).toISOString();
    }
  }
}

/** 模板保存前的 UI 预校验（与 core validate_for_column 同规则）；返回错误文案或 null。 */
export function validateForColumn(tok: string, column: string): string | null {
  const r = parseTok(tok, column);
  return typeof r === "string" ? `时间占位 ${tok}：${r}` : null;
}

// ----------------------------------------------------------------------
// 占位池：文法的人工筛选子集 + 人话标签。columns 限定可用列；
// occurred_at 由 isPastNowToken 额外收窄（不允许未来）。
// ----------------------------------------------------------------------

export interface PoolEntry {
  token: string;
  /** 显示标签（getter：随界面语言即时取词） */
  label: string;
  columns: string[];
}

export const TIME_POOL: PoolEntry[] = [
  { token: "@now", get label() { return t("tpltime.now"); }, columns: ["occurred_at", "start_at", "end_at", "due_at"] },
  { token: "@now-30m", get label() { return t("tpltime.now_30m"); }, columns: ["occurred_at"] },
  { token: "@now-1h", get label() { return t("tpltime.now_1h"); }, columns: ["occurred_at"] },
  { token: "@next_hour", get label() { return t("tpltime.next_hour"); }, columns: ["start_at"] },
  { token: "@d0T09:00", get label() { return t("tpltime.d0_0900"); }, columns: ["start_at", "due_at"] },
  { token: "@d0T18:00", get label() { return t("tpltime.d0_1800"); }, columns: ["start_at", "end_at", "due_at"] },
  { token: "@d0Tend", get label() { return t("tpltime.d0_end"); }, columns: ["end_at", "due_at"] },
  { token: "@d+1T09:00", get label() { return t("tpltime.d1_0900"); }, columns: ["start_at", "due_at"] },
  { token: "@d+1Tend", get label() { return t("tpltime.d1_end"); }, columns: ["end_at", "due_at"] },
  { token: "@start+30m", get label() { return t("tpltime.start_30m"); }, columns: ["end_at"] },
];

export function poolFor(column: string): PoolEntry[] {
  return TIME_POOL.filter((e) => e.columns.includes(column));
}

/** token → 人话标签（展示模板 defaults 时用）；非 token / 未知 token 原样返回 */
export function tokenLabel(v: unknown): string {
  if (typeof v !== "string" || !isToken(v)) return String(v ?? "");
  return TIME_POOL.find((e) => e.token === v)?.label ?? v;
}

/** 模板 defaults 的时间列值（@token）→ 面板可编辑的具体时间。
 *  模板存意图：非 token 值在 core 保存时即被拒绝，此处返回 null。 */
export function resolveTimeValue(
  v: unknown,
  ctx: ResolveCtx,
): string | null {
  if (typeof v !== "string" || !isToken(v)) return null;
  return resolveToken(v, ctx);
}

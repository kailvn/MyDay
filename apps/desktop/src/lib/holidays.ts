/**
 * 中国法定节假日（SPRINT2-SPEC §4）：数据与逻辑分离。
 *
 * - **数据源是 JSON**，不硬编码：内置默认随应用分发（`public/holidays.json`，
 *   vite 打包到根路径 fetch 获取）；用户可在设置页导入自定义 JSON——
 *   存到数据目录 `holidays.json`（Tauri 命令读写），加载优先级 用户 > 内置；
 * - 官方每年 11 月前后发布次年安排，拿到 JSON 导入即可，无需改代码；
 * - 覆盖不到的日期一律返回 null（优雅降级为普通日期）；
 * - 自检：holidays.golden.ts 校验内置 JSON 数据完整性。
 */

export interface HolidayRange {
  name: string;
  from: string;
  to: string;
}

export interface WorkDay {
  date: string;
  name: string;
}

/** 节假日 JSON 文档（内置与用户导入同构） */
export interface HolidaysDoc {
  version: number;
  sources: string[];
  holidays: HolidayRange[];
  workdays: WorkDay[];
}

export interface HolidayInfo {
  /** 节日名，如「国庆节」 */
  name: string;
  /** true = 放假日（休）；false = 调休上班日（班） */
  off: boolean;
  /** 多天假期中的序号（1 起）与总天数；单日 / 班日无 */
  index?: number;
  total?: number;
}

// ----------------------------------------------------------------------
// JSON 解析与校验（导入与加载共用一套规则，宁报错不猜测）
// ----------------------------------------------------------------------

const DATE_RE = /^\d{4}-\d{2}-\d{2}$/;

function validDate(s: unknown): s is string {
  if (typeof s !== "string" || !DATE_RE.test(s)) return false;
  const d = new Date(s + "T00:00:00");
  return !Number.isNaN(d.getTime()) && s === fmtDay(d);
}

function fmtDay(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

const fromKey = (k: string): Date => {
  const [y, m, d] = k.split("-").map(Number);
  return new Date(y, m - 1, d);
};

export type ParseResult =
  | { ok: true; doc: HolidaysDoc }
  | { ok: false; error: string };

/** 解析并严格校验节假日 JSON 文本。错误信息面向导入者，可直接展示。 */
export function parseHolidaysJson(text: string): ParseResult {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    return { ok: false, error: `JSON 语法错误：${e instanceof Error ? e.message : String(e)}` };
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    return { ok: false, error: "顶层必须是 JSON 对象" };
  }
  const obj = raw as Record<string, unknown>;
  const holidaysRaw = obj.holidays;
  const workdaysRaw = obj.workdays;
  if (!Array.isArray(holidaysRaw) || !Array.isArray(workdaysRaw)) {
    return { ok: false, error: '缺少 "holidays" 或 "workdays" 数组' };
  }
  const holidays: HolidayRange[] = [];
  for (const [i, r] of holidaysRaw.entries()) {
    if (typeof r !== "object" || r === null) return { ok: false, error: `holidays[${i}] 不是对象` };
    const { name, from, to } = r as Record<string, unknown>;
    if (typeof name !== "string" || !name.trim()) return { ok: false, error: `holidays[${i}].name 不能为空` };
    if (!validDate(from) || !validDate(to)) return { ok: false, error: `holidays[${i}]（${name}）日期须为合法 YYYY-MM-DD` };
    if (from > to) return { ok: false, error: `holidays[${i}]（${name}）from 晚于 to` };
    holidays.push({ name: name.trim(), from, to });
  }
  const workdays: WorkDay[] = [];
  const workSet = new Set<string>();
  for (const [i, w] of workdaysRaw.entries()) {
    if (typeof w !== "object" || w === null) return { ok: false, error: `workdays[${i}] 不是对象` };
    const { date, name } = w as Record<string, unknown>;
    if (typeof name !== "string" || !name.trim()) return { ok: false, error: `workdays[${i}].name 不能为空` };
    if (!validDate(date)) return { ok: false, error: `workdays[${i}]（${name}）日期须为合法 YYYY-MM-DD` };
    if (workSet.has(date)) return { ok: false, error: `workdays[${i}]（${name}）日期重复：${date}` };
    workSet.add(date);
    workdays.push({ date, name: name.trim() });
  }
  // 假期区间之间不得重叠
  const sorted = [...holidays].sort((a, b) => a.from.localeCompare(b.from));
  for (let i = 1; i < sorted.length; i++) {
    if (sorted[i].from <= sorted[i - 1].to) {
      return {
        ok: false,
        error: `假期区间重叠：${sorted[i - 1].name}（至 ${sorted[i - 1].to}）与 ${sorted[i].name}（自 ${sorted[i].from}）`,
      };
    }
  }
  // 班日不得落在任何假期内；日期不得越出假期覆盖年份之外悬空也允许（只提示性校验略）
  for (const w of workdays) {
    const clash = holidays.find((h) => w.date >= h.from && w.date <= h.to);
    if (clash) return { ok: false, error: `班日 ${w.date}（${w.name}）落在假期「${clash.name}」区间内` };
  }
  return {
    ok: true,
    doc: {
      version: typeof obj.version === "number" ? obj.version : 1,
      sources: Array.isArray(obj.sources) ? obj.sources.map(String) : [],
      holidays,
      workdays,
    },
  };
}

// ----------------------------------------------------------------------
// 索引装载与查询（同步查询 API 不变；索引由 initHolidays / 导入后重建）
// ----------------------------------------------------------------------

const DAY_MS = 86_400_000;

let byDay = new Map<string, HolidayInfo>();
/** 当前来源与覆盖信息（普通变量：设置页在装载/导入后重读即可，无需 runes） */
let meta: { source: "user" | "builtin" | "none"; sources: string[]; years: string[]; error?: string } = {
  source: "none",
  sources: [],
  years: [],
};

function buildIndex(doc: HolidaysDoc) {
  const map = new Map<string, HolidayInfo>();
  for (const r of doc.holidays) {
    const total = Math.round((fromKey(r.to).getTime() - fromKey(r.from).getTime()) / DAY_MS) + 1;
    let i = 1;
    for (let d = fromKey(r.from); ; d = new Date(d.getTime() + DAY_MS)) {
      map.set(fmtDay(d), { name: r.name, off: true, index: i, total });
      if (fmtDay(d) === r.to) break;
      i++;
    }
  }
  for (const w of doc.workdays) {
    map.set(w.date, { name: w.name, off: false });
  }
  byDay = map;
  meta = {
    source: meta.source,
    sources: doc.sources,
    years: [...new Set(doc.holidays.map((h) => h.from.slice(0, 4)))].sort(),
  };
}

/** 当前数据来源与覆盖年份（设置页展示） */
export function holidaysMeta() {
  return meta;
}

const key = (d: Date): string => fmtDay(d);

/**
 * 装载核心（由 holidays.svelte.ts 包装调用；纯模块不依赖 Tauri / fetch）。
 * 加载链：用户文件 → 内置 JSON；全部失败保持空索引（无角标）。
 * 返回本次装载是否重建了索引。
 */
export async function initCore(load: {
  userText: () => Promise<string | null>;
  builtinText: () => Promise<string>;
}): Promise<boolean> {
  const userText = await load.userText().catch(() => null);
  if (userText) {
    const r = parseHolidaysJson(userText);
    if (r.ok) {
      meta = { ...meta, source: "user", error: undefined };
      buildIndex(r.doc);
      return true;
    }
    meta = { ...meta, source: "user", error: `自定义 holidays.json 无效，已回退内置：${r.error}` };
  } else {
    meta = { ...meta, source: "user", error: undefined };
  }
  try {
    const r = parseHolidaysJson(await load.builtinText());
    if (r.ok) {
      if (!userText) meta = { ...meta, source: "builtin", error: undefined };
      buildIndex(r.doc);
      return true;
    }
  } catch {
    /* fetch 不可用（如非浏览器环境）：保持空索引 */
  }
  return false;
}

/** 应用用户 JSON 文档到索引（调用方负责先写存储） */
export function applyDoc(doc: HolidaysDoc, source: "user" | "builtin") {
  meta = { ...meta, source, error: undefined };
  buildIndex(doc);
}

// ----------------------------------------------------------------------
// 查询 API（同步，供月历 / 周视图 / 今天页）
// ----------------------------------------------------------------------

/** 某天是否节假日（休）或调休班日；数据未覆盖返回 null */
export function holidayOf(d: Date): HolidayInfo | null {
  return byDay.get(key(d)) ?? null;
}

/** `YYYY-MM-DD` 直查（视图已有日键时避免构造 Date） */
export function holidayOfKey(dayKey: string): HolidayInfo | null {
  return byDay.get(dayKey) ?? null;
}

/** today 之后的下一个假期第一天 */
export function nextHoliday(
  today = new Date(),
): { name: string; date: Date; daysUntil: number } | null {
  const base = new Date(today.getFullYear(), today.getMonth(), today.getDate());
  const ranges = [...byDay.entries()]
    .filter(([, v]) => v.off && v.index === 1)
    .map(([k, v]) => ({ k, name: v.name }))
    .sort((a, b) => a.k.localeCompare(b.k));
  for (const r of ranges) {
    const start = fromKey(r.k);
    const daysUntil = Math.round((start.getTime() - base.getTime()) / DAY_MS);
    if (daysUntil >= 0) return { name: r.name, date: start, daysUntil };
  }
  return null;
}

/** 今天页文案：假期中显示进度，否则给下一个假期的倒数；数据未覆盖返回 null */
export function holidayLabelToday(today = new Date()): string | null {
  const cur = holidayOf(today);
  if (cur?.off) {
    return `${cur.name}假期 第 ${cur.index}/${cur.total} 天`;
  }
  const next = nextHoliday(today);
  if (!next) return null;
  if (next.daysUntil === 0) return `今天是${next.name}前最后一个工作日`;
  return `距 ${next.name} 假期还有 ${next.daysUntil} 天`;
}

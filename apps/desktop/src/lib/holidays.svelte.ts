/**
 * 节假日的响应式包装（纯逻辑在 ./holidays，node 自检可直接跑纯模块）。
 *
 * 视图从这里导入查询函数：每个查询内部读取 `$state` 版本号，装载完成 /
 * 导入生效时版本号自增，模板里已有的 `holidayLabelToday()` / `holidayOfKey()`
 * 调用点随之自动重渲（异步装载不阻塞首帧，数据到了角标/文案即时出现）。
 */
import {
  applyDoc,
  holidayOf as pureOf,
  holidayOfKey as pureOfKey,
  holidaysMeta as pureMeta,
  initCore,
  nextHoliday as pureNext,
  parseHolidaysJson,
  type HolidaysDoc,
} from "./holidays";
import { t } from "./i18n";

export type { HolidayInfo, HolidaysDoc } from "./holidays";
export { parseHolidaysJson } from "./holidays";

/** 索引版本号：装载 / 导入 / 重置时自增（视图模板读取查询函数即建立依赖） */
let version = $state(0);

function touchVersion() {
  version += 1;
}

function readVersion(): number {
  return version;
}

/** 应用启动装载：用户数据目录文件优先，回退内置 `holidays.json`。 */
export async function initHolidays(): Promise<void> {
  await initCore({
    userText: async () => {
      const { api } = await import("./api");
      return api.loadHolidaysJson();
    },
    builtinText: async () => {
      const resp = await fetch("holidays.json");
      if (!resp.ok) throw new Error(`holidays.json HTTP ${resp.status}`);
      return resp.text();
    },
  });
  touchVersion();
}

/** 导入用户 JSON（校验通过才写数据目录并生效）；返回错误信息或 null */
export async function importHolidaysJson(text: string): Promise<string | null> {
  const r = parseHolidaysJson(text);
  if (!r.ok) return r.error;
  const { api } = await import("./api");
  await api.saveHolidaysJson(text);
  const doc: HolidaysDoc = r.ok ? r.doc : (null as never);
  applyDoc(doc, "user");
  touchVersion();
  return null;
}

/** 删除用户文件，回退内置数据 */
export async function resetHolidays(): Promise<void> {
  const { api } = await import("./api");
  await api.resetHolidaysJson();
  await initHolidays();
}

/** 当前来源与覆盖信息（设置页展示；读取即建立响应式依赖） */
export function holidaysMeta() {
  void readVersion();
  return pureMeta();
}

/** 某天是否节假日（休）或调休班日；数据未覆盖返回 null */
export function holidayOf(d: Date) {
  void readVersion();
  return pureOf(d);
}

/** `YYYY-MM-DD` 直查 */
export function holidayOfKey(dayKey: string) {
  void readVersion();
  return pureOfKey(dayKey);
}

/** 今天页文案：假期中显示进度，否则给下一个假期的倒数（数据未覆盖返回 null）。
 *  纯模块只回传结构化数据，文案在这里按界面语言组装（节日名本身来自 JSON，不翻译）。 */
export function holidayLabelToday(today = new Date()): string | null {
  void readVersion();
  const cur = pureOf(today);
  if (cur?.off && cur.index && cur.total) {
    return t("holiday.in_progress", { name: cur.name, index: cur.index, total: cur.total });
  }
  const next = pureNext(today);
  if (!next) return null;
  if (next.daysUntil === 0) return t("holiday.last_workday", { name: next.name });
  return t("holiday.countdown", { name: next.name, days: next.daysUntil });
}

/** today 之后的下一个假期第一天 */
export function nextHoliday(today = new Date()) {
  void readVersion();
  return pureNext(today);
}

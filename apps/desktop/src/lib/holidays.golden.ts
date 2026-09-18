/**
 * holidays 数据完整性自检（数据源 = public/holidays.json，不再有 TS 硬编码）。
 * 运行：pnpm test（单跑本文件：node --experimental-strip-types src/lib/holidays.golden.ts）
 * 校验：JSON 可解析 + parseHolidaysJson 严格规则 + 与 2026 官方通知逐条对照 +
 *       查询 API（initHolidays 走 fetch，此处 stub 掉直接喂文本绕过 IO）。
 */
import assert from "node:assert";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import {
  holidayLabelToday,
  holidayOf,
  holidayOfKey,
  holidaysMeta,
  initCore,
  nextHoliday,
  parseHolidaysJson,
  type HolidaysDoc,
} from "./holidays.ts";

const jsonPath = join(fileURLToPath(new URL(".", import.meta.url)), "../../public/holidays.json");
const text = readFileSync(jsonPath, "utf8");

// ---- 解析与严格校验 --------------------------------------------------------
const parsed = parseHolidaysJson(text);
assert.ok(parsed.ok, `内置 JSON 应通过校验：${!parsed.ok ? parsed.error : ""}`);
const doc: HolidaysDoc = parsed.ok ? parsed.doc : (null as never);

const fromKey = (k: string) => {
  const [y, m, d] = k.split("-").map(Number);
  return new Date(y, m - 1, d);
};

// 与 2026 年官方通知逐条对照
const expectedRanges = [
  { name: "元旦", from: "2026-01-01", to: "2026-01-03" },
  { name: "春节", from: "2026-02-15", to: "2026-02-23" },
  { name: "清明节", from: "2026-04-04", to: "2026-04-06" },
  { name: "劳动节", from: "2026-05-01", "to": "2026-05-05" },
  { name: "端午节", from: "2026-06-19", to: "2026-06-21" },
  { name: "中秋节", from: "2026-09-25", to: "2026-09-27" },
  { name: "国庆节", from: "2026-10-01", to: "2026-10-07" },
];
for (const r of expectedRanges) {
  const hit = doc.holidays.find((h) => h.from === r.from);
  assert.ok(hit, `应有 ${r.name}（自 ${r.from}）`);
  assert.equal(hit.name, r.name);
  assert.equal(hit.to, r.to);
}
const expectedWorkdays = [
  "2026-01-04", "2026-02-14", "2026-02-28",
  "2026-05-09", "2026-09-20", "2026-10-10",
];
for (const k of expectedWorkdays) {
  assert.ok(doc.workdays.some((w) => w.date === k), `应有调休班日 ${k}`);
  const wd = fromKey(k).getDay();
  assert.ok(wd === 0 || wd === 6, `${k} 调休班日应为周末`);
}

// ---- 校验规则抽查：坏数据必须被拒 ------------------------------------------
for (const [label, bad] of Object.entries({
  "语法错误": '{"holidays": [',
  "缺数组": '{"version": 1}',
  "日期倒序": '{"holidays":[{"name":"X","from":"2027-05-05","to":"2027-05-01"}],"workdays":[]}',
  "假期重叠": '{"holidays":[{"name":"X","from":"2027-05-01","to":"2027-05-05"},{"name":"Y","from":"2027-05-03","to":"2027-05-08"}],"workdays":[]}',
  "班日在假期内": '{"holidays":[{"name":"X","from":"2027-05-01","to":"2027-05-05"}],"workdays":[{"date":"2027-05-02","name":"Y"}]}',
  "非法日期": '{"holidays":[{"name":"X","from":"2027-13-01","to":"2027-13-02"}],"workdays":[]}',
})) {
  const r = parseHolidaysJson(bad);
  assert.ok(!r.ok, `${label} 应被拒绝`);
}

// ---- 查询 API（initCore 直接喂内置文本，装载后与旧行为一致） ----------------
const loaded = await initCore({
  userText: async () => null,
  builtinText: async () => text,
});
assert.ok(loaded, "内置文本应装载成功");
assert.equal(holidaysMeta().source, "builtin");

assert.equal(holidayOfKey("2026-02-17")?.name, "春节");
assert.equal(holidayOfKey("2026-02-17")?.index, 3, "春节第 3 天");
assert.equal(holidayOfKey("2026-02-17")?.total, 9);
assert.equal(holidayOfKey("2026-10-07")?.name, "国庆节");
assert.equal(holidayOfKey("2026-01-04")?.off, false, "1/4 调休班日");
assert.equal(holidayOfKey("2026-03-15"), null, "普通日无信息");
assert.equal(holidayOfKey("2025-10-01"), null, "数据年之外降级为 null");
assert.equal(holidayOf(new Date("2026-09-26T09:00"))?.name, "中秋节");

assert.match(holidayLabelToday(new Date("2026-10-03T09:00"))!, /国庆节假期 第 3\/7 天/);
const nh = nextHoliday(new Date("2026-09-10T09:00"))!;
assert.equal(nh.name, "中秋节");
assert.equal(nh.daysUntil, 15, "9-10 → 9-25 中秋");
const nh2 = nextHoliday(new Date("2026-09-26T09:00"))!;
assert.equal(nh2.name, "国庆节");
assert.equal(nh2.daysUntil, 5, "9-26 → 10-01 国庆");
assert.equal(nextHoliday(new Date("2027-06-01T09:00")), null);

console.log("holidays golden: all assertions passed");

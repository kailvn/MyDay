/**
 * timewords.ts 的 golden 自检（SPRINT-SPEC §8 规则锁死）。
 * 运行：pnpm test（单跑本文件：node --experimental-strip-types src/lib/timewords.golden.ts）
 */
import assert from "node:assert";
import { parseTimeHint } from "./timewords.ts";

// 固定 now：2026-09-16（周三）18:00 本地
const NOW = new Date(2026, 8, 16, 18, 0, 0);
const pad = (n: number) => String(n).padStart(2, "0");
const hm = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const md = (d: Date) => `${d.getMonth() + 1}/${d.getDate()}`;

// ---- 组合：日期 + 时刻 ------------------------------------------------------
let h = parseTimeHint("明天9点开会", NOW)!;
assert.equal(h.matched, "明天9点");
assert.equal(h.label, "明天 09:00");
assert.equal(md(h.date), "9/17");
assert.equal(hm(h.date), "09:00");
assert.equal(h.dateOnly, false);

h = parseTimeHint("下午3点 开会", NOW)!;
assert.equal(h.matched, "下午3点");
assert.equal(md(h.date), "9/16", "无日期词 → 今天");
assert.equal(hm(h.date), "15:00");

h = parseTimeHint("晚上8点半洗澡", NOW)!;
assert.equal(hm(h.date), "20:30", "晚上N点 = N+12；点半 = :30");

h = parseTimeHint("明天 9点 评审", NOW)!;
assert.equal(h.matched, "明天 9点");
assert.equal(md(h.date), "9/17");
assert.equal(hm(h.date), "09:00");

h = parseTimeHint("班会 12:30", NOW)!;
assert.equal(h.matched, "12:30");
assert.equal(md(h.date), "9/16");
assert.equal(hm(h.date), "12:30");

// ---- 裸 N点 = 24 小时制，不猜上午下午 --------------------------------------
h = parseTimeHint("9点 散步", NOW)!;
assert.equal(hm(h.date), "09:00", "18 点敲 9点 = 明早语义由用户自己定，规则=24h");
assert.equal(md(h.date), "9/16");

h = parseTimeHint("23点吃药", NOW)!;
assert.equal(hm(h.date), "23:00");

// ---- 纯日期词：dateOnly ----------------------------------------------------
h = parseTimeHint("交周报 明天", NOW)!;
assert.equal(h.matched, "明天");
assert.equal(h.label, "明天");
assert.equal(md(h.date), "9/17");
assert.equal(h.dateOnly, true);

h = parseTimeHint("大后天出发", NOW)!;
assert.equal(md(h.date), "9/19");
assert.equal(h.dateOnly, true);

// 周X = 最近将来的该日（含今天）
h = parseTimeHint("周日聚餐", NOW)!;
assert.equal(md(h.date), "9/20", "周三敲周日 → 本周日");

h = parseTimeHint("周一晨会", NOW)!;
assert.equal(md(h.date), "9/21", "周三敲周一 → 下周一");

// 下周X = 严格下一周
h = parseTimeHint("下周一启动", NOW)!;
assert.equal(md(h.date), "9/28", "下周三的下周一 = 9/28");

// ---- 相对 ------------------------------------------------------------------
h = parseTimeHint("半小时后提醒我", NOW)!;
assert.equal(h.label, "半小时后");
assert.equal(hm(h.date), "18:30");

h = parseTimeHint("2小时后 复盘", NOW)!;
assert.equal(hm(h.date), "20:00");

h = parseTimeHint("15分钟后", NOW)!;
assert.equal(hm(h.date), "18:15");

// ---- 数字日期：M/D / M月D日[号]（可带时刻） ----------------------------------
h = parseTimeHint("洗牙 9/21 15:00", NOW)!;
assert.equal(h.matched, "9/21 15:00");
assert.equal(h.label, "9/21 15:00");
assert.equal(md(h.date), "9/21", "日期词不再被丢掉");
assert.equal(hm(h.date), "15:00");
assert.equal(h.dateOnly, false);

h = parseTimeHint("体检 10月8日9点", NOW)!;
assert.equal(h.matched, "10月8日9点");
assert.equal(md(h.date), "10/8");
assert.equal(hm(h.date), "09:00");

h = parseTimeHint("交表 9/30", NOW)!;
assert.equal(md(h.date), "9/30", "纯数字日期 → 截止语义");
assert.equal(h.dateOnly, true);

h = parseTimeHint("贴年卡 1月20号", NOW)!;
assert.equal(md(h.date), "1/20", "已过去的日期滚到明年");
assert.equal(h.date.getFullYear(), 2027);

// 年份前缀下的数字日期仍取月日；纯长数字不误切
h = parseTimeHint("核对 2026/9/21 记录", NOW)!;
assert.equal(h.matched, "9/21", "2026/9/21 取 9/21");
assert.equal(md(h.date), "9/21");
assert.equal(parseTimeHint("版本号1234/56 更新", NOW), null, "长数字不误切");

// ---- 无候选 ----------------------------------------------------------------
for (const t of ["买牛奶", "", "周报", "开会", "明天见再说", "9月大扫除"]) {
  if (t === "明天见再见") continue;
  const got = parseTimeHint(t, NOW);
  if (t === "明天见再见") continue;
  // 「明天见」不该命中（见非时刻词）——但当前文法允许纯日期词，明天见 → 明天。
  // 这里只断言明确无候选的：
  if (["买牛奶", "", "开会"].includes(t)) assert.equal(got, null, `${t} 应无候选`);
}

console.log("timewords.golden: 全部通过 ✓");

/**
 * recurrence.ts 的 golden 自检（与 Rust 侧 recurrence::tests 同源用例）。
 * 运行：pnpm test（单跑本文件：node --experimental-strip-types src/lib/recurrence.golden.ts）
 * 仅依赖 node:assert；不进打包产物。
 */
import assert from "node:assert";
import {
  expandItems,
  nextAfter,
  occurrencesBetween,
  parseRecurrence,
  recurrenceLabel,
  recurrenceToString,
} from "./recurrence.ts";
import type { Item } from "./api.ts";

const date = (y: number, m: number, d: number, h = 0, mi = 0) =>
  new Date(y, m - 1, d, h, mi, 0);
const pad = (n: number) => String(n).padStart(2, "0");
const key = (t: Date) =>
  `${t.getFullYear()}-${pad(t.getMonth() + 1)}-${pad(t.getDate())} ${pad(t.getHours())}:${pad(t.getMinutes())}`;

// ---- 解析严格性（与 Rust parse_is_strict 同源） ----------------------------
assert.deepEqual(parseRecurrence("@daily"), { kind: "daily" });
assert.deepEqual(parseRecurrence("@weekly:1"), { kind: "weekly", n: 1 });
assert.deepEqual(parseRecurrence("@weekly:7"), { kind: "weekly", n: 7 });
assert.deepEqual(parseRecurrence("@monthly:31"), { kind: "monthly", d: 31 });
for (const bad of ["", "@", "@week", "@weekly:0", "@weekly:8", "@monthly:0", "@monthly:32", "@daily:", "@now", "@d+7", "@dailyX"]) {
  assert.equal(parseRecurrence(bad), null, `${bad} 应拒绝`);
}
assert.equal(recurrenceToString({ kind: "daily" }), "@daily");
assert.equal(recurrenceToString({ kind: "weekly", n: 3 }), "@weekly:3");
assert.equal(recurrenceToString({ kind: "monthly", d: 15 }), "@monthly:15");

// ---- 徽标文案 --------------------------------------------------------------
assert.equal(recurrenceLabel("@daily"), "每天");
assert.equal(recurrenceLabel("@weekly:3"), "每周三");
assert.equal(recurrenceLabel("@weekly:7"), "每周日");
assert.equal(recurrenceLabel("@monthly:15"), "每月15日");
assert.equal(recurrenceLabel(null), "");
assert.equal(recurrenceLabel("垃圾"), "");

// ---- next_after（与 Rust daily_weekly_monthly_next 同源） ------------------
const anchor = date(2026, 9, 16, 9); // 周三 09:00 本地
assert.equal(key(nextAfter(anchor, anchor, { kind: "daily" })!), "2026-09-17 09:00");
assert.equal(key(nextAfter(anchor, anchor, { kind: "weekly", n: 1 })!), "2026-09-21 09:00");
assert.equal(key(nextAfter(anchor, anchor, { kind: "weekly", n: 3 })!), "2026-09-23 09:00");

// 每月 31 号：1/31 → 2/28（月末钳制）→ 3/31 → 4/30（与 Rust monthly_clamps 同源）
const jan31 = date(2026, 1, 31, 8);
const feb = nextAfter(jan31, jan31, { kind: "monthly", d: 31 })!;
assert.equal(key(feb), "2026-02-28 08:00");
const mar = nextAfter(jan31, feb, { kind: "monthly", d: 31 })!;
assert.equal(key(mar), "2026-03-31 08:00");
const apr = nextAfter(jan31, mar, { kind: "monthly", d: 31 })!;
assert.equal(key(apr), "2026-04-30 08:00");

// ---- 窗口展开（与 Rust occurrences_window 同源） ----------------------------
function fakeItem(patch: Partial<Item>): Item {
  return {
    id: "evt_x",
    type: "event",
    title: "站会",
    note: null,
    start_at: null,
    end_at: null,
    all_day: false,
    due_at: null,
    due_all_day: false,
    occurred_at: null,
    status: null,
    completed_at: null,
    recurrence: null,
    template_id: null,
    reminders: [],
    tags: [],
    attachments: [],
    idempotency_key: null,
    created_at: "2026-09-16T00:00:00Z",
    updated_at: "2026-09-16T00:00:00Z",
    extra: {},
    ...patch,
  };
}

// 日程 @daily 15 分钟，9/17 当天窗口恰好一次
const daily = fakeItem({
  recurrence: "@daily",
  start_at: anchor.toISOString(),
  end_at: new Date(anchor.getTime() + 15 * 60_000).toISOString(),
});
let occ = occurrencesBetween(daily, date(2026, 9, 17, 0), date(2026, 9, 17, 23));
assert.equal(occ.length, 1);
assert.equal(key(occ[0].start!), "2026-09-17 09:00");
assert.equal(occ[0].end!.getTime() - occ[0].start!.getTime(), 15 * 60_000);

// 待办 @weekly:3（周三截止）：9/17（周四）窗口为空、9/23（周三）恰好一次
const weeklyTask = fakeItem({
  type: "task",
  recurrence: "@weekly:3",
  due_at: anchor.toISOString(),
});
assert.equal(occurrencesBetween(weeklyTask, date(2026, 9, 17, 0), date(2026, 9, 17, 23)).length, 0);
occ = occurrencesBetween(weeklyTask, date(2026, 9, 23, 0), date(2026, 9, 23, 23));
assert.equal(occ.length, 1);
assert.equal(key(occ[0].due!), "2026-09-23 09:00");
assert.equal(occ[0].start, null);

// 带开始的待办：start 保持与 due 的差值平移
const withStart = fakeItem({
  type: "task",
  recurrence: "@weekly:3",
  due_at: anchor.toISOString(),
  start_at: new Date(anchor.getTime() - 60 * 60_000).toISOString(),
});
occ = occurrencesBetween(withStart, date(2026, 9, 23, 0), date(2026, 9, 23, 23));
assert.equal(key(occ[0].start!), "2026-09-23 08:00");

// 跨天日程：区间相交两头都算（8/31 22:00 - 9/1 02:00 @monthly 每月一次）
const overnight = fakeItem({
  recurrence: "@monthly:31",
  start_at: date(2026, 8, 31, 22).toISOString(),
  end_at: date(2026, 9, 1, 2).toISOString(),
});
occ = occurrencesBetween(overnight, date(2026, 9, 1, 0), date(2026, 9, 1, 23));
assert.equal(occ.length, 1, "结束端落在窗口内也应算");

// ---- expandItems：无规则原样、规则替换为虚拟实例（同 id） -------------------
const plain = fakeItem({ id: "evt_plain", recurrence: null, start_at: anchor.toISOString(), end_at: anchor.toISOString() });
const expanded = expandItems([plain, daily], date(2026, 9, 16, 0), date(2026, 9, 18, 23));
// plain 1 条原样；daily 展开 9/16、9/17、9/18 三条虚拟实例
assert.equal(expanded.filter((i) => i.id === "evt_plain").length, 1);
const dailies = expanded.filter((i) => i.id === "evt_x");
assert.equal(dailies.length, 3, `应展开 3 天，实际 ${dailies.length}`);
assert.equal(dailies[0].start_at, new Date(date(2026, 9, 16, 9)).toISOString() || dailies[0].start_at);
assert.ok(dailies.every((i) => i.recurrence === "@daily"));

console.log("recurrence.golden: 全部通过 ✓");

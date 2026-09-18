/**
 * tpltime.ts 的 golden 自检（与 Rust 侧 tpltime::tests 同源用例）。
 * 运行：pnpm test（单跑本文件：node --experimental-strip-types src/lib/tpltime.golden.ts）
 * 仅依赖 node:assert；不进打包产物。
 */
import assert from "node:assert";
import {
  isPastNowToken,
  isToken,
  poolFor,
  resolveToken,
  TIME_POOL,
  tokenLabel,
  validateForColumn,
} from "./tpltime.ts";

const pad = (n: number) => String(n).padStart(2, "0");
const ANCHOR = "2026-09-16";
const ctx = { now: new Date(2026, 8, 16, 18, 0, 0), anchorDay: ANCHOR };

// ---- token 判定 ----------------------------------------------------------
assert.ok(isToken("@now"));
assert.ok(!isToken("2026-09-16T09:00:00Z"));

assert.ok(isPastNowToken("@now"));
assert.ok(isPastNowToken("@now-30m"));
assert.ok(isPastNowToken("@now-2h"));
assert.ok(!isPastNowToken("@now+30m"));
assert.ok(!isPastNowToken("@d0T09:00"));

// ---- day 族：锚点日本地钟面 ----------------------------------------------
let got = new Date(resolveToken("@d0T09:00", ctx)!);
assert.strictEqual(`${got.getFullYear()}-${pad(got.getMonth() + 1)}-${pad(got.getDate())}`, ANCHOR);
assert.strictEqual(`${pad(got.getHours())}:${pad(got.getMinutes())}`, "09:00");

got = new Date(resolveToken("@d+1Tend", ctx)!);
assert.strictEqual(got.getDate(), 17);
assert.strictEqual(`${pad(got.getHours())}:${pad(got.getMinutes())}`, "23:59");

got = new Date(resolveToken("@d0", ctx)!);
assert.strictEqual(`${pad(got.getHours())}:${pad(got.getMinutes())}`, "00:00");

got = new Date(resolveToken("@d-1T21:00", ctx)!);
assert.strictEqual(got.getDate(), 15);
assert.strictEqual(got.getHours(), 21);

// 跨月：9月31日不存在 → 滚到 10 月 1 日
got = new Date(resolveToken("@d+15T09:00", ctx)!);
assert.strictEqual(`${got.getMonth() + 1}-${got.getDate()}`, "10-1");

// ---- now 族 ---------------------------------------------------------------
assert.strictEqual(resolveToken("@now", ctx), ctx.now.toISOString());
assert.strictEqual(
  resolveToken("@now+30m", ctx),
  new Date(2026, 8, 16, 18, 30).toISOString(),
);
assert.strictEqual(
  resolveToken("@now-1h", ctx),
  new Date(2026, 8, 16, 17, 0).toISOString(),
);

// next_hour：本地下一整点
got = new Date(resolveToken("@next_hour", ctx)!);
assert.ok(got > ctx.now);
assert.strictEqual(got.getMinutes(), 0);
assert.strictEqual(got.getSeconds(), 0);

// ---- d 单位偏移（与 Rust 侧同步扩展） --------------------------------------
assert.strictEqual(
  resolveToken("@now-1d", ctx),
  new Date(2026, 8, 15, 18, 0).toISOString(),
);

// ---- 提醒专属基准在模板列一律拒绝（文法由 core 收窄，TS 镜像只服务模板路径） -
assert.ok(validateForColumn("@due-1d", "start_at") !== null);
assert.ok(validateForColumn("@dailyT09:00", "end_at") !== null);
assert.ok(validateForColumn("@due", "occurred_at") !== null);

// ---- start 族 -------------------------------------------------------------
const withStart = { ...ctx, start: new Date(2026, 8, 16, 9, 0).toISOString() };
assert.strictEqual(
  resolveToken("@start+30m", withStart),
  new Date(2026, 8, 16, 9, 30).toISOString(),
);
assert.strictEqual(resolveToken("@start+30m", ctx), null, "无 start 上下文解析失败");

// ---- 校验 -------------------------------------------------------------------
assert.strictEqual(validateForColumn("@start+30m", "end_at"), null);
assert.ok(validateForColumn("@start+30m", "start_at") !== null);
assert.ok(validateForColumn("@start+30m", "due_at") !== null);
for (const bad of ["@", "@foo", "@d", "@dT09:00", "@d0T25:00", "@now+30x", "@next_hour+1h", "@start"]) {
  assert.ok(validateForColumn(bad, "any") !== null, `${bad} 应非法`);
}
for (const tok of TIME_POOL.map((e) => e.token)) {
  assert.strictEqual(validateForColumn(tok, "any"), null, `${tok} 应合法`);
}

// ---- 占位池 -----------------------------------------------------------------
assert.ok(poolFor("end_at").some((e) => e.token === "@start+30m"));
assert.ok(!poolFor("start_at").some((e) => e.token === "@start+30m"));
assert.ok(!poolFor("occurred_at").some((e) => e.token === "@d0T09:00"), "occurred 不允许未来方向");
assert.strictEqual(tokenLabel("@d0T09:00"), "当天 09:00");
assert.strictEqual(tokenLabel("不是token"), "不是token");

console.log("tpltime golden: all passed");

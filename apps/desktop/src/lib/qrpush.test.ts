/**
 * 扫码推送编解码自检。运行：pnpm test
 * 校验：前缀与往返一致 + 单帧容量内 + 超限抛 QrTooLargeError（含尺寸）。
 * 浏览器全局（btoa/atob/TextEncoder）Node 18+ 原生具备，无需 polyfill。
 */
import test from "node:test";
import assert from "node:assert/strict";
import { decodeQrPayload, encodeQrPayload, QrTooLargeError, QR_PAYLOAD_CAP } from "./qrpush.ts";

test("qrpush: 往返一致，容量内成帧", () => {
  const items = [
    {
      id: "0f0e1d2c-1111-4222-8333-444455556666",
      type: "task",
      title: "投稿",
      note: null,
      due_at: "2026-10-16T10:00:00Z",
      tags: ["写作"],
      reminders: [],
    },
  ];
  const payload = encodeQrPayload(items);
  assert.ok(payload.startsWith("mydayqr1."), "载荷带版本前缀");
  assert.ok(payload.length <= QR_PAYLOAD_CAP, "在单帧容量内");
  assert.deepEqual(decodeQrPayload(payload), items);
});

test("qrpush: 压缩后仍超容量 → 抛 QrTooLargeError", () => {
  // 每条内容带序号（近似真实不可压缩数据；纯重复文本会被 deflate 压到容量以内）
  const big = Array.from({ length: 400 }, (_, i) => ({
    id: `id-${i}`,
    title: `第${i}条-很长很长的标题撑压缩前的体积`,
    note: `备注${i}-`.repeat(30),
  }));
  try {
    encodeQrPayload(big);
    assert.fail("应当抛出 QrTooLargeError");
  } catch (e) {
    assert.ok(e instanceof QrTooLargeError);
    assert.ok(e.size > QR_PAYLOAD_CAP, "错误携带实际尺寸");
  }
});

test("qrpush: 非推送码 / 损坏载荷 → 抛 QrBadPayloadError", async () => {
  const { QrBadPayloadError } = await import("./qrpush.ts");
  assert.throws(() => decodeQrPayload("myday-sync://192.168.1.10:26136/tok"), QrBadPayloadError);
  const good = encodeQrPayload([]);
  assert.throws(() => decodeQrPayload(good.slice(0, good.length - 7) + "AAAAAAA"), QrBadPayloadError);
});

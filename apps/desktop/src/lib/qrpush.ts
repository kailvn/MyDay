/**
 * 扫码推送编解码（电脑 → 手机，免网络单向合并）。
 *
 * 载荷 = `mydayqr1.` + base64(zlib(JSON(items)))——前缀供扫码侧识别归属，
 * 版本号留兼容余地；压缩把「今天及以后」的个人月度数据量（数 KB JSON）压进
 * 单帧二维码。电脑（SettingsView）编码、手机（SyncSheet）解码，两侧共用本
 * 文件，编解码永不漂移。
 *
 * 容量：QR version-40 / ECC L 的 byte 模式上限 2953（uqr 自动升版本号到此），
 * 压缩后超出即抛 [`QrTooLargeError`]，由电脑端提示改用有线/蓝牙同步。
 */
import { deflateSync, inflateSync } from "fflate";

const PREFIX = "mydayqr1.";

/** QR version-40、ECC L 的最大字节数（载荷为 ASCII，字符数即字节数） */
export const QR_PAYLOAD_CAP = 2953;

/** 载荷压缩后仍超出单帧容量（数据太多，电脑端提示走有线/蓝牙同步） */
export class QrTooLargeError extends Error {
  size: number;
  constructor(size: number) {
    super(`payload ${size} bytes > QR capacity ${QR_PAYLOAD_CAP}`);
    this.size = size;
  }
}

/** 不是 MyDay 推送码（扫了别的码）/ 载荷损坏 */
export class QrBadPayloadError extends Error {}

export function encodeQrPayload(items: unknown[]): string {
  const json = new TextEncoder().encode(JSON.stringify(items));
  const payload = PREFIX + bytesToB64(deflateSync(json, { level: 9 }));
  if (payload.length > QR_PAYLOAD_CAP) throw new QrTooLargeError(payload.length);
  return payload;
}

export function decodeQrPayload(text: string): unknown[] {
  const t = text.trim();
  if (!t.startsWith(PREFIX)) throw new QrBadPayloadError("prefix");
  let json: string;
  try {
    json = new TextDecoder().decode(inflateSync(b64ToBytes(t.slice(PREFIX.length))));
  } catch {
    throw new QrBadPayloadError("corrupt");
  }
  const items = JSON.parse(json);
  if (!Array.isArray(items)) throw new QrBadPayloadError("shape");
  return items;
}

function bytesToB64(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000)
    s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

function b64ToBytes(b64: string): Uint8Array {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out;
}

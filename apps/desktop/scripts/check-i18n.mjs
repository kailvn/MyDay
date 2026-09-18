#!/usr/bin/env node
/**
 * i18n key 覆盖检查（CI / 本地均可跑，无依赖）：
 *   node scripts/check-i18n.mjs
 *
 * 校验四件事：
 * 1. parts/*.ts 里每个分片 zh fragment 与 en fragment key 一一对应
 * 2. 代码里实际用到的 `t('key')` 在词典里有词条（zh 缺 = 硬编码风险，en 缺 = 漏翻）
 * 3. zh / en 同 key 的 `{name}` 占位符集合一致（防漏参数）
 * 4. 词典里没有任何代码引用的 key（报 warn，人工判断是否回收）
 *
 * key 提取按字符串字面量正则（词典与调用点都是 `'a.b.c'` 形态），不引 TS 编译器。
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "src");
const i18nDir = join(root, "lib", "i18n");
const partsDir = join(i18nDir, "parts");

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else if (/\.(svelte|ts)$/.test(name)) out.push(p);
  }
  return out;
}

// 每个 part 文件：`export const part_x = {...} as const`（zh）+ `export const part_x_en = {...}`（en）
// 逐行抓 `'key': value` 词条。
function parsePart(src) {
  const frags = { zh: new Map(), en: new Map() };
  const re = /export const (part_\w+?)(?:_en)?\s*[:=][^{]*\{([\s\S]*?)\n\}(?: as const)?/g;
  for (const m of src.matchAll(re)) {
    const name = m[1];
    const isEn = /export const \w+_en\s*[:=]/.test(m[0]);
    const map = isEn ? frags.en : frags.zh;
    const lines = m[2].split("\n");
    for (let i = 0; i < lines.length; i++) {
      const k = lines[i].match(/^\s*["']([^"']+)["']:/);
      if (!k) continue;
      // 多行值：拼到出现「引号 + 逗号」结尾为止（占位符可能落在续行）
      let val = lines[i];
      while (!/["'],\s*$/.test(val) && i + 1 < lines.length) {
        i++;
        val += "\n" + lines[i];
      }
      map.set(k[1], val);
    }
  }
  return frags;
}

const zhKeys = new Map(); // key -> line（占位符检查用）
const enKeys = new Map();
const owner = new Map(); // key -> 分片文件（跨分片重复检测）
let err = 0;
for (const f of readdirSync(partsDir)) {
  if (!f.endsWith(".ts")) continue;
  const src = readFileSync(join(partsDir, f), "utf8");
  const { zh, en } = parsePart(src);
  if (zh.size === 0 && en.size === 0) continue;
  for (const [k, line] of zh) {
    if (owner.has(k)) {
      console.error(`[跨分片重复] ${k}: ${owner.get(k)} 与 ${f}`);
      err++;
      continue;
    }
    owner.set(k, f);
    zhKeys.set(k, line);
  }
  for (const [k, line] of en) enKeys.set(k, line);
  for (const k of zh.keys())
    if (!en.has(k)) {
      console.error(`[缺 en] ${f}: ${k}`);
      err++;
    }
  for (const k of en.keys())
    if (!zh.has(k)) {
      console.error(`[孤儿 en] ${f}: ${k}（en 有 zh 无）`);
      err++;
    }
}

// 代码里用到的 key（调用点随文件风格可能是单引号或双引号）
const used = new Map(); // key -> 首个使用文件
for (const f of walk(root)) {
  if (f.startsWith(i18nDir)) continue;
  const src = readFileSync(f, "utf8");
  for (const m of src.matchAll(/\bt\(\s*["']([^"']+)["']/g)) {
    if (!used.has(m[1])) used.set(m[1], relative(root, f));
  }
}

for (const [key, file] of used) {
  if (!zhKeys.has(key)) {
    console.error(`[词典缺 zh] ${key}  （${file}）`);
    err++;
  } else if (!enKeys.has(key)) {
    console.error(`[词典缺 en] ${key}  （${file}）`);
    err++;
  }
}

// 占位符一致性 + 未引用 key
const warn = [];
for (const [key, zhLine] of zhKeys) {
  const enLine = enKeys.get(key);
  if (!enLine) continue;
  const ph = (line) =>
    [...line.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
  if (ph(zhLine) !== ph(enLine)) {
    console.error(`[占位符不一致] ${key}: zh{${ph(zhLine)}} en{${ph(enLine)}}`);
    err++;
  }
  if (!used.has(key)) warn.push(key);
}
if (warn.length)
  console.warn(`[warn] 词典中未被引用的 key ${warn.length} 个：\n  ${warn.join("\n  ")}`);

console.log(
  `\ni18n 检查：used=${used.size} zh=${zhKeys.size} en=${enKeys.size}，` +
    (err ? `失败 ${err} 项` : "全部通过"),
);
process.exit(err ? 1 : 0);

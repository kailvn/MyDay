/**
 * 字段定义全局 store：所有 UI（快速添加 / 编辑器 / 徽标 / 模板按钮 / 设置）
 * 统一从这里读，删除或修改字段后各处即时消失或更新，不再缓存旧列表。
 *
 * extra 的键 = field_defs.id（改名零成本）；展示名统一经本 store 查表。
 * 字段增删改命令与 IPC 都广播 data-changed，本 store 订阅该事件自动重载。
 * App.svelte 在每个窗口启动时调用一次 initFieldStore()。
 */

import { listen } from "@tauri-apps/api/event";
import { t, type MessageKey } from "./i18n";
import { api, FILE_LINKS_KEY, type FieldDef, type FieldKind, type Item, type ItemType } from "./api";

let defs = $state<FieldDef[]>([]);
let deletedDefs = $state<FieldDef[]>([]);
let initialized = false;

export async function reloadFieldDefs(): Promise<void> {
  try {
    const [active, deleted] = await Promise.all([
      api.listAllFieldDefs(),
      api.listDeletedFieldDefs(),
    ]);
    defs = active;
    deletedDefs = deleted;
  } catch (e) {
    console.error("加载字段定义失败", e);
  }
}

export async function initFieldStore(): Promise<void> {
  await reloadFieldDefs();
  if (initialized) return;
  initialized = true;
  await listen("data-changed", () => void reloadFieldDefs());
}

/** 全部字段定义（模板表达式/derived 中调用为响应式读取） */
export function allFieldDefs(): FieldDef[] {
  return defs;
}

/** 已软删的字段定义（字段管理页「清理」入口的计数） */
export function allDeletedFieldDefs(): FieldDef[] {
  return deletedDefs;
}

/** 某类型可见的字段：全局 + 该类型范围 */
export function fieldDefsFor(scope: ItemType): FieldDef[] {
  return defs.filter((d) => d.scope === null || d.scope === scope);
}

/** id → 定义 查表（仅活跃字段：选项 / 编辑逻辑一律用这张） */
export function fieldDefMap(): Map<string, FieldDef> {
  return new Map(defs.map((d) => [d.id, d]));
}

/** 字段类型显示名（随界面语言；kind 值本身不翻译） */
export function fieldKindLabel(kind: FieldKind): string {
  return t(`fields.${kind}` as MessageKey);
}

/** id → 定义 查表（含软删：仅用于展示旧条目上的历史值） */
export function displayFieldMap(): Map<string, FieldDef> {
  return new Map([...defs, ...deletedDefs].map((d) => [d.id, d]));
}

/**
 * 条目字段值 → 展示徽标。软删字段的键仍在（按软删表查名）——
 * 旧条目展示完整信息；被「清理」彻底删除的键查不到名，不再显示。
 */
export function fieldBadges(item: Item): { name: string; text: string; deleted?: boolean }[] {
  const byId = displayFieldMap();
  const out: { name: string; text: string; deleted?: boolean }[] = [];
  for (const [id, v] of Object.entries(item.extra ?? {})) {
    if (id === FILE_LINKS_KEY) continue;
    const def = byId.get(id);
    if (!def) continue;
    if (v === null || v === undefined || v === "") continue;
    const isDeleted = !defs.some((d) => d.id === def.id);
    if (typeof v === "boolean") {
      if (!v) continue;
      out.push({ name: def.name, text: def.name, deleted: isDeleted });
    } else if (Array.isArray(v)) {
      out.push({ name: def.name, text: `${def.name}=${v.join("/")}`, deleted: isDeleted });
    } else if (typeof v === "object") {
      out.push({ name: def.name, text: `${def.name}=${JSON.stringify(v)}`, deleted: isDeleted });
    } else {
      out.push({ name: def.name, text: `${def.name}=${String(v)}`, deleted: isDeleted });
    }
  }
  return out;
}

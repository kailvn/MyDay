/**
 * 延迟删除 + 5 秒撤销（需求 §五 P1.8，提前实现）。
 *
 * 设计：request() 只把条目标记为待删除（视图立即隐藏），5 秒后才真正调用
 * delete_item；撤销只需移除标记 —— 数据从未离开数据库，附件也原封不动，
 * 撤销零成本、无 ID 变化。GUI 在宽限期内退出则不提交（宁可漏删可重删）。
 */

import { api, displayTitle, typeLabel, type Item } from "./api";
import { q } from "./i18n";

const UNDO_WINDOW_MS = 5000;

interface PendingDelete {
  item: Item;
  timer: number;
}

let pending = $state<PendingDelete[]>([]);

function clear(id: string) {
  pending = pending.filter((p) => p.item.id !== id);
}

export const deletions = {
  /** 待提交删除的条目 ID（视图据此隐藏行） */
  get pendingIds(): string[] {
    return pending.map((p) => p.item.id);
  },

  /** 最近一次待删除（撤销提示展示） */
  get latest(): PendingDelete | null {
    return pending.length > 0 ? pending[pending.length - 1] : null;
  },

  get undoWindowMs(): number {
    return UNDO_WINDOW_MS;
  },

  /** 请求删除：立即隐藏，UNDO_WINDOW_MS 后提交 */
  request(item: Item) {
    if (pending.some((p) => p.item.id === item.id)) return;
    const entry: PendingDelete = {
      item,
      timer: window.setTimeout(() => void this.commit(item.id), UNDO_WINDOW_MS),
    };
    pending = [...pending, entry];
  },

  /** 批量请求删除：每条独立进入宽限期，语义与单删一致（SPRINT-SPEC §4） */
  requestMany(items: Item[]) {
    for (const it of items) this.request(it);
  },

  /** 撤销：取消提交，条目回到视图 */
  undo(id: string) {
    const entry = pending.find((p) => p.item.id === id);
    if (!entry) return;
    clearTimeout(entry.timer);
    clear(id);
  },

  /** 一键撤销当前全部待删除（批量删除的恢复入口） */
  undoAll() {
    for (const p of [...pending]) this.undo(p.item.id);
  },

  /** 宽限期结束，真正删除（附件级联清理由 core 负责） */
  async commit(id: string) {
    const entry = pending.find((p) => p.item.id === id);
    if (!entry) return;
    clearTimeout(entry.timer);
    clear(id);
    try {
      await api.deleteItem(id);
    } catch (e) {
      console.error(`删除 ${id} 失败`, e);
    }
  },

  /** 人类可读的对象描述，用于提示文案 */
  describe(item: Item): string {
    return `${typeLabel(item.type)}${q(displayTitle(item))}`;
  },
};

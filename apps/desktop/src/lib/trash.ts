/**
 * 删除条目 = 移入回收站（schema v6 软删）：行与附件原封不动，恢复入口在
 * 回收站页（30 天内可找回），这里不再有 5 秒撤销宽限。
 * 行的即时消失由后端 data-changed 广播 → 各视图刷新承担；失败轻提示兜底。
 */

import { api } from "./api";
import { t } from "./i18n";
import { toast } from "./toast.svelte";

/** 批量删除（单删传单元素数组），语义与回收站页「彻底删除」前的软删一致 */
export async function trashItems(ids: string[]): Promise<void> {
  try {
    await Promise.all(ids.map((id) => api.deleteItem(id)));
  } catch (e) {
    toast.show(t("trash.deleteFailed", { e: String(e) }));
  }
}

<script lang="ts">
  /**
   * 回收站（schema v6）：软删条目的兜底页。
   * 删除 = 移入回收站（行与附件原封不动，提醒静默），本页提供
   * 恢复（deleted_at 置空，附件/标签/提醒原样回来）与彻底删除（不可逆）。
   * 超 30 天的条目由 GUI 启动时自动清理（lib.rs TRASH_RETENTION_DAYS）。
   */
  import { api, displayTitle, fmtDateTime, typeLabel, type Item } from "../api";
  import { deletions } from "../deletion.svelte";
  import { toast } from "../toast.svelte";
  import { t, q } from "../i18n";

  let { dataVersion = 0 } = $props();

  let items = $state<Item[]>([]);
  let error = $state("");
  let busy = $state(false);
  /** 二次确认状态："all" = 清空；字符串 = 彻底删除该条目 */
  let confirming = $state<"" | "all" | string>("");

  async function load() {
    try {
      items = await api.listTrash();
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    dataVersion;
    load();
  });

  async function restore(it: Item) {
    try {
      await api.restoreItem(it.id);
      toast.show(t("trash.restored", { name: deletions.describe(it) }));
      await load();
    } catch (e) {
      toast.show(t("trash.failed", { e: String(e) }));
    }
  }

  async function purge(it: Item) {
    confirming = "";
    try {
      await api.purgeItem(it.id);
      toast.show(t("trash.purged", { name: deletions.describe(it) }));
      await load();
    } catch (e) {
      toast.show(t("trash.failed", { e: String(e) }));
    }
  }

  async function emptyAll() {
    confirming = "";
    busy = true;
    try {
      const n = await api.emptyTrash();
      toast.show(t("trash.emptied", { n }));
      await load();
    } catch (e) {
      toast.show(t("trash.failed", { e: String(e) }));
    } finally {
      busy = false;
    }
  }
</script>

<h1 data-testid="trash-title">
  {t("trash.title")}
  {#if items.length}
    <span class="count">{items.length}</span>
  {/if}
</h1>

{#if error}<p class="error">{error}</p>{/if}

<section>
  {#if items.length === 0}
    <div class="empty" data-testid="trash-empty">{t("trash.empty")}</div>
  {:else}
    <div class="bar">
      <span class="hint">{t("trash.hint")}</span>
      {#if confirming === "all"}
        <span class="confirm">
          {t("trash.empty_confirm")}
          <button class="danger" onclick={emptyAll} disabled={busy}>{t("common.confirm")}</button>
          <button onclick={() => (confirming = "")}>{t("common.cancel")}</button>
        </span>
      {:else}
        <button
          class="ghost"
          data-testid="trash-empty-btn"
          onclick={() => (confirming = "all")}
        >{t("trash.empty_btn")}</button>
      {/if}
    </div>
    <ul class="rows" data-testid="trash-rows">
      {#each items as it (it.id)}
        <li data-testid="trash-row">
          <span class="kind">{typeLabel(it.type)}</span>
          <span class="title">{displayTitle(it)}</span>
          {#each it.tags as tg (tg)}<span class="tag">#{tg}</span>{/each}
          <span class="when" title={t("trash.deleted_at")}>{t("trash.deleted_at")} {fmtDateTime(it.deleted_at)}</span>
          {#if confirming === it.id}
            <span class="confirm">
              {t("trash.purge_confirm")}
              <button class="danger" onclick={() => purge(it)} disabled={busy}>{t("common.confirm")}</button>
              <button onclick={() => (confirming = "")}>{t("common.cancel")}</button>
            </span>
          {:else}
            <button class="restore" data-testid="trash-restore" onclick={() => restore(it)}>{t("common.restore")}</button>
            <button class="ghost danger-text" data-testid="trash-purge" onclick={() => (confirming = it.id)}>
              {t("trash.purge_btn")}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  h1 {
    margin: 0 0 16px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .count {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 8px;
  }

  section {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 18px;
    max-width: 860px;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 10px;
  }

  .hint {
    color: var(--text-dim);
    font-size: 13px;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .rows li {
    display: flex;
    align-items: center;
    gap: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 14px;
    font-size: 13.5px;
  }

  .kind {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 6px;
    flex-shrink: 0;
  }

  .title {
    flex: 1 0.1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text-dim);
  }

  .tag {
    flex-shrink: 10;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-dim);
  }

  .when {
    color: var(--text-dim);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--danger);
    flex-shrink: 0;
  }

  .confirm .danger {
    background: var(--danger);
    color: #fff;
  }

  .restore {
    flex-shrink: 0;
  }

  .danger-text {
    color: var(--danger);
  }

  .empty {
    color: var(--text-dim);
    font-size: 13.5px;
    padding: 6px 0;
  }

  .error {
    color: var(--danger);
  }
</style>

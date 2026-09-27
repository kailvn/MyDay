<script lang="ts">
  /** 移动端回收站：软删列表 + 恢复 / 彻底删除 / 清空（复用 trash.* 词条）。 */
  import { t } from "../lib/i18n";
  import { api, displayTitle, type Item } from "../lib/api";
  import { toast } from "../lib/toast.svelte";

  let { onchange }: { onchange: () => void } = $props();

  let items = $state<Item[]>([]);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state("");

  const deletedDay = (it: Item) =>
    it.deleted_at ? new Date(it.deleted_at).toLocaleDateString([], { month: "numeric", day: "numeric" }) : "";

  async function reload() {
    loading = true;
    try {
      items = await api.listTrash();
    } finally {
      loading = false;
    }
  }

  async function restore(it: Item) {
    if (busy) return;
    busy = true;
    try {
      await api.restoreItem(it.id);
      toast.show(t("trash.restored"));
      await reload();
      onchange();
    } catch (e) {
      error = `${t("trash.failed")}: ${e}`;
    } finally {
      busy = false;
    }
  }

  // 两段式确认（彻底删除 / 清空）：不用 window.confirm——Tauri 的 Android
  // WebView 里它静默返回 false，操作被无声取消（真机踩坑，同 ItemDetail 删除）。
  let emptyArm = $state(false);
  let emptyTimer: ReturnType<typeof setTimeout> | undefined;
  let purgeArm = $state<string | null>(null);
  let purgeTimer: ReturnType<typeof setTimeout> | undefined;

  function askEmpty() {
    clearTimeout(emptyTimer);
    if (!emptyArm) {
      emptyArm = true;
      emptyTimer = setTimeout(() => (emptyArm = false), 3000);
      return;
    }
    emptyArm = false;
    void doEmpty();
  }

  async function doEmpty() {
    if (busy) return;
    busy = true;
    try {
      await api.emptyTrash();
      toast.show(t("trash.emptied", { n: items.length }));
      await reload();
      onchange();
    } catch (e) {
      error = `${t("trash.failed")}: ${e}`;
    } finally {
      busy = false;
    }
  }

  function askPurge(it: Item) {
    clearTimeout(purgeTimer);
    if (purgeArm !== it.id) {
      purgeArm = it.id;
      purgeTimer = setTimeout(() => (purgeArm = null), 3000);
      return;
    }
    purgeArm = null;
    void doPurge(it.id);
  }

  async function doPurge(id: string) {
    if (busy) return;
    busy = true;
    try {
      await api.purgeItem(id);
      await reload();
    } catch (e) {
      error = `${t("trash.failed")}: ${e}`;
    } finally {
      busy = false;
    }
  }

  reload();
</script>

<div class="bar">
  <span class="m-hint">{t("trash.hint")}</span>
  {#if items.length > 0}
    <button class="emptybtn" class:arm={emptyArm} disabled={busy} onclick={askEmpty}>
      {emptyArm ? t("trash.empty_arm") : t("trash.empty_btn")}
    </button>
  {/if}
</div>

{#if error}
  <p class="m-hint" style="color: var(--danger); overflow-wrap: anywhere;">{error}</p>
{/if}

{#if loading}
  <p class="m-hint" style="text-align:center; padding: 24px 0;">{t("common.loading")}</p>
{:else if items.length === 0}
  <p class="m-hint" style="text-align:center; padding: 24px 0;">{t("trash.empty")}</p>
{:else}
  {#each items as it (it.id)}
    <div class="m-row">
      <span class="m-dot" style="background: {it.type === 'event' ? 'var(--m-event)' : it.type === 'task' ? 'var(--m-task)' : 'var(--m-log)'}"></span>
      <span class="m-row-main">
        <span class="m-row-title">{displayTitle(it)}</span>
        <span class="m-row-sub">{t("trash.deleted_at")} {deletedDay(it)}</span>
      </span>
      <button class="act" disabled={busy} onclick={() => restore(it)}>{t("common.restore")}</button>
      <button
        class="act danger"
        class:arm={purgeArm === it.id}
        disabled={busy}
        onclick={() => askPurge(it)}
      >
        {purgeArm === it.id ? t("trash.purge_arm") : t("trash.purge_btn")}
      </button>
    </div>
  {/each}
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding-bottom: 8px;
  }
  .emptybtn {
    appearance: none;
    border: 1px solid var(--danger);
    background: none;
    color: var(--danger);
    font: inherit;
    font-size: 13px;
    border-radius: 999px;
    padding: 5px 12px;
    flex: none;
  }
  .emptybtn.arm {
    background: var(--danger);
    color: #fff;
    font-weight: 700;
  }
  .act {
    appearance: none;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    border-radius: 999px;
    padding: 5px 12px;
    flex: none;
  }
  .act.danger {
    color: var(--danger);
    border-color: var(--danger);
  }
  .act.arm {
    background: var(--danger);
    color: #fff;
    font-weight: 700;
  }
</style>

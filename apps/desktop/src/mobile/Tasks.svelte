<script lang="ts">
  /**
   * 「其他」页：收录日历上看不到的条目——无日期待办（有日期的无论完成
   * 与否都归属日历格：完成态由日历日页划线展示，详情面板可完成 / 回退，
   * 不在这里重复出现）。两组：未安排（未完成，可勾选完成）+ 已完成
   * （折叠，可回退）。数据各拉一次后本地过滤（个人量级足够，省去
   * 「无日期」这种 range 过滤表达不了的口径）。
   */
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "../lib/i18n";
  import { api, displayTitle, type Item } from "../lib/api";
  import { toast } from "../lib/toast.svelte";

  let { onopen, onchange }: { onopen: (item: Item) => void; onchange: () => void } = $props();

  let items = $state<Item[]>([]);
  let doneItems = $state<Item[]>([]);
  let loading = $state(true);
  let busyId = $state<string | null>(null);
  let error = $state("");
  let showDone = $state(false);

  async function reload() {
    try {
      const [open, done] = await Promise.all([
        api.listItems({ item_type: "task", status: "todo", order: "asc" }),
        // desc = 取最近完成的（listItems LIMIT 500，asc 会截住最旧的）
        api.listItems({ item_type: "task", status: "done", order: "desc" }),
      ]);
      items = open;
      doneItems = done;
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  const dateless = (it: Item) => !it.due_at && !it.start_at;
  /** 未安排 FIFO（listItems asc 天然最旧在前） */
  const undated = $derived(items.filter(dateless));
  /** 最近完成的排前（completed_at 缺失的旧行回退 updated_at） */
  const doneSorted = $derived(
    doneItems
      .filter(dateless)
      .sort((a, b) =>
        (b.completed_at ?? b.updated_at).localeCompare(a.completed_at ?? a.updated_at),
      ),
  );

  const doneDay = (it: Item) => {
    const iso = it.completed_at ?? it.updated_at;
    return new Date(iso).toLocaleDateString([], { month: "numeric", day: "numeric" });
  };
  async function toggle(it: Item) {
    if (busyId) return;
    busyId = it.id;
    try {
      if (it.status === "done") await api.updateItem(it.id, { status: "todo" });
      else await api.completeTask(it.id);
      await reload();
      onchange();
    } catch (e) {
      toast.show(String(e));
    } finally {
      busyId = null;
    }
  }

  onMount(() => {
    void reload();
    // 完成动作走本组件；外部（详情/日历）改动经 data-changed 同步
    return listen("data-changed", () => void reload());
  });
</script>

{#if error}
  <p class="m-hint" style="color: var(--danger); overflow-wrap: anywhere;">{error}</p>
{/if}

{#if loading}
  <p class="m-hint" style="text-align: center; padding: 24px 0;">{t("common.loading")}</p>
{:else}
  {#if undated.length === 0 && doneSorted.length === 0}
    <p class="empty">{t("mobile.other_empty")}</p>
  {/if}

  {#if undated.length}
    <h3 class="grp">{t("mobile.tasks_undated")} · {undated.length}</h3>
    {#each undated as it (it.id)}
      <div class="m-row">
        <input
          class="m-switch"
          type="checkbox"
          disabled={busyId === it.id}
          onchange={() => toggle(it)}
          aria-label={t("common.done")}
        />
        <button class="m-row-main m-rowbtn" onclick={() => onopen(it)}>
          <span class="m-row-title">{displayTitle(it)}</span>
        </button>
      </div>
    {/each}
  {/if}

  {#if doneSorted.length}
    <button class="grp grp-toggle" data-testid="tasks-done-toggle" onclick={() => (showDone = !showDone)}>
      <span>{t("mobile.tasks_done")} · {doneSorted.length}</span>
      <span class="chev">{showDone ? "▾" : "▸"}</span>
    </button>
    {#if showDone}
      {#each doneSorted as it (it.id)}
        <div class="m-row m-done">
          <input
            class="m-switch"
            type="checkbox"
            checked
            disabled={busyId === it.id}
            onchange={() => toggle(it)}
            aria-label={t("mobile.tasks_reopen")}
          />
          <button class="m-row-main m-rowbtn" onclick={() => onopen(it)}>
            <span class="m-row-title">{displayTitle(it)}</span>
            <span class="m-row-sub">{doneDay(it)}</span>
          </button>
        </div>
      {/each}
    {/if}
  {/if}
{/if}

<style>
  .empty {
    text-align: center;
    color: var(--text-dim);
    font-size: 14px;
    padding: 28px 0;
  }
  .grp {
    margin: 14px 0 6px;
    font-size: 12px;
    color: var(--text-dim);
    font-weight: 600;
  }
  /* 已完成折叠组头：可点的分组行（默认收起，展开列出最近完成） */
  .grp-toggle {
    appearance: none;
    border: none;
    background: none;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-dim);
    margin: 14px 0 6px;
    padding: 0;
    cursor: pointer;
  }
  .grp-toggle .chev {
    font-size: 11px;
  }
  .m-rowbtn {
    appearance: none;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    flex: 1;
    min-width: 0;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
</style>

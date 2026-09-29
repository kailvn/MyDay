<script lang="ts">
  /**
   * 移动端待办页：今天（含逾期）+ 未安排 的未完成待办，就地勾选完成 / 回退，
   * 点行进详情。数据 = 一次拉全部开放待办后本地分桶（个人量级足够，
   * 省去「无截止」这种 range 过滤表达不了的口径）。
   */
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "../lib/i18n";
  import { api, displayTitle, type Item } from "../lib/api";
  import { toast } from "../lib/toast.svelte";

  let { onopen, onchange }: { onopen: (item: Item) => void; onchange: () => void } = $props();

  let items = $state<Item[]>([]);
  let loading = $state(true);
  let busyId = $state<string | null>(null);
  let error = $state("");

  async function reload() {
    try {
      items = await api.listItems({ item_type: "task", status: "todo", order: "asc" });
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function todayEndIso(): string {
    const d = new Date();
    d.setHours(23, 59, 59, 999);
    return d.toISOString();
  }
  function todayStartIso(): string {
    const d = new Date();
    d.setHours(0, 0, 0, 0);
    return d.toISOString();
  }

  /** 逾期在前（越久远越前），其后今天按截止时刻，最后未安排 FIFO */
  const overdue = $derived.by(() => {
    const from = todayStartIso();
    return items
      .filter((it) => it.due_at && it.due_at < from)
      .sort((a, b) => (a.due_at ?? "").localeCompare(b.due_at ?? ""));
  });
  const today = $derived.by(() => {
    const from = todayStartIso();
    const to = todayEndIso();
    return items
      .filter((it) => it.due_at && it.due_at >= from && it.due_at <= to)
      .sort((a, b) => (a.due_at ?? "").localeCompare(b.due_at ?? ""));
  });
  const undated = $derived(items.filter((it) => !it.due_at && !it.start_at));
  const total = $derived(overdue.length + today.length + undated.length);

  const hm = (iso: string | null) =>
    iso
      ? new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false })
      : "";
  const overdueDay = (iso: string | null) =>
    iso
      ? new Date(iso).toLocaleDateString([], { month: "numeric", day: "numeric" })
      : "";

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
{:else if total === 0}
  <p class="empty">{t("mobile.tasks_empty")}</p>
{:else}
  {#if overdue.length}
    <h3 class="grp grp-warn">{t("mobile.tasks_overdue")} · {overdue.length}</h3>
    {#each overdue as it (it.id)}
      <div class="m-row">
        <input
          class="m-switch"
          type="checkbox"
          checked={false}
          disabled={busyId === it.id}
          onchange={() => toggle(it)}
          aria-label={t("common.done")}
        />
        <button class="m-row-main m-rowbtn" onclick={() => onopen(it)}>
          <span class="m-row-title">{displayTitle(it)}</span>
          <span class="m-row-sub warn">{overdueDay(it.due_at)} {hm(it.due_at)}</span>
        </button>
      </div>
    {/each}
  {/if}

  {#if today.length}
    <h3 class="grp">{t("mobile.tasks_today")} · {today.length}</h3>
    {#each today as it (it.id)}
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
          <span class="m-row-sub">{it.due_all_day ? t("calendar.all_day") : hm(it.due_at)}</span>
        </button>
      </div>
    {/each}
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
  .grp-warn {
    color: var(--danger);
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
  .m-row-sub.warn {
    color: var(--danger);
  }
</style>

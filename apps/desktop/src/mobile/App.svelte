<script lang="ts">
  /**
   * 移动端根组件：日历月视图为主页 + FAB 快速添加 + 回收站（顶栏入口）。
   * 数据 = 可见月网格窗口的 list_items_window → expandItems（重复展开，
   * 虚拟实例与桌面同口径）→ 按天分组。data-changed / reminder-fired 触发重载。
   */
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "../lib/i18n";
  import { api, type Item } from "../lib/api";
  import { expandItems } from "../lib/recurrence";
  import CalendarMonth from "./CalendarMonth.svelte";
  import DaySheet from "./DaySheet.svelte";
  import ItemDetail from "./ItemDetail.svelte";
  import QuickAdd from "./QuickAdd.svelte";
  import Trash from "./Trash.svelte";
  import SyncSheet from "./SyncSheet.svelte";

  const pad2 = (n: number) => String(n).padStart(2, "0");

  let view = $state<"calendar" | "trash">("calendar");
  let year = $state(new Date().getFullYear());
  let month = $state(new Date().getMonth()); // 0 起
  let byDay = $state<Map<string, Item[]>>(new Map());
  let daySel = $state<string | null>(null);
  let detail = $state<Item | null>(null);
  let quickFor = $state<{ open: boolean; day: string | null }>({ open: false, day: null });
  let syncOpen = $state(false);

  const monthLabel = $derived(
    new Date(year, month, 1).toLocaleDateString(t("common.today") === "今天" ? "zh-CN" : "en-US", {
      year: "numeric",
      month: "long",
    }),
  );

  /** 月网格覆盖的 [周一首格, 周日末格] 窗口（数据拉取 + 重复展开共用） */
  function windowRange(): { from: Date; to: Date } {
    const first = new Date(year, month, 1);
    const from = new Date(first);
    from.setDate(from.getDate() - ((first.getDay() + 6) % 7));
    const to = new Date(from);
    to.setDate(from.getDate() + 41);
    to.setHours(23, 59, 59, 999);
    return { from, to };
  }

  /** 条目归属日（镜像桌面日历口径：日程跨天两侧都算，待办=开始+截止，记录=发生） */
  function relatedDays(it: Item, into: Map<string, Item[]>) {
    const push = (d: Date) => {
      const key = `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
      if (!into.has(key)) into.set(key, []);
      into.get(key)!.push(it);
    };
    if (it.deleted_at) return;
    if (it.type === "log") {
      if (it.occurred_at) push(new Date(it.occurred_at));
    } else if (it.type === "task") {
      if (it.start_at) push(new Date(it.start_at));
      if (it.due_at) push(new Date(it.due_at));
    } else if (it.start_at) {
      const start = new Date(it.start_at);
      const end = it.end_at ? new Date(it.end_at) : start;
      for (let d = new Date(start); d <= end; d.setDate(d.getDate() + 1)) {
        push(d);
        if (into.size > 400) break; // 防御：超长跨天事件
      }
    }
  }

  async function reload() {
    const { from, to } = windowRange();
    try {
      const raw = await api.listItemsWindow(from.toISOString(), to.toISOString());
      const expanded = expandItems(raw, from, to);
      const map = new Map<string, Item[]>();
      for (const it of expanded) relatedDays(it, map);
      byDay = map;
    } catch (e) {
      console.error("myday mobile: 加载失败", e);
    }
  }

  function shiftMonth(delta: number) {
    const m = month + delta;
    year += Math.floor(m / 12);
    month = ((m % 12) + 12) % 12;
  }
  function goToday() {
    const d = new Date();
    year = d.getFullYear();
    month = d.getMonth();
    daySel = `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
  }

  $effect(() => {
    // year/month 变化即重载（daySel 变化不触发）
    void year;
    void month;
    void reload();
  });

  onMount(() => {
    const unlisteners = [
      listen("data-changed", () => void reload()),
      listen("reminder-fired", () => void reload()),
    ];
    return () => {
      for (const u of unlisteners) u.then((f) => f());
    };
  });

  // ---- Android 返回键：有弹层先关最上层,而非退出应用 ----
  // 每开一层压一个 history 栈位;系统返回触发 popstate → 关最上层。
  // UI 自己关层时用 history.back() 弹栈,popstate 借 pendingPop 区分,防二次误关。
  let pushed = 0;
  let pendingPop = 0;
  const layers = $derived(
    (view === "trash" ? 1 : 0) +
      (daySel !== null ? 1 : 0) +
      (detail ? 1 : 0) +
      (quickFor.open ? 1 : 0) +
      (syncOpen ? 1 : 0),
  );

  $effect(() => {
    const n = layers;
    if (n > pushed) {
      while (pushed < n) history.pushState({ myday: pushed++ }, "");
    } else if (n < pushed) {
      const delta = pushed - n;
      pendingPop += delta;
      for (let i = 0; i < delta; i++) history.back();
    }
  });

  function closeTopLayer() {
    if (detail) detail = null;
    else if (quickFor.open) quickFor = { open: false, day: null };
    else if (syncOpen) syncOpen = false;
    else if (daySel !== null) daySel = null;
    else if (view === "trash") view = "calendar";
  }

  onMount(() => {
    const onPop = () => {
      if (pendingPop > 0) {
        pendingPop--;
        pushed--;
        return;
      }
      if (pushed > 0) {
        pushed--;
        closeTopLayer();
      }
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  });
</script>

{#if view === "calendar"}
  <header class="m-header">
    <button class="m-iconbtn" aria-label={t("calendar.prev_month")} onclick={() => shiftMonth(-1)}>‹</button>
    <h1 class="m-title">{monthLabel}</h1>
    <button class="m-iconbtn" aria-label={t("calendar.next_month")} onclick={() => shiftMonth(1)}>›</button>
    <button class="m-today" onclick={goToday}>{t("common.today")}</button>
    <button class="m-iconbtn" aria-label={t("mobile.sync_title")} onclick={() => (syncOpen = true)}>⇄</button>
    <button class="m-iconbtn" aria-label={t("trash.title")} onclick={() => (view = "trash")}>🗑</button>
  </header>

  <CalendarMonth {year} {month} byDay={byDay} selected={daySel} onselect={(k) => (daySel = k)} />

  <button class="m-fab" aria-label={t("mobile.quickadd_title")} onclick={() => (quickFor = { open: true, day: daySel })}>
    +
  </button>
{:else}
  <header class="m-header">
    <button class="m-iconbtn" onclick={() => (view = "calendar")}>‹</button>
    <h1 class="m-title">{t("trash.title")}</h1>
  </header>
  <div class="trashwrap">
    <Trash onchange={() => void reload()} />
  </div>
{/if}

{#if daySel !== null && !detail && !quickFor.open}
  <DaySheet
    dayKey={daySel}
    items={byDay.get(daySel) ?? []}
    onopen={(it) => (detail = it)}
    onclose={() => (daySel = null)}
    onadd={() => (quickFor = { open: true, day: daySel })}
  />
{/if}

{#if detail}
  <ItemDetail
    item={detail}
    onclose={() => (detail = null)}
    onchange={() => {
      void reload();
      detail = null;
    }}
  />
{/if}

{#if syncOpen}
  <SyncSheet onclose={() => (syncOpen = false)} />
{/if}

{#if quickFor.open}
  <QuickAdd
    defaultDay={quickFor.day}
    onclose={() => (quickFor = { open: false, day: null })}
    onsaved={() => void reload()}
  />
{/if}

<style>
  .trashwrap {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 14px calc(24px + env(safe-area-inset-bottom));
  }
</style>

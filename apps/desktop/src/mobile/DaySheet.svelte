<script lang="ts">
  /** 日议程弹层：点月格某天后的当日列表，按锚点时间排序；点条目进只读详情。 */
  import { t } from "../lib/i18n";
  import { displayTitle, type Item } from "../lib/api";
  // 响应式包装：节假日数据异步装载完成后标题自动更新
  import { holidayOfKey } from "../lib/holidays.svelte";

  let {
    dayKey,
    items,
    onopen,
    onclose,
    onadd,
  }: {
    dayKey: string;
    items: Item[];
    onopen: (item: Item) => void;
    onclose: () => void;
    onadd: () => void;
  } = $props();

  const hm = (iso: string | null) =>
    iso ? new Date(iso).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false }) : "";

  function anchor(it: Item): number {
    const iso = it.type === "log" ? it.occurred_at : it.type === "task" ? (it.due_at ?? it.start_at) : it.start_at;
    return iso ? Date.parse(iso) : Number.MAX_SAFE_INTEGER;
  }

  function timeText(it: Item): string {
    if (it.type === "log") return hm(it.occurred_at);
    if (it.type === "task") return it.due_all_day ? t("calendar.all_day") : hm(it.due_at);
    if (it.all_day) return t("calendar.all_day");
    return `${hm(it.start_at)}–${hm(it.end_at)}`;
  }

  function dot(it: Item): string {
    return it.type === "event" ? "var(--type-event)" : it.type === "task" ? "var(--type-task)" : "var(--type-log)";
  }

  const sorted = $derived([...items].sort((a, b) => anchor(a) - anchor(b)));
  const headText = $derived.by(() => {
    const [y, m, d] = dayKey.split("-").map(Number);
    const dt = new Date(y, m - 1, d);
    const date = dt.toLocaleDateString(t("common.today") === "今天" ? "zh-CN" : "en-US", {
      month: "long",
      day: "numeric",
      weekday: "short",
    });
    const hol = holidayOfKey(dayKey);
    return hol ? `${date} · ${hol.name}` : date;
  });
</script>

<div class="m-backdrop" onclick={onclose} aria-hidden="true"></div>
<div class="m-sheet" role="dialog" aria-label={headText}>
  <div class="m-grip"></div>
  <div class="m-sheet-head">
    <h2 class="m-sheet-title">{headText}</h2>
    <button class="m-today" onclick={onadd}>＋</button>
    <button class="m-iconbtn" onclick={onclose}>✕</button>
  </div>
  <div class="m-sheet-body">
    {#if sorted.length === 0}
      <p class="empty">{t("mobile.empty_day")}</p>
    {:else}
      {#each sorted as it (it.id + (it.start_at ?? ""))}
        <button class="m-row" class:m-done={it.type === "task" && it.status === "done"} onclick={() => onopen(it)}>
          <span class="m-dot" style="background: {dot(it)}"></span>
          <span class="m-row-main">
            <span class="m-row-title">{displayTitle(it)}</span>
            {#if it.type === "task" && it.status === "done"}
              <span class="m-row-sub">✓ {t("common.done")}</span>
            {/if}
          </span>
          <span class="m-row-time">{timeText(it)}</span>
        </button>
      {/each}
    {/if}
  </div>
</div>

<style>
  .empty {
    text-align: center;
    color: var(--text-dim);
    font-size: 14px;
    padding: 28px 0;
  }
</style>

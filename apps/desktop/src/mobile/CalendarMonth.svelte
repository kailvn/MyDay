<script lang="ts">
  /**
   * 移动端月网格（Outlook 式）：6×7 周一起始，细线分隔、行高均分撑满剩余视口；
   * 格内日号（首日带月份）+ 休/班角标 + 节假日暖色 chip + 至多四条类型色块
   * （超出折叠 +n）。纯展示，数据由 App.svelte 按天分组喂入；节假日 / 休班
   * 口径复用 lib/holidays（与桌面日历一致）。
   */
  import { t } from "../lib/i18n";
  // 响应式包装：数据异步装载完成后自动重渲（纯模块不触发更新）
  import { holidayOfKey } from "../lib/holidays.svelte";
  import { displayTitle, type Item } from "../lib/api";

  let {
    year,
    month, // 0 起
    byDay,
    selected,
    onselect,
  }: {
    year: number;
    month: number;
    byDay: Map<string, Item[]>;
    selected: string | null;
    onselect: (key: string) => void;
  } = $props();

  interface Cell {
    key: string;
    day: number;
    adj: boolean; // 非本月
  }

  const pad2 = (n: number) => String(n).padStart(2, "0");
  const todayKey = $derived(
    (() => {
      const d = new Date();
      return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
    })(),
  );

  // 周一起始（与桌面日历同口径）：((getDay()+6)%7) 回退到本周一
  const cells = $derived.by(() => {
    const first = new Date(year, month, 1);
    const start = new Date(first);
    start.setDate(start.getDate() - ((first.getDay() + 6) % 7));
    const out: Cell[] = [];
    for (let i = 0; i < 42; i++) {
      const d = new Date(start);
      d.setDate(start.getDate() + i);
      out.push({
        key: `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`,
        day: d.getDate(),
        adj: d.getMonth() !== month,
      });
    }
    return out;
  });

  const weekdays = $derived.by(() => {
    // 2023-01-02 恰是周一：拿一周的名字，随 locale
    const names = [...Array(7)].map((_, i) => {
      const d = new Date(2023, 0, 2 + i);
      return d.toLocaleDateString(t("common.today") === "今天" ? "zh-CN" : "en-US", {
        weekday: "short",
      });
    });
    return names;
  });

  /** 日号：每月首日带月份（Outlook 口径：9月1日 / Sep 1），其余只显日号；
   *  月份取格子自身（相邻月的 1 号随真实月份，不随显示月） */
  function dayLabel(c: Cell): string {
    if (t("common.today") === "今天") {
      const m = Number(c.key.slice(5, 7));
      return c.day === 1 ? `${m}月${c.day}日` : `${c.day}日`;
    }
    const d = new Date(c.key + "T00:00:00");
    return c.day === 1
      ? new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" }).format(d)
      : String(c.day);
  }

  function typeColor(it: Item): string {
    return it.type === "event" ? "var(--m-event)" : it.type === "task" ? "var(--m-task)" : "var(--m-log)";
  }

  /** 格内色块文案：格宽只有 1/7 屏，时间前缀会把标题挤没——直接显示标题，
   *  具体时刻在日议程（DaySheet）里看 */
  function chipLabel(it: Item): string {
    return displayTitle(it);
  }

  function chips(key: string): Item[] {
    const list = byDay.get(key) ?? [];
    // 已完成待办沉底（镜像桌面口径）
    return [...list].sort((a, b) => {
      const ad = a.type === "task" && a.status === "done" ? 1 : 0;
      const bd = b.type === "task" && b.status === "done" ? 1 : 0;
      return ad - bd;
    });
  }
</script>

<div class="weekhead" aria-hidden="true">
  {#each weekdays as w}
    <span>{w}</span>
  {/each}
</div>

<div class="grid" role="grid">
  {#each cells as c (c.key)}
    {@const hol = holidayOfKey(c.key)}
    {@const list = chips(c.key)}
    <button
      class="cell"
      class:adj={c.adj}
      class:sel={c.key === selected}
      class:today={c.key === todayKey}
      onclick={() => onselect(c.key)}
      role="gridcell"
    >
      <span class="daynum">
        {dayLabel(c)}
        {#if hol}
          <i class="holbadge" class:off={hol.off}>{hol.off ? t("calendar.hol_off") : t("calendar.hol_work")}</i>
        {/if}
      </span>
      {#if hol}
        <span class="holname">{hol.name}</span>
      {/if}
      {#each list.slice(0, 4) as it (it.id + it.start_at)}
        <span class="chip" style="--c: {typeColor(it)}">{chipLabel(it)}</span>
      {/each}
      {#if list.length > 4}
        <span class="more">+{list.length - 4}</span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .weekhead {
    flex: none;
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    text-align: center;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-dim);
    padding: 6px 0;
  }
  /* 网格撑满剩余视口：6 行 1fr 均分；1px 缝隙 + 底色 = 细分隔线（Outlook 式线框） */
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    grid-template-rows: repeat(6, minmax(0, 1fr));
    gap: 1px;
    background: var(--border);
    border-top: 1px solid var(--border);
  }
  .cell {
    appearance: none;
    border: none;
    background: var(--bg);
    border-radius: 0;
    min-height: 0;
    min-width: 0;
    padding: 3px 4px;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 3px;
    overflow: hidden;
    font: inherit;
    color: var(--text);
    text-align: left;
  }
  .cell.adj {
    opacity: 0.4;
  }
  .cell.sel {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .cell.today {
    background: color-mix(in srgb, var(--accent) 10%, var(--bg));
  }
  .cell.today .daynum {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .daynum {
    font-size: 12px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 3px;
    align-self: flex-start;
    padding: 0 5px;
    border-radius: 9px;
    line-height: 1.5;
    white-space: nowrap;
  }
  .holbadge {
    font-style: normal;
    font-size: 9px;
    font-weight: 400;
    color: #d05656;
  }
  .holbadge.off {
    color: #2e9e5b;
  }
  .holname {
    align-self: flex-start;
    max-width: 100%;
    font-size: 10px;
    line-height: 1.5;
    padding: 0 5px;
    border-radius: 4px;
    color: var(--m-hol);
    background: color-mix(in srgb, var(--m-hol) 16%, transparent);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chip {
    font-size: 11px;
    line-height: 1.35;
    padding: 1px 5px;
    border-radius: 4px;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 20%, var(--bg));
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .more {
    font-size: 10px;
    color: var(--text-dim);
    padding-left: 3px;
  }
</style>

<script module lang="ts">
  const pad = (n: number) => String(n).padStart(2, "0");

  /** 打开中的 TimePopover 数量：宿主面板的 Esc 处理据此让位（只关弹层不关面板） */
  export const timePopoverOpen = { count: 0 };
</script>

<script lang="ts">
  /**
   * 时间选择器面板（需求 §3.2 五行布局 / SPRINT-SPEC §5）：
   * 日期快捷 → 迷你月历 → 时刻快捷（含最近使用）→ 小时×分钟网格。
   * - 点时刻即应用即关；点日期只改日期（时间保持默认），面板不关
   * - 方向键在时间网格移动，Enter 应用，Esc 关闭，点击外部关闭
   * - variant 决定快捷行与无值时的默认钟点：
   *   start/end → 今天/明天/后天/下周一（默认 09:00）
   *   due       → 今天/明天/本周五（默认 23:59）
   *   occurred  → 现在/5分钟前/30分钟前（不允许未来）
   * - 最近使用：localStorage `myday.recent_times` 存 HH:mm 取前 3
   */
  import { t, type MessageKey } from "./i18n";

  const pad = (n: number) => String(n).padStart(2, "0");
  const DAY_MS = 86_400_000;

  export type TimeVariant = "start" | "end" | "due" | "occurred";

  let {
    value = "",
    variant = "start",
    onchange,
  }: {
    value?: string;
    variant?: TimeVariant;
    onchange?: (v: string) => void;
  } = $props();

  let open = $state(false);
  let root: HTMLElement | null = $state(null);

  // 向宿主登记开关状态：Esc 只关最上层的弹层，不误杀整个创建面板
  $effect(() => {
    if (open) timePopoverOpen.count += 1;
    return () => {
      if (open) timePopoverOpen.count -= 1;
    };
  });

  const PLACEHOLDER: Record<TimeVariant, MessageKey> = {
    start: "timepop.ph_start",
    end: "timepop.ph_end",
    due: "timepop.ph_due",
    occurred: "timepop.ph_occurred",
  };

  /** 值串（YYYY-MM-DDTHH:mm）→ 显示 */
  function label(v: string): string {
    if (!v) return t(PLACEHOLDER[variant]);
    return v.replace("T", " ");
  }
  function parse(v: string): Date | null {
    if (!v) return null;
    const d = new Date(v);
    return isNaN(d.getTime()) ? null : d;
  }

  function emit(d: Date | null) {
    onchange?.(d ? `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}` : "");
  }

  // ---- 快捷行 --------------------------------------------------------------
  const DATE_QUICKS: { label: MessageKey; days: number }[] = [
    { label: "common.today", days: 0 },
    { label: "timepop.tomorrow", days: 1 },
    { label: "timepop.day_after", days: 2 },
  ];
  /** 迷你月历表头（周一 → 周日） */
  const WEEKDAYS: MessageKey[] = [
    "timepop.wd1",
    "timepop.wd2",
    "timepop.wd3",
    "timepop.wd4",
    "timepop.wd5",
    "timepop.wd6",
    "timepop.wd7",
  ];
  function defaultClock(): { h: number; m: number } {
    if (variant === "due") return { h: 23, m: 59 };
    if (variant === "occurred") return { h: new Date().getHours(), m: new Date().getMinutes() };
    return { h: 9, m: 0 };
  }
  function applyDate(daysFromToday: number, weekday?: number) {
    const cur = parse(value);
    const clock = cur ? { h: cur.getHours(), m: cur.getMinutes() } : defaultClock();
    const base = new Date();
    if (weekday !== undefined) {
      // 下一个周X（严格晚于今天）
      const diff = (weekday - base.getDay() + 7) % 7 || 7;
      base.setDate(base.getDate() + diff);
    } else {
      base.setDate(base.getDate() + daysFromToday);
    }
    base.setHours(clock.h, clock.m, 0, 0);
    emit(base);
  }
  function applyClock(h: number, m: number) {
    const cur = parse(value) ?? new Date();
    const d = new Date(cur.getFullYear(), cur.getMonth(), cur.getDate(), h, m, 0, 0);
    remember(`${pad(h)}:${pad(m)}`);
    emit(d);
    open = false;
  }
  function applyOccOffset(mins: number) {
    const d = new Date(Date.now() - mins * 60_000);
    d.setSeconds(0, 0);
    emit(d);
    open = false;
  }
  function quicks(): { label: string; run: () => void }[] {
    if (variant === "occurred") {
      return [
        { label: t("timepop.now"), run: () => applyOccOffset(0) },
        { label: t("timepop.min5_ago"), run: () => applyOccOffset(5) },
        { label: t("timepop.min30_ago"), run: () => applyOccOffset(30) },
      ];
    }
    if (variant === "due") {
      return [
        { label: t("common.today"), run: () => applyDate(0) },
        { label: t("timepop.tomorrow"), run: () => applyDate(1) },
        { label: t("timepop.friday"), run: () => applyDate(0, 5) },
      ];
    }
    return [
      ...DATE_QUICKS.map((q) => ({ label: t(q.label), run: () => applyDate(q.days) })),
      { label: t("timepop.next_monday"), run: () => applyDate(0, 1) },
    ];
  }

  // ---- 最近使用 ------------------------------------------------------------
  let recents = $state<string[]>(loadRecents());
  function loadRecents(): string[] {
    try {
      return JSON.parse(localStorage.getItem("myday.recent_times") ?? "[]");
    } catch {
      return [];
    }
  }
  function remember(hm: string) {
    recents = [hm, ...recents.filter((x) => x !== hm)].slice(0, 3);
    try {
      localStorage.setItem("myday.recent_times", JSON.stringify(recents));
    } catch {
      /* 隐私模式等场景忽略 */
    }
  }

  // ---- 迷你月历 ------------------------------------------------------------
  let calMonth = $state(new Date());
  function openPanel() {
    open = true;
    const cur = parse(value);
    calMonth = new Date(cur ?? new Date());
    cursor = { hour: (cur ?? defaultDate()).getHours(), minIdx: [0, 30].includes((cur ?? defaultDate()).getMinutes()) ? (cur ?? defaultDate()).getMinutes() / 30 : 1 };
  }
  function defaultDate(): Date {
    const c = defaultClock();
    const d = new Date();
    d.setHours(c.h, c.m, 0, 0);
    return d;
  }
  function shiftMonth(delta: number) {
    calMonth = new Date(calMonth.getFullYear(), calMonth.getMonth() + delta, 1);
  }
  function calGrid(): (number | null)[] {
    const y = calMonth.getFullYear();
    const m = calMonth.getMonth();
    const offset = (new Date(y, m, 1).getDay() + 6) % 7;
    const cells: (number | null)[] = Array(offset).fill(null);
    for (let d = 1; d <= new Date(y, m + 1, 0).getDate(); d++) cells.push(d);
    return cells;
  }
  function pickDay(day: number) {
    // 只改日期，时间保持已填/默认（面板不关）
    const cur = parse(value);
    const clock = cur ? { h: cur.getHours(), m: cur.getMinutes() } : defaultClock();
    emit(new Date(calMonth.getFullYear(), calMonth.getMonth(), day, clock.h, clock.m, 0, 0));
  }

  // ---- 时间网格 + 键盘 ------------------------------------------------------
  const MINUTES = [0, 15, 30, 45];
  let cursor = $state({ hour: new Date().getHours(), minIdx: 1 });
  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "Enter") {
      e.preventDefault();
      applyClock(cursor.hour, MINUTES[cursor.minIdx]);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      cursor = { hour: (cursor.hour + 1) % 24, minIdx: cursor.minIdx };
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      cursor = { hour: (cursor.hour + 23) % 24, minIdx: cursor.minIdx };
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      cursor = { hour: (cursor.hour + 6) % 24, minIdx: (cursor.minIdx + 1) % 4 };
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      cursor = { hour: (cursor.hour + 18) % 24, minIdx: (cursor.minIdx + 3) % 4 };
    }
  }

  // 点击外部关闭
  $effect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  });
</script>

<svelte:window onkeydown={onKey} />

<span class="tp" bind:this={root}>
  <button class="tp-btn" class:set={!!value} onclick={openPanel}>
    {label(value)}
  </button>
  {#if open}
    <div class="panel">
      <div class="row">
        {#each quicks() as q (q.label)}
          <button class="chip" onclick={q.run}>{q.label}</button>
        {/each}
        {#if variant === "occurred"}
          <button class="chip" onclick={() => { emit(null); open = false; }}>{t("timepop.clear")}</button>
        {/if}
      </div>

      <div class="cal">
        <div class="cal-bar">
          <button class="ghost" onclick={() => shiftMonth(-1)}>‹</button>
          <span>{t("timepop.month_ym", { y: calMonth.getFullYear(), m: calMonth.getMonth() + 1 })}</span>
          <button class="ghost" onclick={() => shiftMonth(1)}>›</button>
        </div>
        <div class="cal-grid">
          {#each WEEKDAYS as w (w)}
            <span class="cal-w">{t(w)}</span>
          {/each}
          {#each calGrid() as day, i (i)}
            {#if day === null}
              <span></span>
            {:else}
              <button
                class="cal-day"
                class:picked={value.startsWith(`${calMonth.getFullYear()}-${pad(calMonth.getMonth() + 1)}-${pad(day)}`)}
                onclick={() => pickDay(day)}
              >
                {day}
              </button>
            {/if}
          {/each}
        </div>
      </div>

      <div class="row">
        {#if variant === "occurred"}
          <span class="dim">{t("timepop.past_only")}</span>
        {:else}
          {#each ["09:00", "12:00", "14:00", "18:00", "20:00"] as tm (tm)}
            <button class="chip" onclick={() => applyClock(Number(tm.slice(0, 2)), Number(tm.slice(3)))}>{tm}</button>
          {/each}
        {/if}
        {#if variant !== "occurred" && recents.length}
          <span class="dim">{t("timepop.recent")}</span>
          {#each recents as r (r)}
            <button class="chip" onclick={() => applyClock(Number(r.slice(0, 2)), Number(r.slice(3)))}>{r}</button>
          {/each}
        {/if}
      </div>

      {#if variant !== "occurred"}
        <div class="grid">
          {#each Array(24) as _, h (h)}
            {#each MINUTES as m, mi (m)}
              <button
                class="cell"
                class:cur={cursor.hour === h && cursor.minIdx === mi}
                onclick={() => applyClock(h, m)}
              >
                {pad(h)}:{pad(m)}
              </button>
            {/each}
          {/each}
        </div>
        <p class="dim keys">{t("timepop.keys")}</p>
      {/if}
    </div>
  {/if}
</span>

<style>
  .tp {
    position: relative;
    display: inline-flex;
  }

  .tp-btn {
    min-width: 150px;
    text-align: left;
    font-variant-numeric: tabular-nums;
    color: var(--text-dim);
  }

  .tp-btn.set {
    color: var(--text);
  }

  .panel {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 60;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 34px rgb(0 0 0 / 0.3);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 320px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
  }

  .chip {
    border-radius: 999px;
    font-size: 12px;
    padding: 2px 10px;
  }

  .cal-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: var(--text-dim);
  }

  .cal-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 2px;
  }

  .cal-w {
    text-align: center;
    font-size: 10px;
    color: var(--text-dim);
  }

  .cal-day {
    border: none;
    background: transparent;
    border-radius: 6px;
    font-size: 12px;
    padding: 3px 0;
  }

  .cal-day:hover {
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }

  .cal-day.picked {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 2px;
    max-height: 190px;
    overflow-y: auto;
  }

  .cell {
    border: none;
    background: transparent;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    border-radius: 6px;
    padding: 3px 0;
    color: var(--text);
  }

  .cell:hover {
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }

  .cell.cur {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .dim {
    color: var(--text-dim);
    font-size: 11px;
  }

  .keys {
    margin: 0;
  }
</style>

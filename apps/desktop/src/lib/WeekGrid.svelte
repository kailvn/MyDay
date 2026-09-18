<script lang="ts">
  /**
   * 周 / 日时间网格（SPRINT-SPEC §3.1 + SPRINT2-SPEC §2/§4）：
   * 7 列（或 1 列）× 24 小时行。
   * - 事件块按本地时刻绝对定位；同日重叠贪心分列并排（冲突天然可见）；
   *   跨天日程在每一天画出窗口内的可见段（跨天段不参与拖拽）
   * - all_day 进顶部「全天」行；到期待办进「到期」行（due 主语义是日）
   * - 当前时刻红线（分钟精度）；挂载时滚到 now-2h
   * - 单击空白 = 选中时段；双击空白 = 创建（锚点日 + 该小时整点，+1h）；
   *   **拖拽**：块主体移动（15 分钟吸附、可跨列）、上下边缘调时长、
   *   空白处按下拖动 ≥15 分钟松开直达创建（首尾预填）；Esc 取消；
   *   松手才写库，toast 可撤销；重复块拖拽 = 改整个系列（规则智能改写）
   * - 列头显示节假日 休/班 角标（holidays.ts，数据年外自动隐藏）
   */
  import { onMount } from "svelte";
  import { api, displayTitle, fmtTime, toDateInput, typeLabel, type Item } from "./api";
  import { expandItems } from "./recurrence";
  import { deletions } from "./deletion.svelte";
  import { openCreate, openDetail } from "./panel.svelte";
  import { toast } from "./toast.svelte";
  import { holidayOfKey } from "./holidays.svelte";
  import {
    moveEventOccurrence,
    resizeEventOccurrence,
    snapMinutes,
    SNAP_MIN,
    type RescheduleResult,
  } from "./reschedule";
  import { t, i18n } from "./i18n";

  let { dataVersion = 0, days = 7 }: { dataVersion?: number; days?: number } = $props();

  const HOUR_H = 44;
  /** 星期 / 月份名随 locale（规则：走 Intl，不建 key）；星期一为一周开始（2023-01-02 恰是周一） */
  const localeTag = () => (i18n.locale === "en" ? "en-US" : "zh-CN");
  let weekLabels = $derived(
    Array.from({ length: 7 }, (_, i) => new Date(2023, 0, 2 + i).toLocaleDateString(localeTag(), { weekday: "short" })),
  );

  /** 锚点：显示区间从锚点所在周一开始（days=1 时即锚点当天）；
   *  切页回来原位恢复（module memo），不弹回当周 */
  let anchor = $state(weekMemo?.anchor ?? new Date());
  let items = $state<Item[]>([]);
  let error = $state("");
  let selected: { day: Date; hour: number } | null = $state(null);
  let now = $state(new Date());
  let scroller: HTMLElement | null = $state(null);
  let gridEl: HTMLElement | null = $state(null);

  let weekStart = $derived.by(() => {
    const d = new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate());
    if (days === 1) return d; // 日视图：锚点日当天，不折算到周一
    d.setDate(d.getDate() - ((d.getDay() + 6) % 7)); // 周一为一周开始
    return d;
  });
  let days_ = $derived(
    Array.from({ length: days }, (_, i) => {
      const d = new Date(weekStart);
      d.setDate(d.getDate() + i);
      return d;
    }),
  );
  let windowStart = $derived(new Date(weekStart.getFullYear(), weekStart.getMonth(), weekStart.getDate()));
  let windowEnd = $derived(new Date(weekStart.getFullYear(), weekStart.getMonth(), weekStart.getDate() + days, 0, 0, 0, -1));
  let todayKey = $derived(toDateInput(new Date()));

  let displayItems = $derived(expandItems(items, windowStart, windowEnd));
  let visible = $derived(displayItems.filter((i) => !deletions.pendingIds.includes(i.id)));

  function dayKey(d: Date): string {
    return toDateInput(d);
  }
  /** 当天相交的日程（含跨天的可见段） */
  function eventsOn(d: Date): Item[] {
    const from = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const to = new Date(from.getTime() + 86_400_000);
    return visible
      .filter((it) => {
        if (it.type !== "event" || !it.start_at) return false;
        const s = new Date(it.start_at);
        const e = it.end_at ? new Date(it.end_at) : s;
        return s < to && e >= from;
      })
      .sort((a, b) => Date.parse(a.start_at!) - Date.parse(b.start_at!));
  }
  function allDayOn(d: Date): Item[] {
    return eventsOn(d).filter((it) => it.all_day);
  }
  function gridOn(d: Date): Item[] {
    return eventsOn(d).filter((it) => !it.all_day);
  }
  function duesOn(d: Date): Item[] {
    const key = dayKey(d);
    return visible.filter(
      (it) => it.type === "task" && it.due_at && dayKey(new Date(it.due_at)) === key && it.status !== "done",
    );
  }

  /** 可见段几何：返回相对当天 00:00 的起止分钟 */
  function segment(it: Item, d: Date): { top: number; height: number } {
    const from = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
    const s = Math.max(Date.parse(it.start_at!), from);
    const e = Math.min(it.end_at ? Date.parse(it.end_at) : s, from + 86_400_000);
    const top = ((s - from) / 3_600_000) * HOUR_H;
    const height = Math.max(((e - s) / 3_600_000) * HOUR_H, 20);
    return { top, height };
  }

  /** 同日重叠分列：重叠簇内贪心占列、按簇内列数均分宽度；不重叠的簇占满整列 */
  function layout(dayItems: Item[], d: Date): { col: number; cols: number }[] {
    const segs = dayItems
      .map((it, idx) => ({ idx, ...segment(it, d) }))
      .sort((a, b) => a.top - b.top);
    const out: { col: number; cols: number }[] = new Array(segs.length);
    let cluster: typeof segs = [];
    let clusterEnd = -1;
    const flush = () => {
      if (!cluster.length) return;
      const colEnds: number[] = [];
      const colOf: number[] = [];
      for (const s of cluster) {
        let c = colEnds.findIndex((end) => end <= s.top);
        if (c === -1) {
          c = colEnds.length;
          colEnds.push(0);
        }
        colEnds[c] = s.top + s.height;
        colOf.push(c);
      }
      cluster.forEach((s, i) => (out[s.idx] = { col: colOf[i], cols: colEnds.length }));
      cluster = [];
      clusterEnd = -1;
    };
    for (const s of segs) {
      if (cluster.length && s.top >= clusterEnd) flush();
      cluster.push(s);
      clusterEnd = Math.max(clusterEnd, s.top + s.height);
    }
    flush();
    return out;
  }

  function shift(deltaDays: number) {
    const d = new Date(anchor);
    d.setDate(d.getDate() + deltaDays);
    anchor = d;
    selected = null;
  }
  function goToday() {
    anchor = new Date();
    selected = null;
  }
  function clickSlot(d: Date, hour: number) {
    if (Date.now() < suppressClickUntil) return;
    selected = { day: d, hour };
  }
  function dblclickSlot(d: Date, hour: number) {
    if (Date.now() < suppressClickUntil) return;
    const start = new Date(d.getFullYear(), d.getMonth(), d.getDate(), hour);
    openCreate({ anchorDay: dayKey(d), presetStart: start.toISOString() });
  }
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (drag) {
        drag = null; // 拖拽中 Esc = 放弃，不写库
        return;
      }
      selected = null;
      return;
    }
    // Notion Calendar 式快捷键：T 回今天、←/→ 翻页（输入控件聚焦时不劫持）
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.key.toLowerCase() === "t") {
      e.preventDefault();
      goToday();
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      shift(-days);
    } else if (e.key === "ArrowRight") {
      e.preventDefault();
      shift(days);
    }
  }

  let rangeLabel = $derived(
    days === 1
      ? `${anchor.toLocaleDateString(localeTag(), { month: "long", day: "numeric" })} · ${weekLabels[(anchor.getDay() + 6) % 7]}`
      : `${days_[0].toLocaleDateString(localeTag(), { month: "numeric", day: "numeric" })} – ${days_[days - 1].toLocaleDateString(localeTag(), { month: "numeric", day: "numeric" })}`,
  );

  async function load() {
    try {
      items = await api.listItemsWindow(windowStart.toISOString(), windowEnd.toISOString());
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    dataVersion;
    weekStart;
    load();
  });

  onMount(() => {
    const timer = setInterval(() => (now = new Date()), 30_000);
    return () => clearInterval(timer);
  });
  $effect(() => {
    if (scroller) {
      const h = now.getHours();
      scroller.scrollTop = Math.max(0, h - 2) * HOUR_H;
    }
  });
  $effect(() => {
    weekMemo = { anchor };
  });

  /** 当前时刻红线位置（仅当天列；days=1 恒显） */
  function nowLineFor(d: Date): number | null {
    if (dayKey(d) !== dayKey(now)) return null;
    return (now.getHours() + now.getMinutes() / 60) * HOUR_H;
  }

  // ---- 拖拽（SPRINT2-SPEC §2）---------------------------------------------
  interface EventDrag {
    kind: "move" | "resize-start" | "resize-end";
    /** 系列原条目（items 中按 id 找，时间为系列锚点而非本次发生） */
    base: Item;
    /** 展开后的本次发生（时间为 occurrence） */
    ev: Item;
    /** 原始 occurrence 的当日分钟 */
    occStartMin: number;
    occEndMin: number;
    grabMin: number; // 仅 move：按住点相对块顶的分钟偏移
    dayIdx: number;
    startMin: number;
    endMin: number;
    x0: number;
    y0: number;
    moved: boolean;
  }
  interface SelectDrag {
    kind: "select";
    dayIdx: number;
    anchorMin: number;
    fromMin: number;
    toMin: number;
    x0: number;
    y0: number;
    moved: boolean;
  }
  type Drag = EventDrag | SelectDrag;

  let drag = $state<Drag | null>(null);
  /** 拖拽后短暂抑制 click / dblclick（pointerup 后浏览器仍会派发 click） */
  let suppressClickUntil = 0;
  /** 列几何缓存（拖拽开始时取一次；left 相对 grid-wrap） */
  let colCache: { left: number; width: number }[] = [];

  /** 跨天可见段 / 全天 / 非日程块不参与拖拽 */
  function draggable(ev: Item): boolean {
    if (ev.type !== "event" || ev.all_day || !ev.start_at || !ev.end_at) return false;
    const s = new Date(ev.start_at);
    const e = new Date(ev.end_at);
    return toDateInput(s) === toDateInput(e);
  }

  function cacheCols() {
    if (!gridEl) return;
    const g = gridEl.getBoundingClientRect();
    colCache = [...gridEl.querySelectorAll<HTMLElement>(".day-col")].map((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left - g.left, width: r.width };
    });
  }

  function nearestCol(x: number): number {
    let best = 0;
    let bestDist = Infinity;
    colCache.forEach((c, i) => {
      const dist = Math.abs(x - (c.left + c.width / 2));
      if (dist < bestDist) {
        bestDist = dist;
        best = i;
      }
    });
    return best;
  }

  function onEventPointerDown(e: PointerEvent, ev: Item, dayIdx: number) {
    if (e.button !== 0 || !draggable(ev) || !gridEl) return;
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const zone =
      e.clientY - rect.top <= 7 ? "resize-start" : rect.bottom - e.clientY <= 7 ? "resize-end" : "move";
    cacheCols();
    const base = items.find((it) => it.id === ev.id) ?? ev;
    const s = new Date(ev.start_at!);
    const en = ev.end_at ? new Date(ev.end_at) : s;
    const occStartMin = s.getHours() * 60 + s.getMinutes();
    const occEndMin = en.getHours() * 60 + en.getMinutes();
    const g = gridEl.getBoundingClientRect();
    const pointerMin = ((e.clientY - g.top) / HOUR_H) * 60;
    drag = {
      kind: zone,
      base,
      ev,
      occStartMin,
      occEndMin,
      grabMin: Math.max(0, pointerMin - occStartMin),
      dayIdx,
      startMin: occStartMin,
      endMin: occEndMin,
      x0: e.clientX,
      y0: e.clientY,
      moved: false,
    };
  }

  function onSlotPointerDown(e: PointerEvent, dayIdx: number) {
    if (e.button !== 0 || !gridEl) return;
    cacheCols();
    const g = gridEl.getBoundingClientRect();
    const anchorMin = snapMinutes(((e.clientY - g.top) / HOUR_H) * 60);
    drag = {
      kind: "select",
      dayIdx,
      anchorMin,
      fromMin: anchorMin,
      toMin: anchorMin,
      x0: e.clientX,
      y0: e.clientY,
      moved: false,
    };
  }

  function onDragMove(e: PointerEvent) {
    if (!drag || !gridEl) return;
    const dx = e.clientX - drag.x0;
    const dy = e.clientY - drag.y0;
    if (!drag.moved) {
      if (Math.hypot(dx, dy) < 5) return;
      drag.moved = true;
    }
    e.preventDefault();
    const g = gridEl.getBoundingClientRect();
    const min = Math.max(0, Math.min(24 * 60, snapMinutes(((e.clientY - g.top) / HOUR_H) * 60)));
    if (drag.kind === "select") {
      // 划选跟随落点列：跨列拖选 = 在松手的那天创建（时间范围取垂直跨度），
      // 否则锚在按下列、结束时刻却来自另一列的指针，会预填出怪区间
      drag.dayIdx = nearestCol(e.clientX - g.left);
      drag.fromMin = Math.min(drag.anchorMin, min);
      drag.toMin = Math.max(drag.anchorMin, min);
      return;
    }
    drag.dayIdx = nearestCol(e.clientX - g.left);
    const dur = drag.occEndMin - drag.occStartMin;
    if (drag.kind === "move") {
      drag.startMin = Math.max(0, Math.min(24 * 60 - dur, min - drag.grabMin));
      drag.endMin = drag.startMin + dur;
    } else if (drag.kind === "resize-start") {
      drag.startMin = Math.min(min, drag.occEndMin - SNAP_MIN);
      drag.endMin = drag.occEndMin;
    } else {
      drag.startMin = drag.occStartMin;
      drag.endMin = Math.max(min, drag.occStartMin + SNAP_MIN);
    }
  }

  async function applyReschedule(base: Item, r: RescheduleResult) {
    if (!r.patch || Object.keys(r.patch).length === 0) return;
    try {
      await api.updateItem(base.id, r.patch);
      toast.show(r.note, {
        ms: 6000,
        action: {
          label: t("common.undo"),
          run: async () => {
            try {
              await api.updateItem(base.id, r.undo);
            } catch (e) {
              console.error("撤销改期失败", e);
            }
          },
        },
      });
    } catch (e) {
      toast.show(t("week.reschedule_failed", { error: String(e) }));
    }
  }

  function onDragEnd() {
    if (!drag) return;
    const d = drag;
    drag = null;
    if (!d.moved) return; // 未拖动：交给原生 click / dblclick
    suppressClickUntil = Date.now() + 200;
    if (d.kind === "select") {
      if (d.toMin - d.fromMin >= SNAP_MIN) {
        const day = days_[d.dayIdx];
        const mk = (m: number) => new Date(day.getFullYear(), day.getMonth(), day.getDate(), 0, m);
        openCreate({
          anchorDay: dayKey(day),
          presetStart: mk(d.fromMin).toISOString(),
          presetEnd: mk(d.toMin).toISOString(),
        });
      }
      return;
    }
    const day = days_[d.dayIdx];
    const mk = (m: number) => new Date(day.getFullYear(), day.getMonth(), day.getDate(), 0, m);
    const occStart = new Date(d.ev.start_at!);
    const occEnd = d.ev.end_at ? new Date(d.ev.end_at) : occStart;
    let result: RescheduleResult;
    if (d.kind === "move") {
      result = moveEventOccurrence(d.base, occStart, occEnd, mk(d.startMin));
    } else if (d.kind === "resize-start") {
      result = resizeEventOccurrence(d.base, occStart, occEnd, "start", mk(d.startMin));
    } else {
      result = resizeEventOccurrence(d.base, occStart, occEnd, "end", mk(d.endMin));
    }
    void applyReschedule(d.base, result);
  }

  const fmtMin = (m: number) => `${pad(Math.floor(m / 60))}:${pad(m % 60)}`;
  let dragTip = $derived.by(() => {
    if (!drag || !drag.moved) return "";
    if (drag.kind === "select") {
      return drag.toMin - drag.fromMin >= SNAP_MIN
        ? t("week.release_create", { from: fmtMin(drag.fromMin), to: fmtMin(drag.toMin) })
        : t("week.drag_longer");
    }
    const day = days_[drag.dayIdx];
    const dLabel = days > 1 ? ` ${weekLabels[(day.getDay() + 6) % 7]}` : "";
    return t("week.release_apply", {
      range: `${fmtMin(drag.startMin)} – ${fmtMin(drag.endMin)}`,
      day: dLabel,
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpointermove={onDragMove} onpointerup={onDragEnd} />

<div class="week">
  <div class="bar">
    <h2>{rangeLabel}</h2>
    <span class="spacer"></span>
    <button class="ghost" onclick={() => shift(-days)}>‹</button>
    <button class="ghost" onclick={goToday}>{t("common.today")}</button>
    <button class="ghost" onclick={() => shift(days)}>›</button>
  </div>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="head-row">
    <span class="hour-gutter"></span>
    {#each days_ as d, i (dayKey(d))}
      {@const hol = holidayOfKey(dayKey(d))}
      <span class="day-head" class:today={dayKey(d) === todayKey}>
        {weekLabels[(d.getDay() + 6) % 7]} {d.getDate()}
        {#if hol}
          <span class="hol" class:off={hol.off} class:work={!hol.off} title={hol.name}>{hol.off ? t("week.hol_off") : t("week.hol_work")}</span>
        {/if}
      </span>
    {/each}
  </div>
  <div class="head-row sub">
    <span class="hour-gutter"></span>
    {#each days_ as d (dayKey(d))}
      <span class="day-sub">
        <span class="allday-label">{t("week.all_day")}{allDayOn(d).length ? ` · ${allDayOn(d).length}` : ""}</span>
        {#if duesOn(d).length}
          <span class="due-chips" title={duesOn(d).map(displayTitle).join(i18n.locale === "en" ? ", " : "、")}>
            {t("week.due_n", { n: duesOn(d).length })}
          </span>
        {/if}
      </span>
    {/each}
  </div>

  <div class="scroller" bind:this={scroller}>
    <div class="grid-wrap" data-testid="week-grid" bind:this={gridEl} style={`height:${24 * HOUR_H}px`}>
      <!-- 小时刻度 -->
      <div class="hour-gutter">
        {#each Array(24) as _, h (h)}
          <span class="hour-label" style={`top:${h * HOUR_H}px`}>{pad(h)}:00</span>
        {/each}
      </div>
      {#each days_ as d, di (dayKey(d))}
        <div class="day-col" class:today={dayKey(d) === todayKey} class:weekend={d.getDay() === 0 || d.getDay() === 6}>
          {#each Array(24) as _, h (h)}
            <div
              class="slot"
              class:sel={selected?.day.getTime() === d.getTime() && selected?.hour === h}
              style={`top:${h * HOUR_H}px;height:${HOUR_H}px`}
              title={t("week.slot_tip")}
              onclick={() => clickSlot(d, h)}
              ondblclick={() => dblclickSlot(d, h)}
              onpointerdown={(e) => onSlotPointerDown(e, di)}
            ></div>
          {/each}
          {#each gridOn(d) as ev, i (ev.id + ":" + i)}
            {@const geo = layout(gridOn(d), d)[i]}
            {@const seg = segment(ev, d)}
            <button
              class="event"
              class:grab={draggable(ev)}
              class:compact={seg.height < 38}
              style={`top:${seg.top}px;height:${seg.height}px;left:${(geo.col / geo.cols) * 100}%;width:${(1 / geo.cols) * 100}%`}
              title={`${displayTitle(ev)}\n${fmtTime(ev.start_at)}–${fmtTime(ev.end_at)}${ev.recurrence ? "\n🔁 " + ev.recurrence : ""}${draggable(ev) ? `\n${t("week.event_drag_tip")}` : ""}`}
              onclick={(e) => {
                if (Date.now() < suppressClickUntil) return;
                e.stopPropagation();
                openDetail(ev);
              }}
              onpointerdown={(e) => onEventPointerDown(e, ev, di)}
            >
              <!-- 重叠分列时宽度不够：短块只留标题（时间在 tooltip / 详情里） -->
              {#if !(geo.cols >= 2 && seg.height < 38)}
                <span class="ev-time">{fmtTime(ev.start_at)}</span>
              {/if}
              <span class="ev-title">{displayTitle(ev)}</span>
              {#if ev.recurrence}<span class="ev-flag">🔁</span>{/if}
              {#if ev.type !== "event"}<span class="ev-flag">{typeLabel(ev.type)}</span>{/if}
            </button>
          {/each}
          {#if nowLineFor(d) !== null}
            {@const line = nowLineFor(d) as number}
            <div class="now-line" data-t={`${pad(now.getHours())}:${pad(now.getMinutes())}`} style={`top:${line}px`}></div>
          {/if}
        </div>
      {/each}

      <!-- 拖拽 ghost（半透明预览）与选区高亮 -->
      {#if drag && drag.moved && drag.kind !== "select"}
        {@const col = colCache[drag.dayIdx]}
        <div
          class="ghost"
          style={`top:${(drag.startMin / 60) * HOUR_H}px;height:${Math.max(((drag.endMin - drag.startMin) / 60) * HOUR_H, 18)}px;left:${col?.left ?? 0}px;width:${col?.width ?? 0}px`}
        >
          <span class="ev-time">{fmtMin(drag.startMin)}</span>
          <span class="ev-title">{displayTitle(drag.ev)}</span>
        </div>
      {:else if drag && drag.moved && drag.kind === "select"}
        {@const col = colCache[drag.dayIdx]}
        <div
          class="ghost selzone"
          style={`top:${(drag.fromMin / 60) * HOUR_H}px;height:${Math.max(((drag.toMin - drag.fromMin) / 60) * HOUR_H, 4)}px;left:${col?.left ?? 0}px;width:${col?.width ?? 0}px`}
        ></div>
      {/if}
    </div>
  </div>
</div>

{#if dragTip}
  <div class="drag-tip">{dragTip}</div>
{:else if selected}
  <p class="sel-hint">
    {t("week.selected_prefix", {
      range: `${selected.day.getMonth() + 1}/${selected.day.getDate()} ${pad(selected.hour)}:00 – ${pad(selected.hour + 1)}:00`,
    })}
    <button class="link" onclick={() => dblclickSlot(selected!.day, selected!.hour)}>{t("week.create_here")}</button>
  </p>
{/if}

<script module lang="ts">
  const pad = (n: number) => String(n).padStart(2, "0");

  /** 上次浏览的周/日锚点（切页后原位恢复，不弹回当周） */
  let weekMemo: { anchor: Date } | null = null;
</script>

<style>
  .week {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .bar h2 {
    margin: 0;
    font-size: 15px;
  }

  .spacer {
    flex: 1;
  }

  .head-row {
    display: grid;
    grid-template-columns: 52px repeat(auto-fit, minmax(0, 1fr));
    gap: 2px;
  }

  .head-row.sub {
    margin-top: -4px;
  }

  .day-head {
    text-align: center;
    color: var(--text-dim);
    font-size: 13px;
    padding: 4px 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }

  .day-head.today {
    color: var(--accent);
    font-weight: 700;
  }

  .hol {
    font-size: 10px;
    line-height: 1;
    padding: 2px 3px;
    border-radius: 4px;
    font-weight: 700;
  }

  .hol.off {
    color: #2e9e5b;
    background: color-mix(in srgb, #2e9e5b 16%, transparent);
  }

  .hol.work {
    color: #d05656;
    background: color-mix(in srgb, #d05656 14%, transparent);
  }

  .day-sub {
    display: flex;
    gap: 6px;
    justify-content: center;
    font-size: 11px;
    color: var(--text-dim);
    padding-bottom: 4px;
  }

  .due-chips {
    color: var(--danger, #d33);
  }

  .scroller {
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--card);
  }

  .grid-wrap {
    position: relative;
    display: grid;
    grid-template-columns: 52px repeat(auto-fit, minmax(0, 1fr));
    gap: 2px;
    touch-action: none;
    user-select: none;
  }

  .hour-gutter {
    position: relative;
  }

  .hour-label {
    position: absolute;
    right: 6px;
    transform: translateY(-50%);
    font-size: 10px;
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }

  .day-col {
    position: relative;
    border-left: 1px solid var(--border);
  }

  /* 今天列微强调 + 周末列弱化（Google/Notion 日历惯例） */
  .day-col.weekend {
    background: color-mix(in srgb, var(--text) 3%, transparent);
  }

  .day-col.today {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .slot {
    position: absolute;
    left: 0;
    right: 0;
    border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
    background: transparent;
    border-radius: 0;
  }

  .slot.sel {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .event {
    position: absolute;
    z-index: 2;
    overflow: hidden;
    text-align: left;
    background: color-mix(in srgb, var(--accent) 16%, var(--card));
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-left: 3px solid var(--accent);
    border-radius: 6px;
    padding: 2px 6px;
    font-size: 11px;
    line-height: 1.35;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .event.grab {
    cursor: grab;
  }

  .event.grab:active {
    cursor: grabbing;
  }

  /* 短块（<50 分钟）单行：标题在前、时间随后，两行排不下只会剩时间 */
  .event.compact {
    flex-direction: row-reverse;
    justify-content: flex-end;
    align-items: center;
    gap: 5px;
  }

  .event.compact .ev-time {
    flex-shrink: 0;
  }

  /* 上下边缘调时长命中区（视觉不可见，只换光标） */
  .event.grab::before,
  .event.grab::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    height: 7px;
    cursor: ns-resize;
  }

  .event.grab::before {
    top: 0;
  }

  .event.grab::after {
    bottom: 0;
  }

  .event .ev-time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
  }

  .event .ev-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }

  .event .ev-flag {
    color: var(--text-dim);
    font-size: 10px;
  }

  .ghost {
    position: absolute;
    z-index: 5;
    pointer-events: none;
    border-radius: 6px;
    padding: 2px 6px;
    font-size: 11px;
    display: flex;
    flex-direction: column;
    gap: 1px;
    background: color-mix(in srgb, var(--accent) 30%, var(--card));
    border: 1px solid var(--accent);
    border-left: 3px solid var(--accent);
    box-shadow: 0 4px 14px rgb(0 0 0 / 18%);
    opacity: 0.92;
  }

  .ghost .ev-time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
  }

  .ghost .ev-title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ghost.selzone {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border: 1px dashed color-mix(in srgb, var(--accent) 70%, transparent);
  }

  .now-line {
    position: absolute;
    left: 0;
    right: 0;
    height: 2px;
    background: #e05252;
    z-index: 3;
    pointer-events: none;
  }

  /* 时刻气泡：贴在小时刻度一侧（Google Calendar 式） */
  .now-line::after {
    content: attr(data-t);
    position: absolute;
    left: -54px;
    top: -9px;
    font-size: 10px;
    line-height: 1;
    padding: 2px 4px;
    border-radius: 4px;
    color: #fff;
    background: #e05252;
    font-variant-numeric: tabular-nums;
  }

  .now-line::before {
    content: "";
    position: absolute;
    left: -4px;
    top: -3px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #e05252;
  }

  .drag-tip {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--card);
    border: 1px solid var(--accent);
    color: var(--text);
    border-radius: 999px;
    padding: 6px 16px;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    box-shadow: 0 6px 20px rgb(0 0 0 / 22%);
    z-index: 95;
    pointer-events: none;
  }

  .sel-hint {
    margin: 0;
    font-size: 13px;
    color: var(--text-dim);
  }

  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    font-size: 13px;
    text-decoration: underline;
  }

  .error {
    color: var(--danger);
  }
</style>

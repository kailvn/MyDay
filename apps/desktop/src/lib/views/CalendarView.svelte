<script lang="ts">
  /**
   * 日历（定稿交互）：单击格子 = 选中并在侧栏看当天信息；
   * 双击格子 = 以该日为锚点打开快速添加（类型自动，不强制日程——
   * 激活哪个时间字段，默认值就是这天：开始9:00 / 结束18:00 / 截止当天结束）。
   *
   * 当天信息 = 所有内置时间字段中和当天关联的条目，分三节：
   * 日程（event 与当天相交 / 带开始的任务）、到期（due_at 当天）、记录（occurred_at 当天）。
   * 数据走 list_items_window（OR 语义，跨天日程两头都算），
   * 修复了旧版「记录归到创建日、due-only 待办不显示」的问题。
   *
   * 完成语义（红=紧迫，只属于未完成）：月格里已完成的到期待办灰化 + 删除线，
   * 当天面板已完成行沉底、勾选框就地完成 / 回退——与今天页、周视图口径一致。
   */
  import { api, anchorTime, displayTitle, fmtTime, toDateInput, type Item } from "../api";
  import { expandItems, recurrenceLabel } from "../recurrence";
  import { deletions } from "../deletion.svelte";
  import WeekGrid from "../WeekGrid.svelte";
  import DeleteButton from "../DeleteButton.svelte";
  import EditButton from "../EditButton.svelte";
  import { openCreate, openEdit, rowDetail } from "../panel.svelte";
  import { toast } from "../toast.svelte";
  import { holidayOfKey } from "../holidays.svelte";
  import { moveEventOccurrence, moveTaskDue, type RescheduleResult } from "../reschedule";
  import { t, q, i18n } from "../i18n";

  let { dataVersion = 0 } = $props();  /** 月 = 网格 + 当天面板；周 / 日 = 时间网格（WeekGrid，日 = 单列） */
  let mode = $state<CalMode>(memo?.mode ?? "month");
  const modes = [
    { id: "month", label: "calendar.mode_month" },
    { id: "week", label: "calendar.mode_week" },
    { id: "day", label: "calendar.mode_day" },
  ] as const;

  let cursor = $state(memo?.cursor ?? new Date());
  let selected = $state(memo?.selected ?? new Date().getDate());
  let monthItems = $state<Item[]>([]);
  let error = $state("");

  // 记住上次浏览状态（Anytype 用户高频诉求：切回日历不要弹回当月）
  $effect(() => {
    memo = { mode, cursor, selected };
  });

  function pad(n: number) {
    return String(n).padStart(2, "0");
  }
  function dateKey(d: Date, day: number) {
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(day)}`;
  }
  function dayKeyOf(iso: string): string {
    const d = new Date(iso);
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  }

  /** 周一为一周开始（需求 §12 周起始日，可在设置中扩展）：星期名随 locale（2023-01-02 恰是周一） */
  const localeTag = () => (i18n.locale === "en" ? "en-US" : "zh-CN");
  let weekLabels = $derived(
    Array.from({ length: 7 }, (_, i) => new Date(2023, 0, 2 + i).toLocaleDateString(localeTag(), { weekday: "short" })),
  );

  /** 月格：key = 本地日键；adj = 相邻月（灰化但仍可点/可拖入） */
  interface Cell {
    key: string;
    day: number;
    adj: boolean;
  }

  let todayKey = $derived(toDateInput(new Date()));
  /** 月份 / 选中日标题：月份名随 locale（2026年9月 / September 2026） */
  let monthLabel = $derived(cursor.toLocaleDateString(localeTag(), { year: "numeric", month: "long" }));
  let selectedLabel = $derived(
    new Date(cursor.getFullYear(), cursor.getMonth(), selected).toLocaleDateString(localeTag(), {
      month: "long",
      day: "numeric",
    }),
  );

  let grid = $derived.by(() => {
    const cells: Cell[] = [];
    for (const d = new Date(gridBounds.start); d <= gridBounds.end; d.setDate(d.getDate() + 1)) {
      const day = d.getDate();
      cells.push({ key: dateKey(d, day), day, adj: d.getMonth() !== cursor.getMonth() });
    }
    return cells;
  });

  /** 网格边界：1 号所在周到月末所在周日（相邻月补齐整周，Notion/Anytype 式） */
  let gridBounds = $derived.by(() => {
    const year = cursor.getFullYear();
    const month = cursor.getMonth();
    const start = new Date(year, month, 1);
    start.setDate(start.getDate() - ((start.getDay() + 6) % 7)); // 回到周一
    const end = new Date(year, month + 1, 0);
    end.setDate(end.getDate() + ((7 - end.getDay()) % 7)); // 推到周日（旧式 (day+6)%7 会推到下周一，多出一行幻影周）
    return { start, end };
  });

  /** 本地某日 [00:00, 23:59:59.999] 的 UTC ISO 对 */
  function dayRange(year: number, month: number, day: number): [string, string] {
    return [
      new Date(year, month, day, 0, 0, 0, 0).toISOString(),
      new Date(year, month, day, 23, 59, 59, 999).toISOString(),
    ];
  }

  // ---- 月格 chip 预算（P1 行高不齐修复）------------------------------------
  // .cell 用 aspect-ratio 定高，但内容会把格子的自动最小尺寸撑破 → 行内一高一矮。
  // 格子加 overflow: hidden 后比例高度保住；chip 显示条数改为按格子实际高度
  // 自适应（整行截断，不切半行字，溢出数「+N」恒占一行可见）。常量须与样式一致：
  // chip 字号 10px × line-height 1.5 = 15px 行盒，行距 1px。
  let gridEl = $state<HTMLDivElement>();
  let chipBudget = $state(3);

  function measureChipBudget() {
    // 网格恒为整周（相邻月补位格也有日号），取首格测量即可
    const cell = gridEl?.querySelector<HTMLElement>(".cell");
    const dayNum = cell?.querySelector<HTMLElement>(".day-num");
    if (!cell || !dayNum) return;
    // 可用高度 = 格子内容盒底 − 日号行底 − 底部 padding(6) − 区块上间距(2)
    const avail = cell.clientHeight - (dayNum.offsetTop + dayNum.offsetHeight) - 8;
    chipBudget = Math.max(0, Math.min(3, Math.floor((avail + 1) / 16)));
  }

  $effect(() => {
    if (!gridEl) return;
    measureChipBudget();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measureChipBudget);
    ro.observe(gridEl);
    return () => ro.disconnect();
  });

  /** 条目关联的本地日键集合（跨天日程覆盖每一天，供网格圆点用） */
  function relatedDays(it: Item): string[] {
    const days = new Set<string>();
    const pushRange = (fromIso: string, toIso: string) => {
      const end = new Date(toIso);
      for (let d = new Date(fromIso); d <= end; d.setDate(d.getDate() + 1)) {
        days.add(`${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`);
      }
    };
    if (it.type === "log") {
      if (it.occurred_at) days.add(dayKeyOf(it.occurred_at));
    } else if (it.type === "task") {
      if (it.due_at) days.add(dayKeyOf(it.due_at));
      if (it.start_at) days.add(dayKeyOf(it.start_at));
    } else if (it.start_at) {
      pushRange(it.start_at, it.end_at ?? it.start_at);
    }
    return [...days];
  }

  let eventsByDay = $derived.by(() => {
    const map = new Map<string, Item[]>();
    for (const it of displayItems) {
      if (deletions.pendingIds.includes(it.id)) continue;
      for (const key of relatedDays(it)) {
        if (!map.has(key)) map.set(key, []);
        map.get(key)!.push(it);
      }
    }
    return map;
  });

  /** 月格 chip 文案：日程/全天/到期带时间前缀，记录只有标题 */
  function cellLabel(it: Item): string {
    const title = displayTitle(it);
    if (it.type === "log") return title;
    if (it.type === "task") return `${fmtTime(it.due_at)} ${title}`;
    if (it.all_day) return `${t("calendar.all_day")} ${title}`;
    return `${fmtTime(it.start_at)} ${title}`;
  }
  /** 月格 chip 悬停提示：已完成待办注明状态（红=紧迫只属于未完成） */
  function cellTip(it: Item): string {
    const label = cellLabel(it);
    return it.type === "task" && it.status === "done" ? `✓ ${t("common.done")} · ${label}` : label;
  }
  /** 月格 title：日期键 · 节假日（休/班）· 交互提示 */
  function cellTitle(cell: Cell, hol: ReturnType<typeof holidayOfKey>): string {
    const holPart = hol ? ` · ${hol.name} ${hol.off ? t("calendar.hol_off") : t("calendar.hol_work")}` : "";
    return `${cell.key}${holPart} · ${t("calendar.cell_tip")}`;
  }
  /** 月格 chip 排序：按条目锚点时间升序 */
  function byAnchor(a: Item, b: Item): number {
    return Date.parse(anchorTime(a) ?? "") - Date.parse(anchorTime(b) ?? "");
  }

  // 重复条目展开为窗口内虚拟实例（同 id，时间字段换成本次发生）：
  // 网格圆点与当天面板都用展开后的列表；点击打开的仍是原条目
  let displayItems = $derived(
    expandItems(
      monthItems,
      new Date(gridBounds.start.getFullYear(), gridBounds.start.getMonth(), gridBounds.start.getDate(), 0, 0, 0, 0),
      new Date(gridBounds.end.getFullYear(), gridBounds.end.getMonth(), gridBounds.end.getDate(), 23, 59, 59, 999),
    ),
  );

  type Section = { title: string; items: Item[] };
  /** 已完成沉底（与今天页一致），同组内按时间升序 */
  const doneLast = (a: Item, b: Item) => (a.status === "done" ? 1 : 0) - (b.status === "done" ? 1 : 0);
  /** 当天信息三节（时间升序，已完成沉底） */
  let daySections = $derived.by<Section[]>(() => {
    const [from, to] = dayRange(cursor.getFullYear(), cursor.getMonth(), selected);
    const fromT = Date.parse(from);
    const toT = Date.parse(to);
    const ms = (iso: string | null) => (iso ? Date.parse(iso) : Number.NaN);
    const visible = displayItems.filter((it) => !deletions.pendingIds.includes(it.id));
    const events = visible
      .filter((it) => {
        if (it.type === "event") {
          const s = ms(it.start_at);
          const e = ms(it.end_at ?? it.start_at);
          return s <= toT && e >= fromT; // 区间相交：跨天两头都算
        }
        return it.type === "task" && !!it.start_at && ms(it.start_at) >= fromT && ms(it.start_at) <= toT;
      })
      .sort((a, b) => doneLast(a, b) || ms(a.start_at) - ms(b.start_at));
    const dues = visible
      .filter((it) => it.type === "task" && !!it.due_at && ms(it.due_at) >= fromT && ms(it.due_at) <= toT)
      .sort((a, b) => doneLast(a, b) || ms(a.due_at) - ms(b.due_at));
    const logs = visible
      .filter((it) => it.type === "log" && !!it.occurred_at && ms(it.occurred_at) >= fromT && ms(it.occurred_at) <= toT)
      .sort((a, b) => ms(a.occurred_at) - ms(b.occurred_at));
    return [
      { title: t("type.event"), items: events },
      { title: t("calendar.section_due"), items: dues },
      { title: t("type.log"), items: logs },
    ].filter((s) => s.items.length > 0);
  });

  function shiftMonth(delta: number) {
    const year = cursor.getFullYear();
    const month = cursor.getMonth() + delta;
    cursor = new Date(year, month, 1);
    // 选中日收进新月份（1 号或今天）
    const now = new Date();
    selected =
      now.getFullYear() === cursor.getFullYear() && now.getMonth() === cursor.getMonth()
        ? now.getDate()
        : 1;
  }

  /** 单击：只选中，侧栏看当天（相邻月 = 跳转该月并选中；创建走双击 / ＋ / 侧栏「＋ 新建」） */
  function clickCell(cell: Cell) {
    if (cell.adj) {
      const [y, m] = cell.key.split("-").map(Number);
      cursor = new Date(y, m - 1, 1);
    }
    selected = cell.day;
  }

  /** 双击：以该日为锚点打开快速添加（类型自动；相邻月先跳转） */
  function dblclickCell(cell: Cell) {
    clickCell(cell);
    openCreate({ anchorDay: cell.key });
  }

  /** 月视图回到今天（‹ 今天 › 与 T 键） */
  function goTodayMonth() {
    const now = new Date();
    cursor = new Date(now.getFullYear(), now.getMonth(), 1);
    selected = now.getDate();
  }

  // ---- 月视图拖拽改期（SPRINT2-SPEC §3）-----------------------------------
  /** 当天面板行 → 月历格：日程整体平移天数、到期待办改截止日（保钟点） */
  interface RowDrag {
    kind: "event" | "task";
    /** 系列原条目 */
    base: Item;
    /** 展开后的本次发生 */
    ev: Item;
    x0: number;
    y0: number;
    gx: number;
    gy: number;
    moved: boolean;
    label: string;
    targetKey: string | null;
  }
  let rowDrag = $state<RowDrag | null>(null);
  let suppressRowClickUntil = 0;

  function onRowPointerDown(e: PointerEvent, kind: "event" | "task", ev: Item) {
    if (e.button !== 0) return;
    if (kind === "task" && !ev.due_at) return; // 无截止的待办不参与拖拽
    if (kind === "event" && (ev.type !== "event" || !ev.start_at)) return;
    rowDrag = {
      kind,
      base: monthItems.find((it) => it.id === ev.id) ?? ev,
      ev,
      x0: e.clientX,
      y0: e.clientY,
      gx: e.clientX,
      gy: e.clientY,
      moved: false,
      label: displayTitle(ev),
      targetKey: null,
    };
  }

  function onRowDragMove(e: PointerEvent) {
    if (!rowDrag) return;
    if (!rowDrag.moved) {
      if (Math.hypot(e.clientX - rowDrag.x0, e.clientY - rowDrag.y0) < 5) return;
      rowDrag.moved = true;
    }
    e.preventDefault();
    rowDrag.gx = e.clientX;
    rowDrag.gy = e.clientY;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    rowDrag.targetKey = (el?.closest("[data-cellday]") as HTMLElement | null)?.dataset.cellday ?? null;
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
      toast.show(t("calendar.reschedule_failed", { error: String(e) }));
    }
  }

  function onRowDragEnd() {
    const d = rowDrag;
    rowDrag = null;
    if (!d || !d.moved || !d.targetKey) return;
    suppressRowClickUntil = Date.now() + 200;
    const [y, m, day] = d.targetKey.split("-").map(Number);
    const target = new Date(y, m - 1, day);
    if (d.kind === "event") {
      const s = new Date(d.ev.start_at!);
      const en = d.ev.end_at ? new Date(d.ev.end_at) : s;
      const newStart = new Date(
        target.getFullYear(),
        target.getMonth(),
        target.getDate(),
        s.getHours(),
        s.getMinutes(),
      );
      void applyReschedule(d.base, moveEventOccurrence(d.base, s, en, newStart));
    } else {
      void applyReschedule(d.base, moveTaskDue(d.base, target));
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && rowDrag) {
      rowDrag = null;
      return;
    }
    // 日历快捷键（Notion Calendar 式）：输入控件聚焦时不劫持
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key.toLowerCase();
    if (k === "d" || k === "w" || k === "m") {
      e.preventDefault();
      mode = k === "d" ? "day" : k === "w" ? "week" : "month";
    } else if (mode === "month") {
      if (k === "t") {
        e.preventDefault();
        goTodayMonth();
      } else if (e.key === "ArrowLeft") {
        e.preventDefault();
        shiftMonth(-1);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        shiftMonth(1);
      }
    }
  }

  /** 侧栏「＋ 新建」：为当前选中日期打开创建面板 */
  function createOnSelected() {
    openCreate({ anchorDay: dateKey(cursor, selected) });
  }

  /** 当天面板勾选 / 回退待办（与今天页一致：未完成 → completeTask，已完成 → 改回 todo） */
  async function toggleDone(it: Item) {
    try {
      if (it.status === "done") {
        await api.updateItem(it.id, { status: "todo" });
      } else {
        await api.completeTask(it.id);
      }
      await load();
    } catch (e) {
      toast.show(t("calendar.update_failed", { error: String(e) }));
    }
  }

  async function load() {
    try {
      // 窗口查询：覆盖整周网格（含相邻月补位格），四种内置时间字段一次取回
      const start = new Date(
        gridBounds.start.getFullYear(),
        gridBounds.start.getMonth(),
        gridBounds.start.getDate(),
        0, 0, 0, 0,
      );
      const end = new Date(
        gridBounds.end.getFullYear(),
        gridBounds.end.getMonth(),
        gridBounds.end.getDate(),
        23, 59, 59, 999,
      );
      monthItems = await api.listItemsWindow(start.toISOString(), end.toISOString());
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    dataVersion;
    cursor;
    load();
  });
</script>

<h1>
  {t("calendar.title")}
  {#if mode === "month"}
  <span class="month">
    <button class="ghost" onclick={() => shiftMonth(-1)} title={t("calendar.prev_month")}>‹</button>
    {monthLabel}
    <button class="ghost" onclick={() => shiftMonth(1)} title={t("calendar.next_month")}>›</button>
    <button class="ghost" onclick={goTodayMonth} title={t("calendar.go_today")}>{t("common.today")}</button>
  </span>
  {/if}
  <span class="mode-tabs" title={t("calendar.mode_tabs_tip")}>
    {#each modes as m (m.id)}
      <button class:active={mode === m.id} onclick={() => (mode = m.id)}>{t(m.label)}</button>
    {/each}
  </span>
</h1>

<svelte:window onkeydown={onKeydown} onpointermove={onRowDragMove} onpointerup={onRowDragEnd} />

{#if mode !== "month"}
  <WeekGrid {dataVersion} days={mode === "week" ? 7 : 1} />
{:else}

{#if error}<p class="error">{error}</p>{/if}

<div class="calendar-layout">
  <div class="grid" bind:this={gridEl}>
    {#each weekLabels as w (w)}
      <div class="week-label">{w}</div>
    {/each}
    {#each grid as cell (cell.key)}
      {@const hol = holidayOfKey(cell.key)}
      <button
        class="cell"
        class:adj={cell.adj}
        class:today={cell.key === todayKey}
        class:selected={!cell.adj && cell.day === selected}
        class:droptarget={rowDrag?.targetKey === cell.key}
        data-cellday={cell.key}
        title={cellTitle(cell, hol)}
        onclick={() => clickCell(cell)}
        ondblclick={() => dblclickCell(cell)}
      >
        <span class="cell-head">
          <span class="day-num">{cell.day}</span>
          {#if hol}
            <span class="hol" class:off={hol.off} class:work={!hol.off}>{hol.off ? t("calendar.hol_off") : t("calendar.hol_work")}</span>
          {/if}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span
            class="plus"
            onclick={(e) => {
              e.stopPropagation();
              openCreate({ anchorDay: cell.key });
            }}
          >＋</span>
        </span>
        {#if eventsByDay.has(cell.key)}
          {@const evs = eventsByDay.get(cell.key)!.slice().sort(byAnchor)}
          {@const budget = chipBudget}
          <!-- 整行截断：预算 ≥2 时「条目×(预算−1) + 溢出数」；预算 1 只放一条 chip；
               预算 0（格子极窄）什么都不渲染——任何一行都不会被切半行 -->
          {@const showMore = evs.length > budget && budget >= 2}
          {@const visible = evs.slice(0, showMore ? budget - 1 : budget)}
          <span class="cell-evs">
            {#each visible as ev (ev.id + (anchorTime(ev) ?? ""))}
              <span
                class="cell-ev"
                class:due={ev.type === "task" && ev.status !== "done"}
                class:log={ev.type === "log"}
                class:done={ev.status === "done"}
                title={cellTip(ev)}
              >
                {cellLabel(ev)}
              </span>
            {/each}
            {#if showMore}
              <span class="cell-more">+{evs.length - visible.length}</span>
            {/if}
          </span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="day-panel">
    <h2>
      {selectedLabel}
      <button class="ghost add" onclick={createOnSelected}>{t("calendar.add_new")}</button>
    </h2>
    {#if daySections.length === 0}
      <div class="empty">{t("calendar.day_empty", { name: q(t("calendar.add_new")) })}</div>
    {:else}
      {#each daySections as section (section.title)}
        <h3>{section.title}</h3>
        <ul>
          {#each section.items as ev (ev.id)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <li
              class:done={ev.status === "done"}
              class:draggable={section.title !== t("type.log")}
              title={section.title === t("type.log") ? undefined : t("calendar.row_drag_tip")}
              onclick={(e) => {
                if (Date.now() < suppressRowClickUntil) return;
                rowDetail(ev)(e);
              }}
              onpointerdown={(e) =>
                onRowPointerDown(e, section.title === t("calendar.section_due") ? "task" : "event", ev)}
            >
              {#if ev.type === "task"}
                <!-- 勾选框：就地完成 / 回退（点选框不开详情、不触发拖拽） -->
                <input
                  type="checkbox"
                  checked={ev.status === "done"}
                  title={ev.status === "done" ? t("calendar.mark_undone") : t("calendar.mark_done")}
                  onpointerdown={(e) => e.stopPropagation()}
                  onclick={(e) => e.stopPropagation()}
                  onchange={() => toggleDone(ev)}
                />
              {/if}
              <span class="time">
                {#if section.title === t("type.log")}
                  <!-- log 无 start_at：显示发生时刻（与排序锚点同字段，免空占位列） -->
                  {fmtTime(ev.occurred_at)}
                {:else if section.title === t("calendar.section_due")}
                  {fmtTime(ev.due_at)} {t("calendar.section_due")}
                {:else if ev.all_day}
                  {t("calendar.all_day")}
                {:else}
                  {fmtTime(ev.start_at)}{#if ev.end_at}–{fmtTime(ev.end_at)}{/if}
                {/if}
              </span>
              <span class="title">{displayTitle(ev)}</span>
              {#if ev.recurrence}<span class="tag nowrap">🔁 {recurrenceLabel(ev.recurrence)}</span>{/if}
              {#each ev.tags as tg (tg)}<span class="tag">#{tg}</span>{/each}
              <EditButton onedit={() => openEdit(ev)} />
              <DeleteButton onconfirm={() => deletions.request(ev)} />
            </li>
          {/each}
        </ul>
      {/each}
    {/if}
  </div>
</div>
{/if}

<!-- 拖拽 ghost：跟随指针的小标签（SPRINT2-SPEC §3） -->
{#if rowDrag?.moved}
  <div class="row-ghost" style={`left:${rowDrag.gx + 12}px;top:${rowDrag.gy - 14}px`}>
    {rowDrag.label}{#if rowDrag.targetKey}<span class="ghost-day"> → {t("calendar.ghost_day", { n: Number(rowDrag.targetKey.slice(8)) })}</span>{/if}
  </div>
{/if}


<script module lang="ts">
  /** 月 = 网格 + 当天面板；周 / 日 = 时间网格（WeekGrid，日 = 单列） */
  type CalMode = "month" | "week" | "day";

  /** 上次日历浏览状态（切页后原位恢复） */
  let memo: { mode: CalMode; cursor: Date; selected: number } | null = null;
</script>

<style>
  h1 {
    margin: 0 0 16px;
    display: flex;
    align-items: center;
    gap: 16px;
  }

  .mode-tabs {
    display: inline-flex;
    gap: 2px;
    margin-left: auto;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px;
  }

  .mode-tabs button {
    border: none;
    background: transparent;
    border-radius: 999px;
    padding: 3px 14px;
    font-size: 13px;
  }

  .mode-tabs button.active {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .month {
    font-size: 15px;
    font-weight: 500;
    color: var(--text-dim);
    display: flex;
    align-items: center;
    gap: 4px;
  }

  /* minmax(0,1fr)：允许格列收缩到 0——事件标题 nowrap 的 min-content 不再
     托底列宽，缩窗时格子随之变小（否则窄窗下侧栏被挤出/压到格子上）。
     窗口 < 1080px 时侧栏换到网格下方，两者互不挤压。 */
  .calendar-layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 320px;
    gap: 18px;
  }

  @media (max-width: 1080px) {
    .calendar-layout {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 4px;
  }

  .week-label {
    text-align: center;
    color: var(--text-dim);
    font-size: 12px;
    padding: 4px 0;
  }

  .cell {
    aspect-ratio: 1.15;
    /* 溢出裁剪：格子的自动最小尺寸归零，内容再多也撑不破比例高度，
       同行 7 格（auto 行高取行内最高格）底边保持齐平 */
    overflow: hidden;
    border: none;
    border-radius: 8px;
    background: var(--card);
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: stretch;
    justify-content: flex-start;
    gap: 2px;
    padding: 6px;
  }

  /* 大面积月格豁免全局 button:hover 提亮（整格亮度跳变太显眼，见布局 TODO P4），
     改为轻微底色反馈；须放在 selected / droptarget 之前让位 */
  .cell:hover {
    filter: none;
    background: color-mix(in srgb, var(--text) 4%, var(--card));
  }

  /* 格头行：日号靠左，休/班角标随行紧跟，hover「＋」推到最右。
     （旧版用 ::before 当弹性占位——伪元素永远排在第一个子元素之前，
     把日号推到了右缘、和绝对定位的角标叠在一起） */
  .cell-head {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 16px;
  }

  .cell .plus {
    display: none;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    width: 16px;
    height: 16px;
    border-radius: 5px;
    color: var(--text-dim);
    font-size: 12px;
    line-height: 1;
    cursor: pointer;
  }

  .cell:hover .plus,
  .cell:focus-visible .plus {
    display: inline-flex;
  }

  .cell .plus:hover {
    color: var(--accent-fg);
    background: var(--accent);
  }

  /* 相邻月补位格：整体弱化但仍可点、可拖入、可创建（Notion/Anytype 式） */
  .cell.adj {
    background: transparent;
  }

  .cell.adj .day-num,
  .cell.adj .hol,
  .cell.adj .cell-ev,
  .cell.adj .cell-more {
    opacity: 0.42;
  }

  .cell.adj:hover {
    background: color-mix(in srgb, var(--text) 3%, transparent);
  }

  /* 今天格：日号强调 + 极轻底色（选中态 outline 更强，见下） */
  .cell.today {
    background: color-mix(in srgb, var(--accent) 6%, var(--card));
  }

  /* 月格内事件标题（最多 3 条 + 溢出数）：点格子仍是选中 */
  .cell-evs {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .cell-ev {
    font-size: 10px;
    line-height: 1.5;
    padding: 0 4px;
    border-radius: 3px;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    pointer-events: none;
    background: color-mix(in srgb, var(--accent) 14%, var(--card));
  }

  .cell-ev.due {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 12%, var(--card));
  }

  /* 已完成待办：紧迫感退场（红只属于未完成的截止），灰化 + 删除线，与今天页一致 */
  .cell-ev.done {
    color: var(--text-dim);
    background: color-mix(in srgb, var(--text) 7%, var(--card));
    text-decoration: line-through;
  }

  /* 记录：中性灰（区别于日程蓝 / 到期红） */
  .cell-ev.log {
    color: var(--text-dim);
    background: color-mix(in srgb, var(--text) 7%, var(--card));
  }

  .cell-more {
    font-size: 10px;
    color: var(--accent);
    text-align: left;
    pointer-events: none;
  }

  .cell.today .day-num {
    color: var(--accent);
    font-weight: 700;
  }

  .cell.selected {
    outline: 2px solid var(--accent);
  }

  /* 拖拽悬停目标格 */
  .cell.droptarget {
    outline: 2px dashed var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--card));
  }

  /* 休/班角标：随格头行内联（紧跟日号），不再绝对定位压住日号 */
  .hol {
    flex-shrink: 0;
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

  .day-panel li.draggable {
    cursor: grab;
  }

  .day-panel li.draggable:active {
    cursor: grabbing;
  }

  .row-ghost {
    position: fixed;
    z-index: 120;
    pointer-events: none;
    background: var(--card);
    border: 1px solid var(--accent);
    border-radius: 8px;
    padding: 5px 12px;
    font-size: 13px;
    font-weight: 600;
    box-shadow: 0 6px 18px rgb(0 0 0 / 25%);
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-ghost .ghost-day {
    color: var(--accent);
    font-variant-numeric: tabular-nums;
  }

  .day-panel {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px;
    align-self: start;
  }

  .day-panel h2 {
    margin: 0 0 10px;
    font-size: 14px;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .day-panel h2 .add {
    font-size: 12px;
    padding: 2px 8px;
  }

  .day-panel h3 {
    margin: 10px 0 6px;
    font-size: 12px;
    color: var(--text-dim);
    font-weight: 600;
  }

  .day-panel .tag {
    white-space: nowrap;
  }

  .day-panel ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .day-panel li {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 13px;
    cursor: pointer;
    border-radius: 6px;
    padding: 2px 4px;
  }

  .day-panel li:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }

  /* 已完成行：标题删除线 + 灰化（与今天页一致） */
  .day-panel li.done .title {
    text-decoration: line-through;
    color: var(--text-dim);
  }

  .day-panel li input[type="checkbox"] {
    flex-shrink: 0;
  }

  /* 标题按自然宽度参与布局（flex-basis 0 会把标题挤成 0 宽），收缩率压到 0.1：
     空间不足时 tag（shrink 10）先收缩到底，标题几乎不缩——标题是行内最重要信息 */
  .day-panel .title {
    flex: 1 0.1 auto;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* 空间不足时 tag 先于标题收缩截断（标题是行内最重要的信息） */
  .day-panel li .tag {
    flex-shrink: 10;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    min-width: 84px;
  }

  .error {
    color: var(--danger);
  }
</style>

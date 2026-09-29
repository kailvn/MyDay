<script lang="ts">
  /**
   * 今天视图（场景 A / F）：
   * - 今日安排：分「日程 / 待办」两组——日程 = 当天日程（按开始时间，含重复展开）；
   *   待办 = 带开始时间的待办 ∪ 今天到期或逾期的未完成待办（按 ID 去重）
   * - 展示逻辑（对齐 Notion/日历类应用）：进行中的日程高亮标「进行中」、
   *   下一个未开始的标「下一个」；逾期待办红标「逾期」并显示完整截止日期
   *   时间；已完成的行沉到列表末尾。
   * - 今日活动：今天创建或修改过的所有条目（含日程/待办/记录），倒序；可折叠，默认收起。
   * 每行显示创建/修改时间，可编辑（✎）与删除（✕，入回收站）。
   */
  import { onMount } from "svelte";
  import { api, displayTitle, fmtTime, fmtDateTime, toDateInput, typeLabel, fileLinksOf, type Item } from "../api";
  import { expandItems, recurrenceLabel } from "../recurrence";
  import { fieldBadges } from "../fields.svelte";
  import { trashItems } from "../trash";
  import DeleteButton from "../DeleteButton.svelte";
  import EditButton from "../EditButton.svelte";
  import FileLinkChips from "../FileLinkChips.svelte";
  import { openEdit, rowDetail } from "../panel.svelte";
  import ItemTimeInfo from "../ItemTimeInfo.svelte";
  import { holidayLabelToday } from "../holidays.svelte";
  import { t, i18n } from "../i18n";

  let { dataVersion = 0 } = $props();

  let events = $state<Item[]>([]);
  let tasks = $state<Item[]>([]);
  let activity = $state<Item[]>([]);
  let error = $state("");
  /** 今日活动折叠（默认收起：今天页先看安排，活动是辅助信息） */
  let activityOpen = $state(false);
  /** 标签过滤（"" = 全部）：今日安排 / 今日活动统一生效 */
  let tagFilter = $state("");

  /** 标签下拉选项：今日加载条目中出现过的标签 */
  let allTags = $derived.by(() => {
    const set = new Set<string>();
    for (const it of [...events, ...tasks, ...activity]) {
      for (const tg of it.tags) set.add(tg);
    }
    return [...set].sort((a, b) => a.localeCompare(b, i18n.locale === "en" ? "en" : "zh"));
  });
  const matchTag = (it: Item) => !tagFilter || it.tags.includes(tagFilter);

  /** 进行中 / 下一个标记的基准时刻（30 秒自刷） */
  let now = $state(new Date());
  let todayStart = $derived(new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime());

  const isOngoing = (it: Item) => {
    if (!it.start_at) return false;
    const s = Date.parse(it.start_at);
    const e = it.end_at ? Date.parse(it.end_at) : s;
    return s <= now.getTime() && now.getTime() < e;
  };
  const isOverdue = (it: Item) => !!it.due_at && Date.parse(it.due_at) < todayStart;
  /** 已完成沉底 + 时间升序 */
  const bySchedule = (a: Item, b: Item) => {
    const da = a.status === "done" ? 1 : 0;
    const db = b.status === "done" ? 1 : 0;
    if (da !== db) return da - db;
    return Date.parse(a.start_at ?? a.due_at ?? "") - Date.parse(b.start_at ?? b.due_at ?? "");
  };
  /** 下一个未开始的日程 id（仅一个） */
  let nextId = $derived.by(() => {
    const nowMs = now.getTime();
    const upcoming = visibleEvents
      .filter((i) => i.status !== "done" && i.start_at && Date.parse(i.start_at) > nowMs)
      .sort((a, b) => Date.parse(a.start_at!) - Date.parse(b.start_at!));
    return upcoming[0]?.id ?? null;
  });

  let dayLabel = $derived(
    new Date().toLocaleDateString(i18n.locale === "zh" ? "zh-CN" : "en-US", {
      month: "long",
      day: "numeric",
      weekday: "long",
    }),
  );

  let visibleEvents = $derived(events.filter(matchTag));
  let visibleTasks = $derived(tasks.filter(matchTag));
  // 已完成沉底（Notion 式）：进行中的事保持在视野顶部
  let sortedEvents = $derived([...visibleEvents].sort(bySchedule));
  let sortedTasks = $derived([...visibleTasks].sort(bySchedule));
  // 今日活动与今日安排去重：同一条目不在两个列表重复出现
  let shownIds = $derived(new Set([...visibleEvents, ...visibleTasks].map((i) => i.id)));
  let visibleActivity = $derived(
    activity.filter((i) => !shownIds.has(i.id) && matchTag(i)),
  );

  onMount(() => {
    const timer = setInterval(() => (now = new Date()), 30_000);
    return () => clearInterval(timer);
  });

  async function load() {
    try {
      const now = new Date();
      const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      const end = new Date(start.getTime() + 86400_000 - 1);
      // 安排按字段过滤：任何类型只要有 start_at 落在今天即入列
      //（带开始时间的待办同样上"日历"）；另取到期/逾期待办，按 ID 去重合并。
      // 窗口查询 + 重复展开：重复日程锚点不在今天、发生在今天也会入列
      const [windowItems, dueTasks, activityItems] = await Promise.all([
        api.listItemsWindow(start.toISOString(), end.toISOString()),
        api.tasksView("today"),
        api.listItems({ changed_on: toDateInput(now), order: "desc", limit: 200 }),
      ]);
      const expanded = expandItems(windowItems, start, end);
      // 分组：日程 = 今天的日程（含重复展开）；待办 = 带开始时间的待办 ∪
      // 今天到期/逾期待办（tasksView），按 ID 去重（同一条目两边都命中只留一份）
      const evs = expanded.filter((i) => i.type === "event");
      const startedTasks = expanded.filter((i) => i.type === "task" && i.start_at);
      const startedIds = new Set(startedTasks.map((i) => i.id));
      events = evs;
      tasks = [...startedTasks, ...dueTasks.filter((it) => !startedIds.has(it.id))];
      activity = activityItems;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function toggle(item: Item) {
    if (item.status === "done") {
      await api.updateItem(item.id, { status: "todo" });
    } else {
      await api.completeTask(item.id);
    }
    await load();
  }

  $effect(() => {
    dataVersion;
    load();
  });
</script>

<h1>
  {t("common.today")} · {dayLabel}{#if holidayLabelToday()}<span class="holiday">{holidayLabelToday()}</span>{/if}
  <label class="tag-filter" title={t("calendar.tag_filter")}>
    <select data-testid="today-tag-filter" bind:value={tagFilter}>
      <option value="">{t("calendar.tag_all")}</option>
      {#each allTags as tg (tg)}<option value={tg}>#{tg}</option>{/each}
    </select>
  </label>
</h1>
{#if error}<p class="error">{error}</p>{/if}

<section>
  <h2>{t("today.plan_title")}</h2>

  {#if visibleEvents.length === 0 && visibleTasks.length === 0}
    <div class="empty">{t("today.empty")} <kbd>Ctrl+N</kbd> {t("today.empty_after")}</div>
  {:else}
    {#if sortedEvents.length}
      <h3><span>{t("type.event")}</span><span class="cnt">{sortedEvents.length}</span></h3>
      <ul class="rows">
        {#each sortedEvents as ev (ev.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li class:ongoing={isOngoing(ev)} onclick={rowDetail(ev)}>
            <span class="time">{fmtTime(ev.start_at)}–{fmtTime(ev.end_at)}</span>
            <span class="title">{displayTitle(ev)}</span>
            {#if isOngoing(ev)}<span class="badge live">{t("today.ongoing")}</span>{/if}
            {#if ev.id === nextId}<span class="badge next">{t("today.next")}</span>{/if}
            {#if ev.recurrence}<span class="tag">🔁 {recurrenceLabel(ev.recurrence)}</span>{/if}
            {#each ev.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
            <ItemTimeInfo item={ev} />
            <FileLinkChips links={fileLinksOf(ev)} />
            <EditButton onedit={() => openEdit(ev)} />
            <DeleteButton onconfirm={() => void trashItems([ev.id])} />
          </li>
        {/each}
      </ul>
    {/if}
    {#if sortedTasks.length}
      <h3><span>{t("type.task")}</span><span class="cnt">{sortedTasks.length}</span></h3>
      <ul class="rows">
        {#each sortedTasks as task (task.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li class:done={task.status === "done"} class:overdue={isOverdue(task) && task.status !== "done"} onclick={rowDetail(task)}>
            <input type="checkbox" checked={task.status === "done"} onchange={() => toggle(task)} />
            {#if task.start_at}
              <span class="time">{fmtTime(task.start_at)}</span>
            {:else}
              <span class="time">{t("type.task")}</span>
            {/if}
            <span class="title">{displayTitle(task)}</span>
            {#if task.status !== "done" && isOverdue(task)}
              <span class="badge od" title={t("today.overdue_tip", { time: fmtDateTime(task.due_at) })}>{t("today.overdue")}</span>
              <span class="time od-time">{fmtDateTime(task.due_at)}</span>
            {:else if task.due_at}
              <span class="time">{t("today.due_time", { time: fmtTime(task.due_at) })}</span>
            {/if}
            {#if task.extra["fd_priority"] === "高"}<span class="pri">{t("today.priority_high")}</span>{/if}
            {#each task.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
            <ItemTimeInfo item={task} />
            <FileLinkChips links={fileLinksOf(task)} />
            <EditButton onedit={() => openEdit(task)} />
            <DeleteButton onconfirm={() => void trashItems([task.id])} />
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>

<section>
  <h2>
    <button
      class="fold"
      data-testid="today-activity-fold"
      aria-expanded={activityOpen}
      onclick={() => (activityOpen = !activityOpen)}
    >
      <span class="chev">{activityOpen ? "▾" : "▸"}</span>{t("today.activity_title", { n: visibleActivity.length })}
    </button>
  </h2>
  {#if activityOpen}
    {#if visibleActivity.length === 0}
      <div class="empty">{t("today.activity_empty")}</div>
    {:else}
      <ul class="rows">
        {#each visibleActivity as it (it.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li onclick={rowDetail(it)}>
            <span class="kind">{typeLabel(it.type)}</span>
            <span class="change">{it.updated_at !== it.created_at ? t("today.changed") : t("today.created")}</span>
            <span class="title">{it.title}</span>
            {#each fieldBadges(it).slice(0, 4) as b (b.name)}
              <span class="value" class:deleted={b.deleted}>{b.text}</span>
            {/each}
            {#each it.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
            <ItemTimeInfo item={it} />
            <FileLinkChips links={fileLinksOf(it)} />
            <EditButton onedit={() => openEdit(it)} />
            <DeleteButton onconfirm={() => void trashItems([it.id])} />
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</section>


<style>
  h1 {
    margin: 0 0 16px;
    font-size: 20px;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .tag-filter {
    margin-left: auto;
    align-self: center;
    font-size: 13px;
    color: var(--text-dim);
  }

  .tag-filter select {
    max-width: 140px;
    font-size: 13px;
  }

  .holiday {
    margin-left: 12px;
    font-size: 13px;
    font-weight: 600;
    color: var(--type-log);
    background: color-mix(in srgb, var(--type-log) 14%, transparent);
    border-radius: 999px;
    padding: 3px 12px;
    vertical-align: middle;
  }

  h2 {
    font-size: 14px;
    color: var(--text-dim);
    margin: 22px 0 8px;
  }

  /* 安排分组小标题（日程 / 待办） */
  h3 {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-dim);
    margin: 14px 0 6px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .cnt {
    font-weight: 400;
    color: color-mix(in srgb, var(--text-dim) 70%, transparent);
    font-variant-numeric: tabular-nums;
  }

  /* 今日活动折叠头：整行可点，箭头指示开合 */
  .fold {
    border: none;
    background: transparent;
    padding: 0;
    color: inherit;
    font: inherit;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .fold:hover {
    color: var(--text);
  }

  .chev {
    width: 12px;
    font-size: 11px;
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
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 14px;
    cursor: pointer;
  }

  .rows li.done .title {
    text-decoration: line-through;
    color: var(--text-dim);
  }

  .time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    min-width: 110px;
    font-size: 13px;
  }

  .time.od-time {
    min-width: 0;
    color: var(--danger);
  }

  /* 标题是行内最重要信息：按自然宽度参与布局、收缩率压到 0.1，
     溢出时 tag/附件字段先收缩截断 */
  .title {
    flex: 1 0.1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tag,
  .value {
    flex-shrink: 10;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* 进行中 / 下一个 / 逾期 状态徽标 */
  .badge {
    flex-shrink: 0;
    font-size: 11px;
    line-height: 1;
    padding: 3px 8px;
    border-radius: 999px;
    font-weight: 600;
  }

  .badge.live {
    color: var(--accent-fg);
    background: var(--accent);
  }

  .badge.next {
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .badge.od {
    color: var(--accent-fg);
    background: var(--danger);
  }

  li.ongoing {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
    background: color-mix(in srgb, var(--accent) 7%, var(--card));
  }

  li.overdue:not(.done) .title {
    color: var(--danger);
  }

  .kind {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 6px;
  }

  .kind {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 6px;
  }

  .change {
    font-size: 12px;
    color: var(--accent);
    min-width: 30px;
  }

  .value {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }

  .value.deleted {
    font-weight: 400;
    opacity: 0.55;
  }

  .pri {
    color: var(--danger);
    font-size: 12px;
    border: 1px solid var(--danger);
    border-radius: 4px;
    padding: 0 5px;
  }

  .error {
    color: var(--danger);
  }
</style>

<script lang="ts">
  /** 需求 §6：待办页（FILTER-SPEC §9）：四 tab = 四个内置视图，用户新建视图
   *  并列出现在 tab 行；工具条共享（筛选 / 排序 / 另存为）。
   *  SPRINT-SPEC §4：多选批量操作（完成 / 改期 / 删除 + 一键撤销）保留。 */
  import { api, displayTitle, fmtDate, type FieldDef, type Item, type ItemStatus, type ViewDef, type ViewResult } from "../api";
  import { recurrenceLabel } from "../recurrence";
  import { deletions } from "../deletion.svelte";
  import { toast } from "../toast.svelte";
  import DeleteButton from "../DeleteButton.svelte";
  import EditButton from "../EditButton.svelte";
  import { openEdit, rowDetail } from "../panel.svelte";
  import ItemTimeInfo from "../ItemTimeInfo.svelte";
  import ViewToolbar from "../viewmodel/ViewToolbar.svelte";
  import { VIEW_TASKS_DONE, VIEW_TASKS_TODAY } from "../viewIds";
  import { t } from "../i18n";

  let { dataVersion = 0, clockVersion = 0 } = $props();

  let views = $state<ViewDef[]>([]);
  let activeId = $state<string>(VIEW_TASKS_TODAY);
  let result = $state<ViewResult | null>(null);
  let fields = $state<FieldDef[]>([]);
  let templates = $state<{ id: string; name: string }[]>([]);
  let error = $state("");

  // ---- 批量操作状态 ----
  let multi = $state(false);
  let sel = $state<string[]>([]);
  let reschedOpen = $state(false);
  let customDate = $state("");

  let items = $derived(result?.items ?? []);
  let visibleItems = $derived(items.filter((i) => !deletions.pendingIds.includes(i.id)));
  let selectedItems = $derived(visibleItems.filter((i) => sel.includes(i.id)));

  async function load() {
    try {
      const [vs, flds, tpls] = await Promise.all([
        api.viewList("tasks"),
        api.listAllFieldDefs(),
        api.listTemplates(),
      ]);
      views = vs;
      fields = flds;
      templates = tpls.map((tpl) => ({ id: tpl.id, name: tpl.name }));
      if (!views.some((v) => v.id === activeId)) {
        activeId = views.find((v) => v.builtin)?.id ?? VIEW_TASKS_TODAY;
      }
      result = await api.queryView(activeId);
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  async function complete(item: Item, e?: Event) {
    // 重复待办完成 = 推进下一期，语义仍是「未完成」：先把原生点击留在勾选态的
    // 勾选框复位，否则视觉上已勾选，再点一次会误推进两期
    if (e) (e.currentTarget as HTMLInputElement).checked = false;
    const advanced = !!item.recurrence;
    await api.completeTask(item.id);
    if (advanced) toast.show(t("tasks.advanced"));
    await load();
  }

  async function setStatus(item: Item, status: ItemStatus) {
    await api.updateItem(item.id, { status });
    await load();
  }

  function toggleSel(id: string) {
    sel = sel.includes(id) ? sel.filter((x) => x !== id) : [...sel, id];
  }
  function exitMulti() {
    multi = false;
    sel = [];
    reschedOpen = false;
  }

  async function batchComplete() {
    const targets = selectedItems.filter((i) => i.status !== "done");
    for (const it of targets) {
      try {
        await api.completeTask(it.id);
      } catch (e) {
        console.error(`完成 ${it.id} 失败`, e);
      }
    }
    toast.show(t("tasks.batch_done", { n: targets.length }));
    sel = [];
    await load();
  }

  /** 批量改期：只平移 due 的日期部分、保留时刻（@due 提醒自动跟随）；可整体撤销 */
  async function batchReschedule(target: "tomorrow" | "plus1" | "nextMonday" | "custom") {
    const today = new Date();
    const undoDues: { id: string; from: string }[] = [];
    for (const it of selectedItems) {
      if (!it.due_at) continue;
      undoDues.push({ id: it.id, from: it.due_at });
      const due = new Date(it.due_at);
      let nd: Date;
      if (target === "plus1") {
        nd = new Date(due);
        nd.setDate(nd.getDate() + 1);
      } else if (target === "nextMonday") {
        nd = new Date(today.getFullYear(), today.getMonth(), today.getDate());
        nd.setDate(nd.getDate() + (((1 - nd.getDay() + 7) % 7) || 7));
        nd.setHours(due.getHours(), due.getMinutes(), 0, 0);
      } else if (target === "custom" && customDate) {
        const [y, m, d] = customDate.split("-").map(Number);
        nd = new Date(y, m - 1, d, due.getHours(), due.getMinutes(), 0, 0);
      } else {
        // 明天：日期取今天 +1，时刻保留
        nd = new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1);
        nd.setHours(due.getHours(), due.getMinutes(), 0, 0);
      }
      try {
        await api.updateItem(it.id, { due_at: nd.toISOString() });
      } catch (e) {
        console.error(`改期 ${it.id} 失败`, e);
      }
    }
    toast.show(t("tasks.batch_rescheduled", { n: undoDues.length }), {
      ms: 6000,
      action: {
        label: t("common.undo"),
        run: async () => {
          for (const c of undoDues) {
            try {
              await api.updateItem(c.id, { due_at: c.from });
            } catch (e) {
              console.error(`撤销改期 ${c.id} 失败`, e);
            }
          }
          await load();
        },
      },
    });
    sel = [];
    reschedOpen = false;
    await load();
  }

  function batchDelete() {
    // 撤销反馈由 UndoToast 承担（含「全部撤销」入口），不再发全局 toast 避免重叠
    deletions.requestMany(selectedItems);
    sel = [];
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && multi) exitMulti();
    if (multi && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "a") {
      e.preventDefault();
      sel = visibleItems.map((i) => i.id);
    }
  }

  function switchView(id: string) {
    activeId = id;
    exitMulti();
    void load();
  }

  $effect(() => {
    dataVersion;
    clockVersion;
    load();
  });
</script>

<svelte:window onkeydown={onKeydown} />

<h1>
  {t("tasks.title")}
  {#if activeId !== VIEW_TASKS_DONE}
    <button
      class="multi-btn"
      class:active={multi}
      onclick={() => (multi ? exitMulti() : (multi = true))}
    >
      {multi ? t("tasks.multi_exit") : t("tasks.multi")}
    </button>
  {/if}
</h1>

<ViewToolbar
  panel="tasks"
  {views}
  activeId={activeId}
  {result}
  {fields}
  {templates}
  switcher="tabs"
  onswitch={switchView}
  onchanged={() => void load()}
/>

{#if error}<p class="error">{error}</p>{/if}

{#if visibleItems.length === 0}
  <div class="empty">{t("tasks.empty")}</div>
{:else}
  <ul class="rows">
    {#each visibleItems as item (item.id)}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li class:done={item.status === "done"} class:picked={multi && sel.includes(item.id)} onclick={rowDetail(item)}>
        {#if multi}
          <input type="checkbox" checked={sel.includes(item.id)} onclick={(e) => e.stopPropagation()} onchange={() => toggleSel(item.id)} />
        {:else if item.status === "done"}
          <input
            type="checkbox"
            checked
            onchange={() => setStatus(item, "todo")}
          />
        {:else}
          <input
            type="checkbox"
            checked={false}
            title={item.recurrence ? t("tasks.recurrence_tip", { rule: recurrenceLabel(item.recurrence) }) : t("tasks.mark_done")}
            onchange={(e) => complete(item, e)}
          />
        {/if}
        <span class="title">{displayTitle(item)}</span>
        {#if item.recurrence}<span class="tag">🔁 {recurrenceLabel(item.recurrence)}</span>{/if}
        {#if item.extra["fd_priority"] === "高"}<span class="pri high">{t("tasks.priority_high")}</span>{/if}
        {#if item.due_at}<span class="time">{t("tasks.due", { date: fmtDate(item.due_at) })}</span>{/if}
        {#each item.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
        <ItemTimeInfo item={item} />
        {#if !multi}
          <EditButton onedit={() => openEdit(item)} />
          <DeleteButton onconfirm={() => deletions.request(item)} />
        {/if}
      </li>
    {/each}
  </ul>
{/if}

{#if multi && sel.length}
  <div class="batch-bar">
    <span class="count">{t("tasks.selected", { n: sel.length })}</span>
    <button onclick={() => (sel = visibleItems.map((i) => i.id))}>{t("tasks.select_all")}</button>
    <button class="primary" onclick={batchComplete}>{t("tasks.complete")}</button>
    <span class="resched">
      <button onclick={() => (reschedOpen = !reschedOpen)}>{t("tasks.reschedule")}</button>
      {#if reschedOpen}
        <span class="menu">
          <button onclick={() => batchReschedule("tomorrow")}>{t("tasks.tomorrow")}</button>
          <button onclick={() => batchReschedule("plus1")}>{t("tasks.plus1")}</button>
          <button onclick={() => batchReschedule("nextMonday")}>{t("tasks.next_monday")}</button>
          <span class="custom">
            <input type="date" bind:value={customDate} />
            <button disabled={!customDate} onclick={() => batchReschedule("custom")}>{t("common.ok")}</button>
          </span>
        </span>
      {/if}
    </span>
    <button class="danger" onclick={batchDelete}>{t("common.delete")}</button>
    <button onclick={exitMulti}>{t("common.cancel")}</button>
  </div>
{/if}


<style>
  h1 {
    margin: 0 0 16px;
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .multi-btn {
    font-size: 13px;
    padding: 3px 12px;
    border-radius: 999px;
  }

  .multi-btn.active {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
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

  .rows li.picked {
    border-color: var(--accent);
  }

  .rows li.done .title {
    text-decoration: line-through;
    color: var(--text-dim);
  }

  .title {
    flex: 1;
  }

  .time {
    color: var(--text-dim);
    font-size: 13px;
  }

  .pri.high {
    color: var(--danger);
    font-size: 12px;
    border: 1px solid var(--danger);
    border-radius: 4px;
    padding: 0 5px;
  }

  .batch-bar {
    position: fixed;
    bottom: 20px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 8px 14px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    z-index: 80;
  }

  .count {
    color: var(--text-dim);
    font-size: 13px;
    margin-right: 4px;
  }

  .batch-bar .danger {
    color: var(--danger);
    border-color: var(--danger);
  }

  .resched {
    position: relative;
    display: inline-flex;
  }

  .menu {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 8px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    min-width: 180px;
  }

  .custom {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .error {
    color: var(--danger);
  }
</style>

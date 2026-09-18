<script lang="ts">
  /**
   * 搜索页（FILTER-SPEC §9 / §10）：关键词是运行时输入，编译为 OR 子树并入
   * 内置视图「全部类型」的 AST 合取——全引擎一条求值路径。
   * 类型 / 标签 / 日期筛选经工具条（共享筛选编辑器）表达。
   */
  import { anchorTime, api, displayTitle, fmtDateTime, typeLabel, type FieldDef, type Item, type ViewDef, type ViewResult, fileLinksOf } from "../api";
  import { deletions } from "../deletion.svelte";
  import DeleteButton from "../DeleteButton.svelte";
  import EditButton from "../EditButton.svelte";
  import FileLinkChips from "../FileLinkChips.svelte";
  import { openEdit, rowDetail } from "../panel.svelte";
  import ItemTimeInfo from "../ItemTimeInfo.svelte";
  import ViewToolbar from "../viewmodel/ViewToolbar.svelte";
  import { VIEW_SEARCH_ALL } from "../viewIds";
  import { t } from "../i18n";

  let { dataVersion = 0, clockVersion = 0, query = "" } = $props();

  // Ctrl+K 预填为一次性初值，不跟随后续变化
  // svelte-ignore state_referenced_locally
  let q = $state(query);
  let views = $state<ViewDef[]>([]);
  let activeId = $state<string>(VIEW_SEARCH_ALL);
  let result = $state<ViewResult | null>(null);
  let fields = $state<FieldDef[]>([]);
  let templates = $state<{ id: string; name: string }[]>([]);
  let searched = $state(false);
  let error = $state("");

  let hits = $derived(result?.items ?? []);
  let visibleHits = $derived(hits.filter((h) => !deletions.pendingIds.includes(h.id)));

  async function loadViews() {
    try {
      const [vs, flds, tpls] = await Promise.all([
        api.viewList("search"),
        api.listAllFieldDefs(),
        api.listTemplates(),
      ]);
      views = vs;
      fields = flds;
      templates = tpls.map((tpl) => ({ id: tpl.id, name: tpl.name }));
      if (!views.some((v) => v.id === activeId)) {
        activeId = views.find((v) => v.builtin)?.id ?? VIEW_SEARCH_ALL;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function search() {
    if (!q.trim()) {
      result = null;
      searched = false;
      return;
    }
    try {
      result = await api.queryView(activeId, q);
      searched = true;
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  function switchView(id: string) {
    activeId = id;
    if (searched) void search();
  }

  function onchanged() {
    void loadViews();
    if (searched) void search();
  }

  $effect(() => {
    dataVersion;
    clockVersion;
    loadViews();
    if (searched) search();
  });
</script>

<h1>{t("common.search")}</h1>

<form class="bar" onsubmit={(e) => { e.preventDefault(); search(); }}>
  <input
    id="global-search"
    placeholder={t("search.placeholder")}
    bind:value={q}
    oninput={() => search()}
  />
  <button class="primary" type="submit">{t("common.search")}</button>
</form>

<ViewToolbar
  panel="search"
  {views}
  activeId={activeId}
  {result}
  {fields}
  {templates}
  switcher="dropdown"
  onswitch={switchView}
  onchanged={onchanged}
/>

{#if error}<p class="error">{error}</p>{/if}

{#if searched && visibleHits.length === 0}
  <div class="empty">{t("search.no_results")}</div>
{:else if visibleHits.length}
  <ul class="rows">
    {#each visibleHits as it (it.id)}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <li onclick={rowDetail(it)}>
        <span class="kind">{typeLabel(it.type)}</span>
        <span class="title">{displayTitle(it)}</span>
        {#if result?.matched?.[it.id]?.some((m) => m !== "title")}
          <span class="match">{result.matched[it.id].join("·")}</span>
        {/if}
        {#if it.note}<span class="note">{it.note.slice(0, 40)}</span>{/if}
        <span class="time">{fmtDateTime(anchorTime(it) ?? it.created_at)}</span>
        {#each it.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
        {#if it.attachments.length}<span class="att">🖼 {it.attachments.length}</span>{/if}
        <ItemTimeInfo item={it} />
          <FileLinkChips links={fileLinksOf(it)} />
        <EditButton onedit={() => openEdit(it)} />
        <DeleteButton onconfirm={() => deletions.request(it)} />
      </li>
    {/each}
  </ul>
{/if}


<style>
  h1 {
    margin: 0 0 16px;
  }

  .bar {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }

  .bar input {
    flex: 1;
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 14px;
    cursor: pointer;
  }

  .kind {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 6px;
  }

  .title {
    font-weight: 600;
  }

  .note {
    color: var(--text-dim);
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    font-size: 13px;
  }

  .att {
    color: var(--text-dim);
  }

  .match {
    font-size: 11px;
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 4px;
    padding: 0 5px;
  }

  .error {
    color: var(--danger);
  }
</style>

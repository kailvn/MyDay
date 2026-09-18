<script lang="ts">
  /**
   * 需求 §7：时间线（按发生时间分组倒序）+ 模板快速记录。
   * 模板按钮 = 已钉选的记录类模板；defaults 双命名空间：
   * 有字段 id 默认值的模板可一键记录（列/字段默认由 core 合并），
   * 只有列默认（如仅 title）的模板改为打开面板现场补值。
   * 按钮可增删：「＋ 按钮」就地选模板，按钮上的 × 移除（不删模板）。
   */
  import { api, displayTitle, fmtTime, type FieldDef, type Item, type Template, type ViewDef, type ViewResult, fileLinksOf } from "../api";
  import { fieldBadges, fieldDefMap } from "../fields.svelte";
  import { deletions } from "../deletion.svelte";
  import DeleteButton from "../DeleteButton.svelte";
  import EditButton from "../EditButton.svelte";
  import FileLinkChips from "../FileLinkChips.svelte";
  import { openCreate, openEdit, rowDetail } from "../panel.svelte";
  import ItemTimeInfo from "../ItemTimeInfo.svelte";
  import ViewToolbar from "../viewmodel/ViewToolbar.svelte";
  import { VIEW_LOGS_TIMELINE } from "../viewIds";
  import { t } from "../i18n";

  let { dataVersion = 0, clockVersion = 0 } = $props();

  let logs = $state<Item[]>([]);
  let templates = $state<Template[]>([]);
  let views = $state<ViewDef[]>([]);
  let activeId = $state<string>(VIEW_LOGS_TIMELINE);
  let result = $state<ViewResult | null>(null);
  let fields = $state<FieldDef[]>([]);
  let error = $state("");

  // ---- 记录页按钮 = 已钉选的记录类模板 ------------------------------------
  // 新建 / 编辑模板统一在 设置 → 模板管理；这里只做「添加按钮 / 移除按钮」
  let picking = $state(false);
  let tplError = $state("");

  let pageButtons = $derived(
    templates.filter((tpl) => tpl.item_type === "log" && tpl.pinned),
  );
  let unpinned = $derived(
    templates.filter((tpl) => tpl.item_type === "log" && !tpl.pinned),
  );

  async function togglePin(tpl: Template) {
    try {
      await api.setTemplatePinned(tpl.id, !tpl.pinned);
      await load();
    } catch (e) {
      tplError = String(e);
    }
  }

  type DayGroup = { day: string; items: Item[] };

  let visibleLogs = $derived(logs.filter((i) => !deletions.pendingIds.includes(i.id)));

  // 时间线分组：来自视图求值结果（组序 = 组键倒序，组内按 sort，引擎定）
  let groups = $derived.by<DayGroup[]>(() => {
    if (result?.groups) {
      return result.groups.map((g) => ({
        day: g.key ?? "",
        items: g.items.filter((i) => !deletions.pendingIds.includes(i.id)),
      }));
    }
    return [];
  });

  async function load() {
    try {
      const [vs, flds, tpls] = await Promise.all([
        api.viewList("logs"),
        api.listAllFieldDefs(),
        api.listTemplates(),
      ]);
      views = vs;
      fields = flds;
      templates = tpls;
      if (!views.some((v) => v.id === activeId)) {
        activeId = views.find((v) => v.builtin)?.id ?? VIEW_LOGS_TIMELINE;
      }
      const r = await api.queryView(activeId);
      result = r;
      logs = r.items ?? [];
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  function switchView(id: string) {
    activeId = id;
    void load();
  }

  /** defaults 中字段 id 键的数量（有 = 可一键记录） */
  function fieldDefaultKeys(tpl: Template): string[] {
    const byId = fieldDefMap();
    return Object.keys(tpl.defaults ?? {}).filter((k) => byId.has(k));
  }

  /**
   * 点模板：带字段默认值的一键记录（core 合并列/字段默认与标签）；
   * 只有列默认的（如"体重，现场填"）改为打开面板补数值，不静默生成空记录。
   */
  function quickLog(tpl: Template) {
    if (fieldDefaultKeys(tpl).length === 0) {
      openCreate({
        item_type: "log",
        title: (tpl.defaults?.["title"] as string) ?? tpl.name,
        presetTemplateId: tpl.id,
        presetTags: tpl.tag ? [tpl.tag] : [],
      });
      return;
    }
    void (async () => {
      try {
        await api.addItem({
          item_type: "log",
          title: (tpl.defaults?.["title"] as string) ?? tpl.name,
          template_id: tpl.id,
          tags: tpl.tag ? [tpl.tag] : [],
        });
        await load();
      } catch (e) {
        tplError = String(e);
      }
    })();
  }

  $effect(() => {
    dataVersion;
    clockVersion;
    load();
  });
</script>

<h1>{t("logs.title")}</h1>

{#if error}<p class="error">{error}</p>{/if}
{#if tplError}<p class="error">{tplError}</p>{/if}

<div class="templates">
  {#each pageButtons as tpl (tpl.id)}
    <span class="tpl-chip">
      <button class="tpl-btn" onclick={() => quickLog(tpl)}>
        {#if tpl.icon}<span class="tpl-icon">{tpl.icon}</span>{/if}{tpl.name}{#if tpl.note}<span class="tpl-note">{tpl.note}</span>{/if}
      </button>
      <button
        class="x"
        title={t("logs.remove_btn_tip")}
        onclick={() => togglePin(tpl)}>×</button
      >
    </span>
  {/each}
  <button class="ghost add-tpl" onclick={() => (picking = !picking)}>{t("logs.add_button")}</button>
</div>

{#if picking}
  <div class="picker">
    <p class="hint">{t("logs.picker_hint")}</p>
    {#if unpinned.length === 0}
      <p class="dim">{t("logs.picker_empty")}</p>
    {:else}
      <div class="templates">
        {#each unpinned as tpl (tpl.id)}
          <span class="tpl-chip">
            <button class="tpl-btn" onclick={() => togglePin(tpl)}>
              {#if tpl.icon}<span class="tpl-icon">{tpl.icon}</span>{/if}{tpl.name}{#if tpl.note}<span class="tpl-note">{tpl.note}</span>{/if}
            </button>
            <button class="x" title={t("logs.add_as_button")} onclick={() => togglePin(tpl)}>＋</button>
          </span>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<ViewToolbar
  panel="logs"
  {views}
  activeId={activeId}
  {result}
  {fields}
  switcher="dropdown"
  onswitch={switchView}
  onchanged={() => void load()}
/>

{#if groups.length === 0}
  <div class="empty">{t("logs.empty")}</div>
{:else}
      {#each groups as g (g.day)}
        <section>
          <h2>{g.day}</h2>
          <ul>
            {#each g.items as l (l.id)}
              <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <li onclick={rowDetail(l)}>
                <span class="time">{fmtTime(l.occurred_at)}</span>
                <span class="title">{displayTitle(l)}</span>
                {#each fieldBadges(l) as b (b.name)}
                  <span class="value" class:deleted={b.deleted}>{b.text}</span>
                {/each}
                {#each l.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
                {#if l.attachments.length}
                  <span class="att">🖼 {l.attachments.length}</span>
                {/if}
                <ItemTimeInfo item={l} />
              <FileLinkChips links={fileLinksOf(l)} />
                <EditButton onedit={() => openEdit(l)} />
                <DeleteButton onconfirm={() => deletions.request(l)} />
              </li>
            {/each}
          </ul>
        </section>
      {/each}
{/if}


<style>
  h1 {
    margin: 0 0 16px;
  }

  h2 {
    font-size: 13px;
    color: var(--text-dim);
    margin: 20px 0 8px;
  }

  .templates {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 8px;
    align-items: center;
  }

  .templates button {
    border-radius: 999px;
  }

  .tpl-chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding-right: 4px;
  }

  .tpl-chip .x {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 2px 6px;
    font-size: 13px;
  }

  .tpl-chip .x:hover {
    color: var(--danger);
  }

  .add-tpl {
    font-size: 12px;
  }

  ul {
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

  .time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    min-width: 52px;
  }

  .title {
    flex: 1;
  }

  .value {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .value.deleted {
    font-weight: 400;
    opacity: 0.55;
  }

  .att {
    color: var(--text-dim);
    font-size: 13px;
  }

  .tpl-note {
    color: var(--text-dim);
    font-size: 11px;
    margin-left: 4px;
  }

  .error {
    color: var(--danger);
  }
</style>

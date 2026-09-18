<script lang="ts">
  /**
   * 统计页（v2.1 容器模型）：页面 = 容器列表，容器 = 布局（水平 / 垂直）+ 挂件列表。
   * 预置容器 = 开库时替用户建好的普通容器，与自建容器完全同权——可编辑、可重命名、
   * 可删除；「恢复默认统计页」= 完全重置（清掉全部容器铺回预设，页头按钮 + 确认）。
   * 挂件编辑 = 手机小组件式：进容器的「编辑挂件」模式后，卡片可点（打开编辑器）、
   * 右上角 × 移除、末尾 ＋ 空位新增；「完成」退出。全部卡片走通用 WidgetView。
   * 挂件窗口在挂件编辑里各自设置（页面没有全局范围档）。
   */
  import { api, type ContainerConfig, type FieldDef, type StatsPageResult, type ViewDef, type WidgetConfig, type WidgetResult } from "../api";
  import { toast } from "../toast.svelte";
  import WidgetEditor from "../viewmodel/WidgetEditor.svelte";
  import WidgetView from "../viewmodel/WidgetView.svelte";
  import { t, q } from "../i18n";

  let { dataVersion = 0, clockVersion = 0 } = $props();

  let page = $state<StatsPageResult | null>(null);
  let views = $state<ViewDef[]>([]);
  let fields = $state<FieldDef[]>([]);
  let error = $state("");

  /** 处于「编辑挂件」模式的容器 */
  let editContainer = $state("");
  /** 打开中的容器菜单（数据刷新后保持） */
  let openMenu = $state("");
  /** 重命名中的容器 + 草稿名 */
  let renamingId = $state("");
  let renameDraft = $state("");

  let editorOpen = $state(false);
  let editorName = $state("");
  let editorConfig = $state<WidgetConfig | null>(null);
  /** 编辑目标容器 + 被编辑的挂件 key（null = 新增） */
  let editorTarget = $state<{ viewId: string; key: string | null } | null>(null);

  let addContainerOpen = $state(false);
  let newContainerName = $state("");
  let newContainerLayout = $state<"horizontal" | "vertical">("vertical");

  async function load() {
    try {
      const [p, vs, flds] = await Promise.all([
        api.queryStatsPage(),
        api.viewList("stats"),
        api.listAllFieldDefs(),
      ]);
      page = p;
      views = vs;
      fields = flds;
      if (!views.some((v) => v.id === editContainer)) editContainer = "";
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  const widgetsOf = (viewId: string): WidgetResult[] =>
    page?.widgets.filter((w) => w.view_id === viewId) ?? [];
  const rowOf = (viewId: string) => views.find((v) => v.id === viewId);
  const containerWidgets = (viewId: string): WidgetConfig[] =>
    ((rowOf(viewId)?.config as ContainerConfig | undefined)?.widgets ?? []).map((w) =>
      JSON.parse(JSON.stringify(w)),
    );

  async function saveContainerWidgets(viewId: string, widgets: WidgetConfig[]) {
    const row = rowOf(viewId);
    if (!row) return;
    const cfg = JSON.parse(JSON.stringify(row.config)) as ContainerConfig;
    cfg.widgets = widgets;
    await api.viewSave(viewId, cfg);
    await load();
  }

  // ---- 编辑挂件模式 ---------------------------------------------------------

  function toggleEdit(viewId: string) {
    editContainer = editContainer === viewId ? "" : viewId;
    openMenu = "";
  }

  function openAddWidget(viewId: string) {
    editorTarget = { viewId, key: null };
    editorName = "";
    editorConfig = {
      kind: "widget",
      dataset: { item_type: "log", filter: { op: "and", children: [] } },
      window: { days: 365 },
      agg: { group: { by: "time", bucket: "day", time_field: "col:occurred_at" }, metric: { fn: "count" } },
      derived: { kind: "streak", goal: { daily: 1 } },
      render: "card",
      options: { presence: false, top_n: 8 },
    };
    editorOpen = true;
  }

  function openEditWidget(w: WidgetResult) {
    const row = rowOf(w.view_id);
    if (!row) return;
    const idx = Number(w.key.split("#")[1] ?? -1);
    const list = containerWidgets(w.view_id);
    if (idx < 0 || idx >= list.length) return;
    editorTarget = { viewId: w.view_id, key: w.key };
    editorName = w.title;
    editorConfig = list[idx];
    editorOpen = true;
  }

  async function onEditorSaved(title: string, cfg: WidgetConfig) {
    cfg.title = title;
    const target = editorTarget;
    if (!target) return;
    if (target.key == null) {
      await saveContainerWidgets(target.viewId, [...containerWidgets(target.viewId), cfg]);
      toast.show(t("stats.widget_added"));
    } else {
      const idx = Number(target.key.split("#")[1] ?? -1);
      await saveContainerWidgets(
        target.viewId,
        containerWidgets(target.viewId).map((c, i) => (i === idx ? cfg : c)),
      );
    }
    editorTarget = null;
  }

  async function removeWidget(w: WidgetResult) {
    const idx = Number(w.key.split("#")[1] ?? -1);
    await saveContainerWidgets(
      w.view_id,
      containerWidgets(w.view_id).filter((_, i) => i !== idx),
    );
  }

  // ---- 容器级操作 -----------------------------------------------------------

  function startRename(viewId: string, name: string) {
    renamingId = viewId;
    renameDraft = name;
    openMenu = "";
  }

  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  async function commitRename() {
    const id = renamingId;
    const name = renameDraft.trim();
    renamingId = "";
    if (!id || !name) return;
    const row = rowOf(id);
    if (!row || row.name === name) return;
    await api.viewSave(id, JSON.parse(JSON.stringify(row.config)), name);
    await load();
  }

  async function toggleLayout(viewId: string, layout: "horizontal" | "vertical") {
    const row = rowOf(viewId);
    if (!row) return;
    const cfg = { ...(JSON.parse(JSON.stringify(row.config)) as Record<string, unknown>), layout };
    await api.viewSave(viewId, cfg);
    await load();
  }

  async function deleteContainer(viewId: string) {
    const name = rowOf(viewId)?.name ?? viewId;
    if (!confirm(t("stats.delete_confirm", { name: q(name) }))) return;
    await api.viewDelete(viewId);
    if (editContainer === viewId) editContainer = "";
    await load();
  }

  async function restoreDefaults() {
    if (!confirm(t("stats.restore_confirm"))) return;
    await api.statsRestoreDefaults();
    editContainer = "";
    toast.show(t("stats.restored"));
    await load();
  }

  async function createContainer() {
    const name = newContainerName.trim();
    if (!name) return;
    await api.viewCreate(name, "stats", {
      kind: "container",
      layout: newContainerLayout,
      widgets: [],
    });
    toast.show(t("stats.container_created"));
    addContainerOpen = false;
    newContainerName = "";
    await load();
  }

  $effect(() => {
    dataVersion;
    clockVersion;
    load();
  });
</script>

<h1>{t("stats.title")}</h1>
<div class="page-actions">
  <button class="chip" data-testid="add-container" onclick={() => (addContainerOpen = !addContainerOpen)}>{t("stats.add_container")}</button>
  <span class="spacer"></span>
  <button class="ghost" data-testid="restore-defaults" onclick={restoreDefaults}>{t("stats.restore_defaults")}</button>
</div>

{#if addContainerOpen}
  <div class="add-container" data-testid="add-container-form">
    <input placeholder={t("stats.container_name_ph")} bind:value={newContainerName} />
    <select bind:value={newContainerLayout}>
      <option value="vertical">{t("stats.layout_vertical")}</option>
      <option value="horizontal">{t("stats.layout_horizontal")}</option>
    </select>
    <button class="primary" onclick={createContainer}>{t("stats.create_container")}</button>
  </div>
{/if}

{#if error}<p class="error">{error}</p>{/if}

{#if page}
  {#each page.containers as c (c.view_id)}
    {@const editing = editContainer === c.view_id}
    <section
      data-testid="stats-container"
      data-container-id={c.view_id}
      data-layout={c.layout}
      class:editing
    >
      <header>
        {#if renamingId === c.view_id}
          <input
            class="rename-input"
            data-testid="rename-input"
            bind:value={renameDraft}
            use:autofocus
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename();
              else if (e.key === "Escape") renamingId = "";
            }}
            onblur={() => renamingId && commitRename()}
          />
        {:else}
          <h2>
            {c.name}
            <span class="layout-badge">{c.layout === "horizontal" ? t("stats.badge_h") : t("stats.badge_v")}</span>
          </h2>
        {/if}
        <span class="spacer"></span>
        <button
          class="ghost"
          data-testid="edit-widgets"
          class:primary={editing}
          onclick={() => toggleEdit(c.view_id)}
        >{editing ? t("common.done") : t("stats.edit_widgets")}</button>
        <button
          class="menu-trigger"
          aria-label={t("stats.container_menu", { name: c.name })}
          aria-expanded={openMenu === c.view_id}
          onclick={() => (openMenu = openMenu === c.view_id ? "" : c.view_id)}
        >⋯</button>
        {#if openMenu === c.view_id}
          <div class="menu-body">
            <button onclick={() => startRename(c.view_id, c.name)}>{t("common.rename")}</button>
            <button onclick={() => toggleLayout(c.view_id, c.layout === "horizontal" ? "vertical" : "horizontal")}>
              {t("stats.switch_layout", { dir: c.layout === "horizontal" ? t("stats.dir_vertical") : t("stats.dir_horizontal") })}
            </button>
            <button class="dangerish" onclick={() => deleteContainer(c.view_id)}>{t("stats.delete_container")}</button>
          </div>
        {/if}
      </header>
      <div class="widgets" class:horizontal={c.layout === "horizontal"}>
        {#each widgetsOf(c.view_id) as w (w.key)}
          <WidgetView
            widget={w}
            editMode={editing}
            onedit={openEditWidget}
            onremove={removeWidget}
          />
        {/each}
        {#if editing}
          <button class="add-tile" data-testid="add-widget-tile" onclick={() => openAddWidget(c.view_id)}>
            {t("stats.add_widget")}
          </button>
        {:else if widgetsOf(c.view_id).length === 0}
          <p class="empty">{t("stats.empty_container", { btn: q(t("stats.edit_widgets")) })}</p>
        {/if}
      </div>
    </section>
  {/each}
{/if}

{#if editorOpen && editorConfig && editorTarget}
  <WidgetEditor
    name={editorName}
    config={editorConfig}
    {fields}
    onclose={() => (editorOpen = false)}
    onsaved={onEditorSaved}
  />
{/if}

<style>
  h1 {
    margin: 0 0 12px;
  }

  .page-actions {
    display: flex;
    gap: 6px;
    margin-bottom: 14px;
    align-items: center;
  }

  .page-actions .ghost {
    font-size: 12px;
    color: var(--text-dim);
  }

  .page-actions .ghost:hover {
    color: var(--accent);
  }

  .spacer {
    flex: 1;
  }

  .add-container {
    display: flex;
    gap: 6px;
    margin: -6px 0 12px;
    align-items: center;
  }

  .add-container input {
    width: 200px;
  }

  section {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 16px;
    margin-bottom: 14px;
  }

  section.editing {
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
    position: relative;
  }

  h2 {
    font-size: 14px;
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .layout-badge {
    font-size: 11px;
    font-weight: 400;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 4px;
  }

  /* 水平容器：卡片并排（最小 220 / 最佳 250 / 最大 340），放不下自动换行；
     容器宽度封顶 ≈ 4 张（340×4 + gap×3）——宽度限制即每行最多 4 张。
     方向必须显式写 row：同元素还命中下方 .widgets 的 column 缺省 */
  .widgets.horizontal {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    gap: 10px;
    max-width: 1392px;
  }

  .widgets.horizontal > :global(.widget),
  .widgets.horizontal > .add-tile {
    flex: 1 1 250px;
    min-width: 220px;
    max-width: 340px;
  }

  .widgets {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rename-input {
    font-size: 14px;
    font-weight: 600;
    width: 220px;
    padding: 2px 8px;
  }

  .add-tile {
    min-height: 72px;
    border: 2px dashed color-mix(in srgb, var(--text) 25%, transparent);
    border-radius: 10px;
    background: transparent;
    color: var(--text-dim);
    font-size: 14px;
    cursor: pointer;
  }

  .add-tile:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  .menu-trigger {
    border: none;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    padding: 0 4px;
    font-size: 14px;
  }

  .menu-body {
    position: absolute;
    right: 0;
    top: 100%;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 180px;
    z-index: 30;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
  }

  .menu-body button {
    border: none;
    background: transparent;
    text-align: left;
    font-size: 13px;
    padding: 4px 8px;
    border-radius: 6px;
  }

  .menu-body button:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }

  .menu-body .dangerish {
    color: var(--danger);
  }

  .ghost {
    font-size: 12px;
  }

  .ghost.primary {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
  }

  .empty {
    color: var(--text-dim);
    font-size: 13px;
    margin: 0;
  }

  .error {
    color: var(--danger);
  }
</style>

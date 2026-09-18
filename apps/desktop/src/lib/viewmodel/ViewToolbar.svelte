<script lang="ts">
  /**
   * 视图工具条（FILTER-SPEC §11，参照 Anytype）：
   * 左 = 视图切换（下拉或 tab 行，内置标「默认」）+ 激活条件 chips（逐个 ×）+ 排序 chips；
   * 右 = 「筛选」「排序」按钮 + 「另存为」（用户视图另有删除）。
   * 记录 / 待办 / 搜索共用同一组件实例逻辑——一致性靠复用保证（§0 跨面板学习成本）。
   * 筛选 / 排序编辑器内嵌于此：应用即 viewSave（内置 = 写 config_user），广播刷新。
   */
  import type { FieldDef, FilterNode, Panel, ViewDef, ViewResult } from "../api";
  import { api } from "../api";
  import { q, t } from "../i18n";
  import {
    cloneConfig,
    describeCondition,
    describeSort,
    effectiveConfig,
    type FilterEditState,
  } from "../viewmodel";
  import FilterEditor from "./FilterEditor.svelte";
  import SortEditor from "./SortEditor.svelte";

  let {
    panel,
    views,
    activeId,
    result = null,
    fields,
    templates = [],
    switcher = "dropdown",
    onswitch,
    onchanged,
  }: {
    panel: Panel;
    views: ViewDef[];
    activeId: string;
    /** 当前求值结果（提供 chips 与 customized 标志） */
    result?: ViewResult | null;
    fields: FieldDef[];
    templates?: { id: string; name: string }[];
    /** 视图切换形态：下拉（记录/搜索）或 tab 行（待办） */
    switcher?: "dropdown" | "tabs";
    onswitch: (id: string) => void;
    onchanged: () => void;
  } = $props();

  let filterOpen = $state(false);
  let sortOpen = $state(false);
  let saveAsOpen = $state(false);
  let saveAsName = $state("");
  let error = $state("");

  const active = $derived(views.find((v) => v.id === activeId) ?? null);

  /** 激活条件 chips（引用 = 条件在 AST 中的位置） */
  type Chip = { text: string; ref: { kind: "top" | "sub"; index: number } };
  const chips = $derived.by<Chip[]>(() => {
    const cfg = result?.config as { dataset?: { filter?: FilterNode } } | undefined;
    const filter = cfg?.dataset?.filter;
    if (!filter || !("op" in filter)) return [];
    const out: Chip[] = [];
    filter.children.forEach((c, i) => {
      if ("op" in c) {
        c.children.forEach((sc, j) => {
          if (!("op" in sc)) {
            out.push({ text: describeCondition(sc, fields, templates), ref: { kind: "sub", index: j } });
          }
        });
      } else {
        out.push({ text: describeCondition(c, fields, templates), ref: { kind: "top", index: i } });
      }
    });
    return out;
  });

  const sortChips = $derived.by<string[]>(() => {
    const cfg = result?.config as { dataset?: { sort?: { field: string; dir: "asc" | "desc" }[] } } | undefined;
    return (cfg?.dataset?.sort ?? []).map((s) => describeSort(s, fields));
  });

  /** chip 本体可点击 = 直接打开对应编辑器（Anytype 式就地编辑）；× 才是移除 */

  async function removeChip(chip: Chip) {
    if (!result || !active) return;
    const next = cloneConfig(effectiveConfig(active));
    const filter = (next as { dataset: { filter: FilterNode } }).dataset.filter;
    if (!("op" in filter)) return;
    if (chip.ref.kind === "top") {
      filter.children.splice(chip.ref.index, 1);
    } else {
      const sub = filter.children.find((c): c is Extract<FilterNode, { op: "and" | "or" }> => "op" in c);
      if (sub) {
        sub.children.splice(chip.ref.index, 1);
        if (sub.children.length === 0) {
          filter.children.splice(filter.children.indexOf(sub), 1);
        }
      }
    }
    try {
      await api.viewSave(active.id, next);
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }

  async function saveAs() {
    const name = saveAsName.trim();
    if (!name || !active) return;
    try {
      const v = await api.viewDuplicate(active.id, name);
      saveAsOpen = false;
      saveAsName = "";
      onchanged();
      onswitch(v.id);
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteView() {
    if (!active || active.builtin) return;
    if (!confirm(t("vm.toolbar.deleteConfirm", { name: q(active.name) }))) return;
    try {
      await api.viewDelete(active.id);
      const fallback = views.find((v) => v.builtin && v.id !== active.id);
      onchanged();
      if (fallback) onswitch(fallback.id);
    } catch (e) {
      error = String(e);
    }
  }
</script>

{#if error}<p class="error">{error}</p>{/if}

<div class="toolbar" data-testid="view-toolbar">
  {#if switcher === "tabs"}
    <div class="tabs" role="tablist">
      {#each views as v (v.id)}
        <button
          role="tab"
          aria-selected={v.id === activeId}
          class:active={v.id === activeId}
          onclick={() => onswitch(v.id)}
        >{v.name}{v.builtin ? t("vm.toolbar.builtinSuffix") : ""}</button>
      {/each}
    </div>
  {:else}
    <select class="view-select" value={activeId} onchange={(e) => onswitch((e.target as HTMLSelectElement).value)}>
      {#each views as v (v.id)}
        <option value={v.id}>{v.name}{v.builtin ? t("vm.toolbar.builtinSuffix") : ""}</option>
      {/each}
    </select>
  {/if}

  <span class="chips">
    {#each chips as chip (chip.text + chip.ref.index)}
      <span class="chip" data-testid="filter-chip">
        <button class="main" title={t("vm.toolbar.editCondTip")} onclick={() => (filterOpen = true)}>{chip.text}</button>
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <button
          class="x"
          title={t("vm.rule.removeCond")}
          onkeydown={(e) => { if (e.key === "Delete") { e.preventDefault(); removeChip(chip); } }}
          onclick={() => removeChip(chip)}>×</button>
      </span>
    {/each}
    {#each sortChips as s (s)}
      <button class="chip sort" title={t("vm.toolbar.editSortTip")} onclick={() => (sortOpen = true)}>{s}</button>
    {/each}
    {#if result?.customized}
      <span class="chip customized" title={t("vm.toolbar.customizedTip")}>{t("vm.toolbar.customized")}</span>
    {/if}
  </span>

  <span class="spacer"></span>

  <span class="actions">
    <button data-testid="open-filter" onclick={() => (filterOpen = true)}>{t("vm.filter.title")}</button>
    <button data-testid="open-sort" onclick={() => (sortOpen = true)}>{t("vm.sort.title")}</button>
    <button onclick={() => { saveAsOpen = !saveAsOpen; saveAsName = t("vm.toolbar.copyName", { name: active?.name ?? t("vm.toolbar.view") }); }}>{t("vm.toolbar.saveAs")}</button>
    {#if active && !active.builtin}
      <button class="danger" onclick={deleteView}>{t("vm.toolbar.deleteView")}</button>
    {/if}
  </span>

  <!-- 筛选 / 排序 popover 锚定工具条（右对齐、位于其下），列表保持可见 -->
  {#if filterOpen && active}
    <FilterEditor view={active} {fields} {templates} {result} onclose={() => (filterOpen = false)} onchanged={onchanged} />
  {/if}
  {#if sortOpen && active}
    <SortEditor view={active} {fields} onclose={() => (sortOpen = false)} onchanged={onchanged} />
  {/if}
</div>

{#if saveAsOpen && active}
  <div class="saveas">
    <input
      placeholder={t("vm.toolbar.newName")}
      bind:value={saveAsName}
      onkeydown={(e) => { if (e.key === "Enter") saveAs(); }}
    />
    <button class="primary" onclick={saveAs}>{t("vm.toolbar.createCopy")}</button>
  </div>
{/if}

<style>
  .toolbar {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }

  .view-select {
    max-width: 200px;
  }

  .tabs {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .tabs button {
    border-radius: 999px;
  }

  .tabs button.active {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
  }

  .chips {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    align-items: center;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 0 2px;
    font-size: 12px;
  }

  .chip .main {
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 12px;
    padding: 2px 0 2px 8px;
    cursor: pointer;
  }

  .chip .main:hover {
    color: var(--accent);
  }

  .chip.sort {
    color: var(--text-dim);
    padding: 2px 8px;
  }

  .chip.sort:hover {
    color: var(--accent);
  }

  .chip.customized {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    padding: 2px 8px;
  }

  .chip .x {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 0 2px;
    font-size: 13px;
  }

  .chip .x:hover {
    color: var(--danger);
  }

  .actions {
    display: inline-flex;
    gap: 8px;
    align-items: center;
  }

  .spacer {
    flex: 1;
  }

  .danger {
    color: var(--danger);
    border-color: var(--danger);
  }

  .saveas {
    display: flex;
    gap: 6px;
    margin: -6px 0 12px;
  }

  .saveas input {
    width: 240px;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
  }
</style>

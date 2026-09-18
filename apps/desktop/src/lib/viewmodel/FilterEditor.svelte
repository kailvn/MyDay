<script lang="ts">
  /**
   * 筛选编辑器（FILTER-SPEC §11，Anytype 式轻量面板）：
   * 锚定工具条的 popover（列表保持可见），改动即时生效（防抖 400ms 落库），
   * 头部实时显示匹配条数；「＋ 添加条件」先选字段（RuleRows 内置）。
   * 底部 = 清空 / 重置（内置视图）/ 完成；Esc 或点外部 = 保存后关闭。
   * 记录 / 待办 / 搜索共用同一组件（各面板只传参）。
   */
  import type { FieldDef, ViewConfig, ViewDef, ViewResult } from "../api";
  import { api } from "../api";
  import { t } from "../i18n";
  import {
    astToEditState,
    cloneConfig,
    editStateToAst,
    effectiveConfig,
    type FilterEditState,
  } from "../viewmodel";
  import RuleRows from "./RuleRows.svelte";

  let {
    view,
    fields,
    templates = [],
    result = null,
    onclose,
    onchanged,
  }: {
    view: ViewDef;
    fields: FieldDef[];
    templates?: { id: string; name: string }[];
    /** 当前求值结果：实时匹配数 + 标签建议来源 */
    result?: ViewResult | null;
    onclose: () => void;
    onchanged: () => void;
  } = $props();

  const config = cloneConfig(effectiveConfig(view) as ViewConfig);
  let state = $state<FilterEditState>(astToEditState(config.dataset?.filter));
  let itemType = $state<"all" | "event" | "task" | "log">(config.dataset?.item_type ?? "all");
  let error = $state("");
  let saving = $state(false);

  const matched = $derived(result?.total ?? null);

  /** 自由多值（标签）的建议：当前结果里出现过的标签 */
  const tagSuggestions = $derived.by<string[]>(() => {
    const items = result?.items ?? result?.groups?.flatMap((g) => g.items) ?? [];
    const set = new Set<string>();
    for (const it of items) for (const t of it.tags ?? []) set.add(t);
    return [...set].slice(0, 12);
  });

  // —— 即时生效：任何深度改动 → 防抖 400ms 落库 → onchanged 刷新列表 ——
  let timer: ReturnType<typeof setTimeout> | null = null;
  let dirty = false;
  let firstRun = true;

  $effect(() => {
    JSON.stringify($state.snapshot(state));
    if (firstRun) {
      firstRun = false;
      return;
    }
    touch();
  });

  function touch() {
    dirty = true;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void flush(), 400);
  }

  async function flush() {
    if (!dirty) return;
    dirty = false;
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    ensureValid();
    try {
      saving = true;
      const next = cloneConfig(config);
      next.dataset.filter = editStateToAst($state.snapshot(state) as FilterEditState);
      await api.viewSave(view.id, next);
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  /** 关闭 = 未落库的改动先落库（Anytype 式：编辑即生效，无「应用」步骤） */
  function close() {
    void flush();
    onclose();
  }

  $effect(() => () => {
    if (timer) clearTimeout(timer);
  });

  function ensureValid() {
    // 空行 / 未选字段 / 需要值但值为 null 的行落库前清掉（宁可查不到不静默猜测在引擎侧兜底）
    const ok = (r: { field: string; value: unknown; cmp: string }) =>
      r.field && (r.value != null || ["empty", "not_empty", "is_true", "is_false"].includes(r.cmp));
    state.rows = state.rows.filter(ok);
    if (state.sub) {
      state.sub.rows = state.sub.rows.filter(ok);
      if (state.sub.rows.length === 0) state.sub = null;
    }
  }

  function clearAll() {
    state.rows = [];
    state.sub = null;
    touch();
    void flush();
  }

  async function reset() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    dirty = false;
    try {
      await api.viewReset(view.id);
      onchanged();
      onclose();
    } catch (e) {
      error = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<!-- 透明点击捕获层：点外部 = 保存并关闭；不遮暗列表（保持所见即所得） -->
<div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}></div>

<div class="panel" role="dialog" aria-label={t("vm.filter.title")}>
  <div class="head">
    <h3>{t("vm.filter.titleView", { name: view.name })}</h3>
    <span class="count" data-testid="filter-matched">
      {#if saving}{t("vm.filter.saving")}{:else if matched != null}{t("vm.filter.matched", { n: matched })}{/if}
    </span>
  </div>

  <div class="body">
    <div class="op-row">
      <span class="op-label">{t("vm.filter.match")}</span>
      <span class="seg" role="group" aria-label={t("vm.filter.opLabel")}>
        <button class:on={state.op === "and"} onclick={() => (state.op = "and")}>{t("vm.filter.all")}</button>
        <button class:on={state.op === "or"} onclick={() => (state.op = "or")}>{t("vm.filter.any")}</button>
      </span>
    </div>

    <div class="rows-list">
      <RuleRows bind:rows={state.rows} {fields} {templates} {itemType} {tagSuggestions} />

      {#if state.sub}
        <div class="sub">
          <div class="sub-head">
            <span>{t("vm.filter.groupPrefix")}</span>
            <span class="seg small" role="group" aria-label={t("vm.filter.subOpLabel")}>
              <button class:on={state.sub.op === "and"} onclick={() => { if (state.sub) state.sub.op = "and"; }}>{t("vm.filter.subAll")}</button>
              <button class:on={state.sub.op === "or"} onclick={() => { if (state.sub) state.sub.op = "or"; }}>{t("vm.filter.subAny")}</button>
            </span>
            <span>{t("vm.filter.groupSuffix")}</span>
            <span class="flex1"></span>
            <button class="ghost" onclick={() => (state.sub = null)}>{t("vm.filter.removeGroup")}</button>
          </div>
          <RuleRows bind:rows={state.sub.rows} {fields} {templates} {itemType} />
        </div>
      {:else}
        <button class="ghost add-group" onclick={() => (state.sub = { op: "or", rows: [] })}>{t("vm.filter.addGroup")}</button>
      {/if}
    </div>

    {#if error}<p class="error">{error}</p>{/if}
  </div>

  <div class="foot">
    <button class="ghost" onclick={clearAll}>{t("vm.filter.clear")}</button>
    {#if view.builtin}
      <button class="ghost" title={t("vm.filter.resetTip")} onclick={reset}>{t("common.reset")}</button>
    {/if}
    <span class="flex1"></span>
    <button class="primary" onclick={close}>{t("common.done")}</button>
  </div>
  <p class="fine">{t("vm.filter.fine")}</p>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    z-index: 149;
  }

  .panel {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    width: min(560px, calc(100vw - 48px));
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 14px 16px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.3);
    display: flex;
    flex-direction: column;
    gap: 10px;
    z-index: 150;
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  h3 {
    margin: 0;
    font-size: 14px;
  }

  .count {
    color: var(--text-dim);
    font-size: 12px;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-height: min(460px, calc(100vh - 260px));
    overflow-y: auto;
    padding-right: 2px;
  }

  .op-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-dim);
  }

  .seg {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }

  .seg button {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: 12px;
    padding: 4px 10px;
    cursor: pointer;
  }

  .seg button + button {
    border-left: 1px solid var(--border);
  }

  .seg button.on {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .rows-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .sub {
    border: 1px dashed var(--border);
    border-radius: 10px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .sub-head {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13px;
    color: var(--text-dim);
  }

  .seg.small button {
    padding: 2px 8px;
    font-size: 11px;
  }

  .flex1 {
    flex: 1;
  }

  .add-group {
    align-self: flex-start;
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 13px;
    cursor: pointer;
    padding: 2px 0;
  }

  .add-group:hover {
    color: var(--accent);
  }

  .ghost {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    padding: 5px 12px;
    cursor: pointer;
  }

  .ghost:hover {
    border-color: var(--text-dim);
  }

  .foot {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-top: 2px;
  }

  .primary {
    background: var(--accent);
    color: var(--accent-fg);
    border: none;
    border-radius: 8px;
    font-size: 13px;
    padding: 6px 16px;
    cursor: pointer;
  }

  .fine {
    color: var(--text-dim);
    font-size: 11px;
    margin: 0;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
    margin: 0;
  }
</style>

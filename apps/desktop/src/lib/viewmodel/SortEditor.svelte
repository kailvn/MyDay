<script lang="ts">
  /**
   * 排序编辑器（FILTER-SPEC §11 Anytype 式）：多级行列表（字段 + 方向 + 增删），
   * 锚定工具条的轻量 popover，改动即时生效（防抖落库）。
   * NULL / 缺值恒排该级末尾（引擎语义）。各面板共用同一组件。
   */
  import type { FieldDef, SortSpec, ViewConfig, ViewDef } from "../api";
  import { api } from "../api";
  import { t } from "../i18n";
  import { BUILTIN_COLUMNS, cloneConfig, effectiveConfig, fieldApplicable, fieldLabel } from "../viewmodel";
  import FieldMenu from "./FieldMenu.svelte";

  let {
    view,
    fields,
    onclose,
    onchanged,
  }: {
    view: ViewDef;
    fields: FieldDef[];
    onclose: () => void;
    onchanged: () => void;
  } = $props();

  const config = cloneConfig(effectiveConfig(view) as ViewConfig);
  let rows = $state<SortSpec[]>(
    (config.dataset?.sort ?? []).map((s) => ({ field: s.field, dir: s.dir })),
  );
  let itemType = config.dataset?.item_type ?? "all";
  let error = $state("");

  let menuOpen = $state(false);

  // —— 即时生效：改动 → 防抖 250ms 落库（纯下拉/移动操作，无逐键输入） ——
  let timer: ReturnType<typeof setTimeout> | null = null;
  let firstRun = true;

  $effect(() => {
    JSON.stringify($state.snapshot(rows));
    if (firstRun) {
      firstRun = false;
      return;
    }
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => void flush(), 250);
  });

  $effect(() => () => {
    if (timer) clearTimeout(timer);
  });

  async function flush() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    try {
      const next = cloneConfig(config);
      next.dataset.sort = rows.filter((r) => r.field);
      await api.viewSave(view.id, next);
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }

  /** 关闭 = 改动已（或将立即）落库 */
  function close() {
    void flush();
    onclose();
  }

  function addField(field: string) {
    rows.push({ field, dir: "asc" });
    menuOpen = false;
  }
  function move(i: number, up: boolean) {
    const j = up ? i - 1 : i + 1;
    if (j < 0 || j >= rows.length) return;
    [rows[i], rows[j]] = [rows[j], rows[i]];
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  const sortables = () => [
    ...BUILTIN_COLUMNS.map((c) => ({ id: c.id, label: t(c.label) })),
    ...fields.filter((f) => fieldApplicable(f.id, itemType, fields)).map((f) => ({ id: f.id, label: f.name })),
  ];
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) close(); }}></div>

<div class="panel" role="dialog" aria-label={t("vm.sort.title")}>
  <div class="head">
    <h3>{t("vm.sort.titleView", { name: view.name })}</h3>
    <span class="hint">{t("vm.sort.nullsHint")}</span>
  </div>

  <div class="body">
    {#if rows.length === 0}
      <p class="hint">{t("vm.sort.empty")}</p>
    {/if}
    {#each rows as row, i (i)}
      <div class="rule" data-testid="sort-rule">
        <span class="idx">{i + 1}</span>
        <select bind:value={row.field}>
          {#each sortables() as o (o.id)}
            <option value={o.id}>{o.label}</option>
          {/each}
        </select>
        <span class="seg" role="group" aria-label={t("vm.sort.dirLabel")}>
          <button class:on={row.dir === "asc"} title={t("vm.sort.asc")} onclick={() => (row.dir = "asc")}>↑</button>
          <button class:on={row.dir === "desc"} title={t("vm.sort.desc")} onclick={() => (row.dir = "desc")}>↓</button>
        </span>
        <button class="ghost icon" title={t("vm.sort.moveUp")} disabled={i === 0} onclick={() => move(i, true)}>↑</button>
        <button class="ghost icon" title={t("vm.sort.moveDown")} disabled={i === rows.length - 1} onclick={() => move(i, false)}>↓</button>
        <button class="x" title={t("vm.sort.remove")} onclick={() => rows.splice(i, 1)}>×</button>
      </div>
    {/each}

    <div class="add-wrap">
      <button class="ghost add" onclick={() => (menuOpen = !menuOpen)}>{t("vm.sort.add")}</button>
      {#if menuOpen}
        <FieldMenu groups={[{ label: t("vm.field.groupFields"), options: sortables() }]} onpick={addField} onclose={() => (menuOpen = false)} />
      {/if}
    </div>

    {#if error}<p class="error">{error}</p>{/if}
  </div>

  <div class="foot">
    <span class="fine">{rows.length ? rows.map((r) => `${fieldLabel(r.field, fields)} ${r.dir === "asc" ? "↑" : "↓"}`).join(" → ") : ""}</span>
    <span class="flex1"></span>
    <button class="primary" onclick={close}>{t("common.done")}</button>
  </div>
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
    width: min(480px, calc(100vw - 48px));
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

  .hint {
    color: var(--text-dim);
    font-size: 12px;
    margin: 0;
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-height: min(360px, calc(100vh - 260px));
    overflow-y: auto;
  }

  .rule {
    display: flex;
    align-items: center;
    gap: 6px;
    background: color-mix(in srgb, var(--border) 22%, transparent);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 7px 9px;
  }

  .rule select {
    font: inherit;
    font-size: 13px;
    flex: 1;
    min-width: 0;
  }

  .idx {
    color: var(--text-dim);
    font-size: 12px;
    width: 14px;
    text-align: right;
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
    font-size: 13px;
    padding: 3px 9px;
    cursor: pointer;
  }

  .seg button + button {
    border-left: 1px solid var(--border);
  }

  .seg button.on {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .ghost {
    border: 1px solid var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    padding: 4px 10px;
    cursor: pointer;
  }

  .ghost.icon {
    padding: 3px 7px;
    color: var(--text-dim);
  }

  .ghost:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .ghost:not(:disabled):hover {
    border-color: var(--text-dim);
  }

  .add-wrap {
    position: relative;
    align-self: flex-start;
  }

  .add {
    border: 1px dashed var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text-dim);
    font-size: 13px;
    padding: 5px 12px;
    cursor: pointer;
  }

  .add:hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .x {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 15px;
    padding: 0 4px;
    cursor: pointer;
  }

  .x:hover {
    color: var(--danger);
  }

  .foot {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .flex1 {
    flex: 1;
  }

  .fine {
    color: var(--text-dim);
    font-size: 11px;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .error {
    color: var(--danger);
    font-size: 13px;
    margin: 0;
  }
</style>

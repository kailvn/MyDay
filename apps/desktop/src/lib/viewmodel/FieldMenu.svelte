<script lang="ts">
  /**
   * 字段选择菜单（FILTER-SPEC §11，Anytype 式「添加条件先选字段」）：
   * 分组列表 + 关键词过滤，点选即回调。规则行 / 排序行共用，
   * 一致性靠复用保证（§0）。
   */
  import { t } from "../i18n";

  let {
    groups,
    onpick,
    onclose,
  }: {
    groups: { label: string; options: { id: string; label: string }[] }[];
    onpick: (id: string) => void;
    onclose: () => void;
  } = $props();

  let q = $state("");
  let input = $state<HTMLInputElement | null>(null);
  $effect(() => {
    input?.focus();
  });

  const filtered = $derived.by(() =>
    groups
      .map((g) => ({
        label: g.label,
        options: g.options.filter((o) => o.label.toLowerCase().includes(q.trim().toLowerCase())),
      }))
      .filter((g) => g.options.length > 0),
  );

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if (e.key === "Enter") {
      const first = filtered[0]?.options[0];
      if (first) {
        e.preventDefault();
        onpick(first.id);
      }
    }
  }
</script>

<div class="menu" role="menu" aria-label={t("vm.field.choose")}>
  <input bind:this={input} bind:value={q} placeholder={t("vm.field.search")} onkeydown={onKeydown} />
  <div class="list">
    {#each filtered as g (g.label)}
      <div class="group-label">{g.label}</div>
      {#each g.options as o (o.id)}
        <button type="button" role="menuitem" onclick={() => onpick(o.id)}>{o.label}</button>
      {/each}
    {/each}
    {#if filtered.length === 0}
      <div class="empty">{t("vm.field.noMatch")}</div>
    {/if}
  </div>
</div>

<style>
  .menu {
    position: absolute;
    left: 0;
    top: calc(100% + 4px);
    min-width: 220px;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 10px 32px rgb(0 0 0 / 0.22);
    z-index: 30;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  input {
    margin: 8px;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    font: inherit;
    font-size: 13px;
    background: transparent;
    color: var(--text);
  }

  .list {
    max-height: 240px;
    overflow-y: auto;
    padding: 0 4px 6px;
  }

  .group-label {
    font-size: 11px;
    color: var(--text-dim);
    padding: 6px 8px 2px;
  }

  .list button {
    display: block;
    width: 100%;
    text-align: left;
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    padding: 6px 8px;
    border-radius: 6px;
    cursor: pointer;
  }

  .list button:hover {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .empty {
    color: var(--text-dim);
    font-size: 12px;
    padding: 8px;
  }
</style>

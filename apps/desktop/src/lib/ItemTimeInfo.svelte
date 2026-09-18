<script lang="ts">
  /** 条目审计信息：创建 / 修改时间。悬停可见完整日期时间。 */
  import { fmtTime, fmtDateTime, type Item } from "./api";

  let { item }: { item: Item } = $props();

  let modified = $derived(item.updated_at !== item.created_at);
</script>

<span
  class="tinfo"
  title={`创建 ${fmtDateTime(item.created_at)}&#10;修改 ${fmtDateTime(item.updated_at)}`}
>
  建 {fmtTime(item.created_at)}{#if modified}&thinsp;·&thinsp;改 {fmtTime(item.updated_at)}{/if}
</span>

<style>
  .tinfo {
    color: var(--text-dim);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>

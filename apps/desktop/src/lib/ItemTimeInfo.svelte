<script lang="ts">
  /** 条目审计信息：单一时间戳——改过显示修改时间，否则创建时间（月日时分秒，
   *  只有时分难区分同日先后）；悬停可见完整创建 / 修改时刻。 */
  import { fmtStamp, fmtDateTime, type Item } from "./api";

  let { item }: { item: Item } = $props();

  const stamp = $derived(item.updated_at !== item.created_at ? item.updated_at : item.created_at);
</script>

<span
  class="tinfo"
  title={`创建 ${fmtDateTime(item.created_at)}&#10;修改 ${fmtDateTime(item.updated_at)}`}
>
  {fmtStamp(stamp)}
</span>

<style>
  .tinfo {
    color: var(--text-dim);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>

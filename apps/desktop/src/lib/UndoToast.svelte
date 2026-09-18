<script lang="ts">
  /**
   * 删除撤销提示：底部浮出「已删除 …」+ 撤销按钮 + 倒计时条。
   * 仅主窗口渲染（Shell）；宽限期结束或撤销后消失。
   */
  import { deletions } from "./deletion.svelte";
  import { t } from "./i18n";

  let latest = $derived(deletions.latest);
</script>

{#if latest}
  <div class="toast" role="status">
    {#key latest.item.id}
      <div class="bar" style="animation-duration: {deletions.undoWindowMs}ms"></div>
    {/key}
    <span class="msg">
      {t('undo.deleted', { what: deletions.describe(latest.item) })}
      {#if deletions.pendingIds.length > 1}
        {t('undo.morePending', { n: deletions.pendingIds.length - 1 })}
      {/if}
    </span>
    {#if deletions.pendingIds.length > 1}
      <button class="undo" onclick={() => deletions.undoAll()}>
        {t('undo.all', { n: deletions.pendingIds.length })}
      </button>
    {:else}
      <button class="undo" onclick={() => deletions.undo(latest.item.id)}>{t('common.undo')}</button>
    {/if}
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.18);
    padding: 10px 12px 12px;
    display: flex;
    align-items: center;
    gap: 14px;
    overflow: hidden;
    min-width: 280px;
    z-index: 100;
  }

  .bar {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 3px;
    width: 100%;
    background: var(--accent);
    transform-origin: left;
    animation: shrink linear forwards;
  }

  @keyframes shrink {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }

  .msg {
    flex: 1;
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .undo {
    color: var(--accent);
    border: none;
    background: transparent;
    font-weight: 600;
    padding: 4px 8px;
  }

  .undo:hover {
    filter: none;
    text-decoration: underline;
  }
</style>

<script lang="ts">
  /**
   * 文件链接 chips：点击文件名 = 用系统默认程序打开；
   * 📁 = 在文件管理器中定位（无文件管理器支持时打开所在目录）；
   * 传入 onremove 时显示移除按钮（面板编辑态）。
   * 打开失败（文件被移动/删除）就地提示，不打断其他链接。
   */
  import { api, basename } from "./api";
  import { t } from "./i18n";

  let { links, onremove }: { links: string[]; onremove?: (path: string) => void } = $props();

  let err = $state("");
  let errTimer: ReturnType<typeof setTimeout> | null = null;

  function showErr(e: unknown) {
    err = String(e);
    if (errTimer) clearTimeout(errTimer);
    errTimer = setTimeout(() => (err = ""), 3000);
  }

  async function open(p: string) {
    try {
      await api.openFilePath(p);
    } catch (e) {
      showErr(e);
    }
  }

  async function reveal(p: string) {
    try {
      await api.revealFilePath(p);
    } catch (e) {
      showErr(e);
    }
  }
</script>

{#if links.length || err}
  <span class="flinks">
    {#each links as p (p)}
      <span class="flink">
        <button class="fname" title={t('filelink.openTip')} onclick={() => open(p)}>
          📎 {basename(p)}
        </button>
        <button class="act" title={t('filelink.revealTip')} onclick={() => reveal(p)}>📁</button>
        {#if onremove}
          <button class="act" title={t('filelink.remove')} onclick={() => onremove?.(p)}>×</button>
        {/if}
      </span>
    {/each}
    {#if err}<span class="ferr">{err}</span>{/if}
  </span>
{/if}

<style>
  .flinks {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }

  .flink {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    border-radius: 999px;
    padding-right: 4px;
  }

  .fname {
    border: none;
    background: transparent;
    color: var(--text);
    font-size: 12px;
    padding: 2px 4px 2px 8px;
    max-width: 220px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fname:hover {
    text-decoration: underline;
    color: var(--accent);
  }

  .act {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 12px;
    padding: 2px 4px;
    border-radius: 999px;
  }

  .act:hover {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .ferr {
    color: var(--danger);
    font-size: 12px;
  }
</style>

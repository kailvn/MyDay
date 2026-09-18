<script lang="ts">
  /**
   * 文件链接 chips：文件 📎、文件夹 📂，点名字 = 打开
   * （文件走系统默认程序，文件夹进文件管理器）；📁 = 在文件管理器中定位
   * （无文件管理器支持时打开所在目录）；传入 onremove 时显示移除按钮（面板编辑态）。
   * 打开失败（路径被移动/删除）就地提示，不打断其他链接。
   */
  import { api, basename } from "./api";
  import { t } from "./i18n";

  let { links, onremove }: { links: string[]; onremove?: (path: string) => void } = $props();

  let err = $state("");
  let errTimer: ReturnType<typeof setTimeout> | null = null;

  /** 路径 → 是否目录。模块级缓存：面板/详情两处 chips 共享，路径类型不会自发变化 */
  const dirCache = new Map<string, boolean>();
  let dirOf = $state<Record<string, boolean>>({});

  $effect(() => {
    const unknown = links.filter((p) => !dirCache.has(p));
    for (const p of unknown) dirCache.set(p, false); // 先占位按文件展示，避免闪烁
    for (const p of unknown) {
      void api.pathIsDir(p).then((isDir) => {
        dirCache.set(p, isDir);
        dirOf = { ...dirOf, [p]: isDir };
      });
    }
    dirOf = { ...Object.fromEntries(links.map((p) => [p, dirCache.get(p) ?? false])) };
  });

  const isDir = (p: string) => dirOf[p] ?? false;

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
        <button class="fname" title={isDir(p) ? t('filelink.openDirTip') : t('filelink.openTip')} onclick={() => open(p)}>
          {#if isDir(p)}📂{:else}📎{/if} {basename(p)}
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

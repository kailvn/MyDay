<script lang="ts">
  /**
   * 今日悬浮窗（OVERLAY-SPEC v1）：第三个 Tauri 窗口，只读展示「今日未完成」，
   * 唯一高频动作 = 勾选完成；头部日期行拖动定位（位置防抖持久化）；⚙ 内联设置
   * （透明度 / 角落吸附重置 / 锁定）。求值走 overlay_today（myday-core 唯一语义
   * 实现），data-changed（300ms 防抖）+ 60s tick 双路刷新（§4.5，覆盖跨午夜、
   * 进行中态迁移）。
   */
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import { api, fmtTime, type OverlayConfig, type OverlayToday } from "./api";
  import { t } from "./i18n";

  const win = getCurrentWindow();

  let data = $state<OverlayToday | null>(null);
  let cfg = $state<OverlayConfig | null>(null);
  let settingsOpen = $state(false);
  /** 完成淡出中的条目 id（300ms 后真正 complete_task，§5.4） */
  let fading = $state<string[]>([]);
  /** 头部拖动进行中（onMoved 只在拖动时落库，程序化定位不写 custom_pos） */
  let dragging = $state(false);

  let refreshDebounce: ReturnType<typeof setTimeout> | undefined;
  let dragDebounce: ReturnType<typeof setTimeout> | undefined;
  let resizeDebounce: ReturnType<typeof setTimeout> | undefined;

  /** 边缘 / 角落拖拽把手 → 原生 resize 方向（v1.1 §3 尺寸自由调整） */
  const RESIZE_DIRS: Record<string, Parameters<typeof win.startResizeDragging>[0]> = {
    n: "North",
    s: "South",
    e: "East",
    w: "West",
    ne: "NorthEast",
    nw: "NorthWest",
    se: "SouthEast",
    sw: "SouthWest",
  };

  function resizeStart(e: PointerEvent, dir: string) {
    if (e.button !== 0) return;
    e.preventDefault();
    void win.startResizeDragging(RESIZE_DIRS[dir]);
  }

  async function refresh() {
    try {
      data = await api.overlayToday();
    } catch (e) {
      console.error("overlay_today 失败", e);
    }
  }

  function scheduleRefresh() {
    clearTimeout(refreshDebounce);
    refreshDebounce = setTimeout(refresh, 300);
  }

  async function save(next: OverlayConfig) {
    cfg = next;
    try {
      await api.setOverlayConfig(next); // Rust 侧同步托盘 + 落窗口状态 + 广播
    } catch (e) {
      console.error("保存悬浮窗配置失败", e);
    }
  }

  async function complete(id: string) {
    if (fading.includes(id)) return;
    fading = [...fading, id];
    setTimeout(() => {
      fading = fading.filter((x) => x !== id);
      // data-changed 广播后主窗口与本悬浮窗各自刷新（§5.4）
      api.completeTask(id).catch((e) => console.error("完成失败", e));
    }, 300);
  }

  /** 头部进度：只统计今日待办（不含逾期与未安排，§4.4） */
  const progress = $derived.by(() => {
    const done = data?.done_count ?? 0;
    return `${done}/${done + (data?.tasks.length ?? 0)}`;
  });

  const headDate = $derived.by(() => {
    if (!data) return "";
    const d = new Date(`${data.date}T00:00:00`);
    const wd = t("overlay.weekdays").split(",")[d.getDay()];
    return t("overlay.headDate", { m: d.getMonth() + 1, day: d.getDate(), wd });
  });

  /** 摘要行：逾期 / 未安排（皆 0 省略，§4.3） */
  const summaryText = $derived.by(() => {
    const parts: string[] = [];
    if ((data?.overdue_count ?? 0) > 0) parts.push(t("overlay.overdue", { n: data!.overdue_count }));
    if ((data?.unscheduled_count ?? 0) > 0) parts.push(t("overlay.unscheduled", { m: data!.unscheduled_count }));
    return parts.join(" · ");
  });

  const allClear = $derived(
    !!data && data.events.length === 0 && data.tasks.length === 0 && !summaryText,
  );

  function resetCorner(corner: string) {
    if (cfg) void save({ ...cfg, corner, custom_pos: null }); // 清除 custom_pos 重新吸附（§3）
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") settingsOpen = false;
  }

  onMount(async () => {
    // 透明窗口底色让位（与 QuickAddWindow 同款），圆角外露出桌面
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    cfg = await api.getOverlayConfig().catch(() => null);
    await refresh();
    await listen("data-changed", scheduleRefresh);
    await listen("overlay-config", (e) => {
      // 托盘 / 设置页改动广播过来的最新配置
      cfg = (e.payload ?? null) as OverlayConfig | null;
    });
    setInterval(refresh, 60_000);
    // 拖动定位持久化（§3）：onMoved 高频触发，防抖 400ms 落库
    await win.onMoved(() => {
      if (!dragging) return;
      clearTimeout(dragDebounce);
      dragDebounce = setTimeout(() => void api.overlaySaveDragPos(), 400);
    });
    // 拖边尺寸持久化（v1.1）：onResized 高频触发，防抖 400ms；Rust 侧钳制
    // 上下限（220×280 ~ 1200×1600）。程序化 set_size 也会触发，回存同值幂等。
    await win.onResized(() => {
      clearTimeout(resizeDebounce);
      resizeDebounce = setTimeout(() => void api.overlaySaveResizeSize(), 400);
    });
  });
</script>

<svelte:window onkeydown={onKeydown} onpointerup={() => (dragging = false)} />

{#if data && cfg}
  <div class="overlay" style={`--op:${cfg.opacity}`} data-testid="overlay-root">
    <div class="head">
      <span
        class="date"
        data-tauri-drag-region
        onpointerdown={() => (dragging = true)}>{headDate}</span
      >
      <span class="spacer" data-tauri-drag-region onpointerdown={() => (dragging = true)}></span>
      <span class="progress" data-testid="overlay-progress" title={t("overlay.progressTitle")}
        >✓ {progress}</span
      >
      <!-- 锁定态下 ⚙ 不可达（整窗穿透），解锁先走托盘菜单（§5.2/§5.4） -->
      <button class="gear" aria-label={t("overlay.settings")} onclick={() => (settingsOpen = !settingsOpen)}>⚙</button>
    </div>

    {#if settingsOpen}
      <div class="settings" data-testid="overlay-settings">
        <label>
          {t("overlay.opacity")}
          <input
            type="range"
            data-testid="overlay-opacity"
            min="0.3"
            max="1"
            step="0.05"
            bind:value={cfg.opacity}
            onchange={() => cfg && save(cfg)}
          />
        </label>
        <div class="row-line">
          {t("overlay.snap")}
          <button class:active={cfg.corner === "tl"} onclick={() => resetCorner("tl")}>{t("overlay.cornerTl")}</button>
          <button class:active={cfg.corner === "tr"} onclick={() => resetCorner("tr")}>{t("overlay.cornerTr")}</button>
        </div>
        <div class="row-line">
          {t("overlay.locked")}
          <input
            type="checkbox"
            data-testid="overlay-locked"
            checked={cfg.locked}
            onchange={(e) => cfg && save({ ...cfg, locked: e.currentTarget.checked })}
          />
        </div>
        <p class="hint">{t("overlay.lockHint")}</p>
      </div>
    {/if}

    <div class="body">
      {#if allClear}
        <div class="empty" data-testid="overlay-empty">{t("overlay.allClear", { n: data.done_count })}</div>
      {:else}
        {#if data.events.length}
          <div class="sec">{t("type.event")}</div>
          {#each data.events as ev (ev.id)}
            <button
              class="row event"
              class:active={ev.in_progress}
              data-testid="overlay-event"
              title={t("overlay.openMain")}
              onclick={() => api.overlayShowMain()}
            >
              <span class="t">
                {ev.all_day ? t("overlay.allDay") : `${fmtTime(ev.start_at)}${ev.end_at ? `–${fmtTime(ev.end_at)}` : ""}`}
              </span>
              <span class="ti">{ev.title}</span>
            </button>
          {/each}
        {/if}
        {#if data.tasks.length}
          <div class="sec">{t("type.task")}</div>
          <div class="tasks">
            {#each data.tasks as task (task.id)}
              <div class="row task" class:fading={fading.includes(task.id)} data-testid="overlay-task">
                <input type="checkbox" title={t("overlay.markDone")} onchange={() => complete(task.id)} />
                <button class="goto" title={t("overlay.openMain")} onclick={() => api.overlayShowMain()}>
                  <span class="t">{task.due_all_day ? "" : fmtTime(task.due_at)}</span>
                  <span class="ti">{task.title}</span>
                </button>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>

    {#if summaryText}
      <button class="summary" data-testid="overlay-summary" onclick={() => api.overlayShowMain()}>
        {summaryText}
      </button>
    {/if}

    <!-- 边缘 / 角落拖拽把手：无边框窗口的尺寸调整入口（锁定态整窗穿透自然不可达） -->
    {#each Object.keys(RESIZE_DIRS) as dir (dir)}
      <div class={`rz rz-${dir}`} onpointerdown={(e) => resizeStart(e, dir)}></div>
    {/each}
  </div>
{/if}

<style>
  .overlay {
    /* 透明度走 CSS（§5.5）：背景/边框层用 --op，文字保持不透明保证可读 */
    z-index: 0;
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    border-radius: 12px;
    overflow: hidden;
    color: var(--text);
    font-size: 13px;
  }

  .overlay::before {
    content: "";
    position: absolute;
    inset: 0;
    z-index: -1;
    background: var(--card);
    border: 1px solid var(--border);
    opacity: var(--op);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    font-weight: 600;
  }

  .date {
    cursor: move;
    user-select: none;
  }

  .spacer {
    flex: 1;
    align-self: stretch;
    cursor: move;
  }

  .progress {
    color: var(--accent);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .gear {
    border: none;
    background: transparent;
    padding: 2px 4px;
    font-size: 13px;
    cursor: pointer;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }

  .settings label {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: space-between;
  }

  .settings input[type="range"] {
    flex: 1;
    max-width: 130px;
    padding: 0;
  }

  .row-line {
    display: flex;
    align-items: center;
    gap: 6px;
    justify-content: space-between;
  }

  .row-line button {
    font-size: 11px;
    padding: 1px 8px;
  }

  .row-line button.active {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
  }

  .hint {
    margin: 0;
    color: var(--text-dim);
    font-size: 11px;
  }

  .body {
    flex: 1;
    overflow-y: auto;
    padding: 6px 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .sec {
    font-size: 11px;
    color: var(--text-dim);
    padding: 4px 8px 2px;
    font-weight: 600;
  }

  .tasks {
    /* 待办区内部滚动（§4.4），上限随窗高自适应（细滚动条 hover 才显） */
    max-height: calc(100vh - 130px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    scrollbar-width: thin;
    scrollbar-color: transparent transparent;
  }

  .tasks:hover {
    scrollbar-color: var(--border) transparent;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    border-radius: 6px;
    border: none;
    background: transparent;
    text-align: left;
    width: 100%;
    font-size: 12.5px;
    cursor: pointer;
    transition: opacity 0.3s;
  }

  .row:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
    filter: none;
  }

  .row.event.active {
    box-shadow: inset 3px 0 0 var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .row.fading {
    opacity: 0;
  }

  .goto {
    flex: 1;
    min-width: 0;
    display: flex;
    gap: 6px;
    align-items: baseline;
    border: none;
    background: transparent;
    padding: 0;
    font: inherit;
    color: inherit;
    cursor: pointer;
    text-align: left;
  }

  .t {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    flex: none;
    min-width: 34px;
    font-size: 11.5px;
  }

  .ti {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row input[type="checkbox"] {
    flex: none;
    padding: 0;
  }

  .summary {
    border: none;
    border-top: 1px solid var(--border);
    background: transparent;
    color: var(--danger);
    font-size: 12px;
    padding: 7px 10px;
    text-align: left;
    cursor: pointer;
  }

  .summary:hover {
    background: color-mix(in srgb, var(--danger) 8%, transparent);
    filter: none;
  }

  .empty {
    color: var(--text-dim);
    text-align: center;
    padding: 40px 10px;
    font-size: 13px;
  }

  /* 尺寸调整把手：贴边的隐形热区（角上更宽好抓） */
  .rz {
    position: absolute;
    z-index: 6;
  }

  .rz-n { top: 0; left: 14px; right: 14px; height: 6px; cursor: n-resize; }
  .rz-s { bottom: 0; left: 14px; right: 14px; height: 6px; cursor: s-resize; }
  .rz-e { right: 0; top: 14px; bottom: 14px; width: 6px; cursor: e-resize; }
  .rz-w { left: 0; top: 14px; bottom: 14px; width: 6px; cursor: w-resize; }
  .rz-ne { top: 0; right: 0; width: 16px; height: 16px; cursor: nesw-resize; }
  .rz-sw { bottom: 0; left: 0; width: 16px; height: 16px; cursor: nesw-resize; }
  .rz-nw { top: 0; left: 0; width: 16px; height: 16px; cursor: nwse-resize; }
  .rz-se { bottom: 0; right: 0; width: 16px; height: 16px; cursor: nwse-resize; }
</style>

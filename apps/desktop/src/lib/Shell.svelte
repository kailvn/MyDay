<script lang="ts">
  /**
   * 主窗口骨架（需求 §8）：侧边栏导航 + 视图区 + Ctrl+K 搜索 + Ctrl+N 快速添加。
   * 监听 data-changed（CLI / IPC 写入后刷新）与 reveal-item（通知定位）。
   */
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api } from "./api";
  import ItemPanel from "./ItemPanel.svelte";
  import ItemDetail from "./ItemDetail.svelte";
  import { panelRequest, openCreate, openDetail } from "./panel.svelte";
  import UndoToast from "./UndoToast.svelte";
  import { toast } from "./toast.svelte";
  import CommandPalette from "./CommandPalette.svelte";
  import Onboarding from "./Onboarding.svelte";
  import ReminderCenter from "./ReminderCenter.svelte";
  import TodayView from "./views/TodayView.svelte";
  import CalendarView from "./views/CalendarView.svelte";
  import TasksView from "./views/TasksView.svelte";
  import LogsView from "./views/LogsView.svelte";
  import StatsView from "./views/StatsView.svelte";
  import SearchView from "./views/SearchView.svelte";
  import TemplatesView from "./views/TemplatesView.svelte";
  import SettingsView from "./views/SettingsView.svelte";
  import HelpView from "./views/HelpView.svelte";
  import { t, type MessageKey } from "./i18n";

  type ViewId = "today" | "calendar" | "tasks" | "logs" | "stats" | "search" | "templates" | "help" | "settings";

  let view = $state("today");
  let searchQuery = $state("");
  let dataVersion = $state(0);
  /** 跨午夜 / 时区变更计数（FILTER-SPEC §10.6）：相对日期视图整体重查 */
  let clockVersion = $state(0);
  let paletteOpen = $state(false);
  let reminderOpen = $state(false);
  let unread = $state(0);

  async function refreshUnread() {
    try {
      unread = await api.reminderUnread();
    } catch {
      /* 提醒中心不可用不阻塞主界面 */
    }
  }

  const nav: { id: ViewId; labelKey: MessageKey; icon: string }[] = [
    { id: "today", labelKey: "shell.nav.today", icon: "☀" },
    { id: "calendar", labelKey: "shell.nav.calendar", icon: "📅" },
    { id: "tasks", labelKey: "shell.nav.tasks", icon: "✓" },
    { id: "logs", labelKey: "shell.nav.logs", icon: "🗒" },
    { id: "stats", labelKey: "shell.nav.stats", icon: "📈" },
    { id: "search", labelKey: "shell.nav.search", icon: "🔍" },
    { id: "templates", labelKey: "shell.nav.templates", icon: "🗂" },
    { id: "help", labelKey: "shell.nav.help", icon: "？" },
    { id: "settings", labelKey: "shell.nav.settings", icon: "⚙" },
  ];

  // 求值信封失效检测：60 秒定时器查本地日 / 时区；窗口聚焦 / visibilitychange
  // 立即检查（§10.6 前端三处触发）。判定变化才递增，各视图随 clockVersion 重查。
  let clockDay = new Date().toDateString();
  let clockTz = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
  function checkClock() {
    const day = new Date().toDateString();
    const tz = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "";
    if (day !== clockDay || tz !== clockTz) {
      clockDay = day;
      clockTz = tz;
      clockVersion++;
    }
  }
  setInterval(checkClock, 60_000);
  window.addEventListener("focus", checkClock);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) checkClock();
  });

  function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      paletteOpen = !paletteOpen;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n") {
      e.preventDefault();
      api.openQuickAdd();
    }
  }

  onMount(async () => {
    // 首次使用建立已读基线（不把历史提醒算成未读）
    if ((await api.getSetting("reminder_seen_at").catch(() => null)) == null) {
      await api.markRemindersSeen().catch(() => null);
    }
    await refreshUnread();
    await listen("data-changed", () => {
      dataVersion++;
      void refreshUnread();
    });
    await listen("reminder-fired", () => {
      void refreshUnread();
    });
    // 定位：通知点击 / CLI reveal → 打开该条目只读详情（条目可能已删，忽略失败）
    const locate = async (e: { payload: unknown }) => {
      const id = (e.payload as { id?: string } | null)?.id;
      if (!id) return;
      try {
        openDetail(await api.getItem(id));
      } catch {
        /* 条目已删除 */
      }
    };
    await listen("reveal-item", locate);
    await listen("reminder-fired", locate);
    // E2E / 浏览器模式钩子：mock 的 open_quick_add 落为页面内创建面板
    //（Tauri 下走独立窗口，本事件不会被派发）
    window.addEventListener("myday-dev-quick-add", (e) => {
      const d = (e as CustomEvent).detail ?? {};
      openCreate({ item_type: d.itemType ?? null, title: d.title ?? null });
    });
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="shell">
  <aside>
    <div class="brand-row">
      <div class="brand">MyDay</div>
      <button
        class="bell"
        title={t('shell.reminderCenter')}
        aria-label={t('shell.reminderCenter')}
        onclick={() => (reminderOpen = true)}
      >
        🔔
        {#if unread > 0}
          <span class="badge">{unread > 99 ? "99+" : unread}</span>
        {/if}
      </button>
    </div>
    <button class="quick-btn primary" onclick={() => api.openQuickAdd()}>
      ＋ {t('shell.quickAdd')}
      <kbd>Ctrl+N</kbd>
    </button>
    <nav>
      {#each nav as n (n.id)}
        <button class:active={view === n.id} onclick={() => (view = n.id)}>
          <span class="icon">{n.icon}</span>{t(n.labelKey)}
        </button>
      {/each}
    </nav>
  </aside>

  <main>
    {#if view === "today"}
      <TodayView {dataVersion} />
    {:else if view === "calendar"}
      <CalendarView {dataVersion} />
    {:else if view === "tasks"}
      <TasksView {dataVersion} {clockVersion} />
    {:else if view === "logs"}
      <LogsView {dataVersion} {clockVersion} />
    {:else if view === "stats"}
      <StatsView {dataVersion} {clockVersion} />
    {:else if view === "search"}
      <SearchView {dataVersion} {clockVersion} query={searchQuery} />
    {:else if view === "templates"}
      <TemplatesView {dataVersion} />
    {:else if view === "help"}
      <HelpView />
    {:else if view === "settings"}
      <SettingsView />
    {/if}
  </main>

  <!-- 面板：detail=只读详情（点条目行打开），其余 = 统一编辑面板；
       #key 保证面板已开时新请求（如点另一格日历）仍能重新初始化 -->
  {#key panelRequest()}
    {#if panelRequest()?.mode === "detail"}
      <ItemDetail />
    {:else if panelRequest()}
      <ItemPanel />
    {/if}
  {/key}

  <UndoToast />

  <!-- 提醒中心（SPRINT2-SPEC §5）：关闭时刷新未读角标 -->
  <ReminderCenter
    open={reminderOpen}
    onclose={() => {
      reminderOpen = false;
      void refreshUnread();
    }}
  />

  <!-- Ctrl+K 命令面板：动作 + 结果混排 -->
  <CommandPalette
    open={paletteOpen}
    onclose={() => (paletteOpen = false)}
    onNavigate={(v) => (view = v as typeof view)}
    onSearch={(q) => {
      searchQuery = q;
      view = "search";
      setTimeout(() => document.getElementById("global-search")?.focus(), 30);
    }}
  />

  <!-- 全局轻提示（冲突警告 / 批量操作结果 / 拖拽改期撤销等，非阻断） -->
  {#if toast.message}
    <div class="toast" role="status" onclick={() => toast.dismiss()}>
      <span>{toast.message}</span>
      {#if toast.action}
        <button
          class="toast-action"
          onclick={(e) => {
            e.stopPropagation();
            const act = toast.action;
            toast.dismiss();
            if (act) void act.run();
          }}>{toast.action!.label}</button
        >
      {/if}
    </div>
  {/if}

  <!-- 首次启动「选择启用模板」引导（§6） -->
  <Onboarding />
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 200px 1fr;
    height: 100vh;
  }

  aside {
    background: var(--sidebar-bg);
    padding: 16px 10px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    border-right: 1px solid var(--border);
  }

  .brand {
    font-size: 20px;
    font-weight: 700;
    padding: 4px 10px;
    letter-spacing: 0.5px;
  }

  .brand-row {
    display: flex;
    align-items: center;
  }

  .bell {
    margin-left: auto;
    position: relative;
    border: none;
    background: transparent;
    font-size: 17px;
    padding: 6px 8px;
    border-radius: 8px;
    line-height: 1;
  }

  .bell:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
  }

  .bell .badge {
    position: absolute;
    top: 0;
    right: 0;
    background: var(--danger, #d33);
    color: #fff;
    font-size: 10px;
    font-weight: 700;
    border-radius: 999px;
    min-width: 16px;
    height: 16px;
    line-height: 16px;
    text-align: center;
    padding: 0 3px;
  }

  .quick-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 9px 12px;
    font-weight: 600;
  }

  .quick-btn kbd {
    font-size: 10px;
    opacity: 0.75;
    border: 1px solid currentColor;
    border-radius: 3px;
    padding: 0 4px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  nav button {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    background: transparent;
    border: none;
    border-radius: 8px;
    padding: 8px 10px;
    color: var(--text);
  }

  nav button:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    filter: none;
  }

  nav button.active {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    color: var(--text);
    font-weight: 600;
  }

  .icon {
    width: 18px;
    text-align: center;
  }

  main {
    overflow: auto;
    padding: 20px 26px;
  }

  .toast {
    position: fixed;
    bottom: 100px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--card);
    color: var(--text);
    border: 1px solid var(--border);
    border-left: 3px solid #e6a23c;
    border-radius: 8px;
    padding: 9px 16px;
    font-size: 13px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 25%);
    z-index: 90;
    max-width: 70vw;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .toast .toast-action {
    border: 1px solid color-mix(in srgb, var(--accent) 55%, transparent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
    border-radius: 6px;
    padding: 3px 12px;
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
  }
</style>

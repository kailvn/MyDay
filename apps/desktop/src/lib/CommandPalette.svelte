<script lang="ts">
  /**
   * Ctrl+K 命令面板（SPRINT-SPEC §7）：动作 + 搜索结果混排。
   * - 动作：新建日程/待办/记录、快速添加窗口、切换视图、搜索「q」
   * - 动词快路径：「待办 xxx」置顶「创建待办「xxx」」→ 打开预填面板，再 Enter 才保存
   * - 结果：searchItems 前 8 条 → 只读详情；空输入显示今天改过的 5 条
   * - ↑↓ 连续移动，Enter 执行，Esc / 点击遮罩关闭。绝不静默写入。
   */
  import {
    api,
    displayTitle,
    fmtTime,
    toDateInput,
    typeLabel,
    type ItemType,
    type Item,
    type SearchHit,
  } from "./api";
  import { openCreate, openDetail } from "./panel.svelte";
  import { t, type MessageKey } from "./i18n";

  let {
    open = false,
    onNavigate,
    onSearch,
    onclose,
  }: {
    open?: boolean;
    onNavigate: (view: string) => void;
    onSearch: (q: string) => void;
    onclose: () => void;
  } = $props();

  let q = $state("");
  let idx = $state(0);
  let hits = $state<SearchHit[]>([]);
  let recents = $state<Item[]>([]);
  let input: HTMLInputElement | null = $state(null);

  const VIEWS: { id: string; label: MessageKey }[] = [
    { id: "today", label: "palette.view_today" },
    { id: "calendar", label: "palette.view_calendar" },
    { id: "tasks", label: "palette.view_tasks" },
    { id: "logs", label: "palette.view_logs" },
    { id: "stats", label: "palette.view_stats" },
    { id: "templates", label: "palette.view_templates" },
    { id: "settings", label: "palette.view_settings" },
  ];

  /** 动词快路径：「待办 / 日程 / 记录 + 标题」（含英文别名 task/todo/event/log） */
  let verb = $derived.by((): { type: ItemType; rest: string } | null => {
    const m = q.match(/^\s*(待办|任务|日程|事件|记录|task|todo|event|log)\s+(.+)$/i);
    if (!m) return null;
    const w = m[1].toLowerCase();
    const type: ItemType | null =
      w === "待办" || w === "任务" || w === "task" || w === "todo"
        ? "task"
        : w === "记录" || w === "log"
          ? "log"
          : "event";
    return type ? { type, rest: m[2].trim() } : null;
  });

  type Row =
    | { kind: "act"; id: string; label: string; dim?: string; run: () => void }
    | { kind: "hit"; id: string; label: string; dim: string; run: () => void; hit: SearchHit };

  let rows = $derived.by<Row[]>(() => {
    const out: Row[] = [];
    const kw = q.trim().toLowerCase();
    const want = (id: string, label: string) =>
      !kw || label.toLowerCase().includes(kw) || id.includes(kw);
    if (verb) {
      out.push({
        kind: "act",
        id: "verb-create",
        label: t("palette.verb_create", { type: typeLabel(verb.type), name: verb.rest }),
        dim: t("palette.verb_hint"),
        run: () => {
          openCreate({ item_type: verb!.type, title: verb!.rest });
          onclose();
        },
      });
    }
    if (want("new-task", t("palette.new_task")))
      out.push({
        kind: "act",
        id: "new-task",
        label: t("palette.new_task"),
        dim: t("palette.dim_action"),
        run: () => {
          openCreate({ item_type: "task" });
          onclose();
        },
      });
    if (want("new-event", t("palette.new_event")))
      out.push({
        kind: "act",
        id: "new-event",
        label: t("palette.new_event"),
        dim: t("palette.dim_action"),
        run: () => {
          openCreate({ item_type: "event" });
          onclose();
        },
      });
    if (want("new-log", t("palette.new_log")))
      out.push({
        kind: "act",
        id: "new-log",
        label: t("palette.new_log"),
        dim: t("palette.dim_action"),
        run: () => {
          openCreate({ item_type: "log" });
          onclose();
        },
      });
    if (want("quick-add", t("palette.quick_add")))
      out.push({
        kind: "act",
        id: "quick-add",
        label: t("palette.quick_add"),
        dim: t("palette.dim_action"),
        run: () => {
          void api.openQuickAdd();
          onclose();
        },
      });
    for (const v of VIEWS) {
      const viewLabel = t(v.label);
      if (want(`view-${v.id}`, viewLabel))
        out.push({
          kind: "act",
          id: `view-${v.id}`,
          label: t("palette.goto", { name: viewLabel }),
          dim: t("palette.dim_nav"),
          run: () => {
            onNavigate(v.id);
            onclose();
          },
        });
    }
    if (kw) {
      out.push({
        kind: "act",
        id: "search-page",
        label: t("palette.search_label", { name: q.trim() }),
        dim: t("palette.dim_search"),
        run: () => {
          onSearch(q.trim());
          onclose();
        },
      });
    }
    for (const h of hits) {
      out.push({
        kind: "hit",
        id: h.item.id,
        label: displayTitle(h.item),
        dim: `${typeLabel(h.item.type)}${h.item.due_at || h.item.start_at ? " · " + fmtTime(h.item.due_at ?? h.item.start_at) : ""}`,
        hit: h,
        run: () => {
          openDetail(h.item);
          onclose();
        },
      });
    }
    return out;
  });

  // 打开时重置并聚焦；空词预热「今天改过」
  $effect(() => {
    if (!open) return;
    q = "";
    idx = 0;
    hits = [];
    setTimeout(() => input?.focus(), 20);
    void api
      .listItems({ changed_on: toDateInput(new Date()), order: "desc", limit: 5 })
      .then((r) => (recents = r))
      .catch(() => {});
  });
  $effect(() => {
    if (!open) return;
    if (!q.trim()) hits = [];
  });
  // 输入/结果变化后高亮保持在有效范围，Enter 永远有明确目标
  $effect(() => {
    rows.length;
    if (idx >= rows.length) idx = Math.max(0, rows.length - 1);
  });
  $effect(() => {
    q;
    idx = 0;
  });

  function move(delta: number) {
    if (!rows.length) return;
    idx = (idx + delta + rows.length) % rows.length;
  }
  function runRow(row: Row) {
    row.run();
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      move(1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      move(-1);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[Math.min(idx, rows.length - 1)];
      row?.run();
    } else if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="backdrop" onclick={onclose}>
    <div class="palette" role="dialog" aria-label={t("palette.title")} onclick={(e) => e.stopPropagation()}>
      <input
        bind:this={input}
        bind:value={q}
        placeholder={t("palette.placeholder")}
        onkeydown={onKey}
      />
      <ul>
        {#if rows.length === 0}
          <li class="none">{t("palette.empty")}</li>
        {/if}
        {#each rows as row, i (row.id + i)}
          <li
            class:cur={i === idx}
            class:hit={row.kind === "hit"}
            onmouseenter={() => (idx = i)}
            onclick={() => runRow(row)}
          >
            {#if row.kind === "act"}
              <span class="label">{row.label}</span>
              {#if row.dim}<span class="dim">{row.dim}</span>{/if}
            {:else}
              <span class="label">{row.label}</span>
              <span class="dim">{row.dim}</span>
            {/if}
          </li>
        {/each}
      </ul>
      <p class="foot">{t("palette.foot")}</p>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    z-index: 200;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 12vh;
  }

  .palette {
    width: min(620px, 90vw);
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 18px 60px rgb(0 0 0 / 0.4);
    overflow: hidden;
  }

  input {
    width: 100%;
    border: none;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    padding: 13px 16px;
    font-size: 15px;
    background: transparent;
  }

  input:focus {
    outline: none;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 6px;
    max-height: 46vh;
    overflow-y: auto;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 8px;
    cursor: pointer;
  }

  li.cur {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  li.none {
    color: var(--text-dim);
    cursor: default;
  }

  .label {
    flex: 1;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dim {
    color: var(--text-dim);
    font-size: 12px;
    flex-shrink: 0;
  }

  .foot {
    margin: 0;
    padding: 7px 14px;
    border-top: 1px solid var(--border);
    color: var(--text-dim);
    font-size: 11.5px;
  }
</style>

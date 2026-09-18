<script lang="ts">
/**
 * 提醒中心（SPRINT2-SPEC §5）：应用内提醒历史。
 * 系统通知错过 / 关机丢失的兜底入口；每行可定位条目，
 * 未完成待办可直达完成或稍后 10 分钟再响。打开即标记已读（reminder_seen_at）。
 */
  import { api, displayTitle, fmtDateTime, typeLabel, type Item } from "./api";
  import { openDetail } from "./panel.svelte";
  import { t } from "./i18n";

  let { open = false, onclose }: { open?: boolean; onclose: () => void } = $props();

  let entries = $state<{ remind_at: string; item: Item }[]>([]);
  let error = $state("");
  let busy = $state(false);

  $effect(() => {
    if (open) void load();
  });

  async function load() {
    try {
      entries = await api.reminderHistory(50);
      await api.markRemindersSeen();
      error = "";
    } catch (e) {
      error = String(e);
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && open) {
      e.preventDefault();
      onclose();
    }
  }

  async function locate(item: Item) {
    try {
      openDetail(await api.getItem(item.id));
      onclose();
    } catch {
      /* 条目刚被删除 */
    }
  }

  async function complete(item: Item) {
    busy = true;
    try {
      await api.completeTask(item.id);
      await load();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function snooze(item: Item) {
    busy = true;
    try {
      await api.snooze(item.id, new Date(Date.now() + 10 * 60_000).toISOString());
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="overlay" role="presentation" onclick={onclose}>
    <div
      class="panel"
      role="dialog"
      aria-label={t("reminders.title")}
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      <header>
        <h2>{t("reminders.title")}</h2>
        <span class="dim">{t("reminders.count", { n: entries.length })}</span>
        <button class="ghost" onclick={onclose}>{t("common.close")}</button>
      </header>
      {#if error}<p class="error">{error}</p>{/if}
      {#if entries.length === 0}
        <p class="empty">{t("reminders.empty")}<br /><span class="dim">{t("reminders.empty_hint")}</span></p>
      {:else}
        <ul>
          {#each entries as en (en.item.id + ":" + en.remind_at)}
            <li>
              <span class="time">{fmtDateTime(en.remind_at)}</span>
              <span class="kind">{typeLabel(en.item.type)}</span>
              <span class="title" class:done={en.item.status === "done"}>{displayTitle(en.item)}</span>
              {#if en.item.status === "done"}<span class="dim">{t("reminders.done")}</span>{/if}
              <span class="spacer"></span>
              <button class="ghost sm" disabled={busy} onclick={() => locate(en.item)}>{t("reminders.locate")}</button>
              {#if en.item.type === "task" && en.item.status !== "done"}
                <button class="ghost sm" disabled={busy} onclick={() => complete(en.item)}>{t("common.done")}</button>
              {/if}
              {#if en.item.status !== "done"}
                <!-- 已完成的条目再「稍后」没有语义，只对未完成的提供 -->
                <button class="ghost sm" disabled={busy} onclick={() => snooze(en.item)}>{t("reminders.snooze")}</button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.3);
    display: flex;
    align-items: flex-start;
    justify-content: flex-end;
    z-index: 150;
  }

  .panel {
    margin: 52px 24px 24px 0;
    width: 460px;
    max-width: calc(100vw - 48px);
    max-height: calc(100vh - 120px);
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 30%);
    padding: 14px 16px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }

  header h2 {
    margin: 0;
    font-size: 15px;
  }

  header .dim {
    font-size: 12px;
    margin-right: auto;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 8px;
    border-radius: 8px;
    font-size: 13px;
  }

  li:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }

  .time {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
    font-size: 12px;
    white-space: nowrap;
  }

  .kind {
    font-size: 11px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0 6px;
    white-space: nowrap;
  }

  .title {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title.done {
    text-decoration: line-through;
    color: var(--text-dim);
  }

  .spacer {
    flex: 1;
  }

  .dim {
    color: var(--text-dim);
    font-size: 12px;
  }

  .ghost.sm {
    font-size: 12px;
    padding: 3px 8px;
    white-space: nowrap;
  }

  .empty {
    text-align: center;
    color: var(--text-dim);
    font-size: 13px;
    padding: 28px 0;
    margin: 0;
    line-height: 1.8;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
  }
</style>

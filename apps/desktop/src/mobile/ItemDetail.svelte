<script lang="ts">
  /**
   * 条目详情弹层：时间 / 备注 / 字段（fieldBadges 只读渲染）/ 标签 / 重复规则
   * 的查看面 + 轻操作（待办就地完成·回退、删除进回收站）。「编辑」弹
   * EditSheet（标题 / 备注 / 时间 / 提醒 / 标签）；字段值与重复规则仍在桌面编辑。
   */
  import { t } from "../lib/i18n";
  import { api, displayTitle, type Item } from "../lib/api";
  import { fieldBadges } from "../lib/fields.svelte";
  import { recurrenceLabel } from "../lib/recurrence";
  import { toast } from "../lib/toast.svelte";

  let {
    item,
    onclose,
    onchange,
    onedit,
  }: { item: Item; onclose: () => void; onchange: () => void; onedit: (item: Item) => void } = $props();

  let busy = $state(false);
  // 两段式删除确认：第一击进入待确认态,3 秒内再击才真删。
  // 不用 window.confirm——Tauri 的 Android WebView 里它直接返回 false,删除被静默取消(真机踩坑)。
  let confirmDel = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  function askDelete() {
    clearTimeout(confirmTimer);
    if (!confirmDel) {
      confirmDel = true;
      confirmTimer = setTimeout(() => (confirmDel = false), 3000);
      return;
    }
    confirmDel = false;
    void doDelete();
  }

  async function doDelete() {
    busy = true;
    try {
      await api.deleteItem(item.id);
      onchange();
      onclose();
    } catch (e) {
      toast.show(String(e));
    } finally {
      busy = false;
    }
  }

  /** 提醒 spec → 人话：@start-15m → 开始前 15 分钟；@dailyT09:00 → 每天 09:00；RFC3339 → 绝对时刻 */
  function remLabel(spec: string): string {
    const unitOf = (u: string) =>
      u === "m" ? t("mobile.rem_min") : u === "h" ? t("mobile.rem_hour") : t("mobile.rem_day");
    const offText = (s: string): string => {
      const m = /^(\d+)([mhd])$/.exec(s);
      return m ? `${m[1]} ${unitOf(m[2])}` : s;
    };
    const rel = (base: string, rest: string, beforeKey: string, afterKey: string): string => {
      if (!rest) return base;
      if (rest.startsWith("-")) return t(beforeKey, { t: offText(rest.slice(1)) });
      return t(afterKey, { t: offText(rest.replace(/^\+/, "")) });
    };
    if (spec.startsWith("@start"))
      return rel(t("mobile.rem_at_start"), spec.slice(6), "mobile.rem_before_start", "mobile.rem_after_start");
    if (spec.startsWith("@due"))
      return rel(t("mobile.rem_at_due"), spec.slice(4), "mobile.rem_before_due", "mobile.rem_after_due");
    if (spec.startsWith("@dailyT")) return t("mobile.rem_daily", { t: spec.slice(7) });
    if (spec.startsWith("@")) return spec;
    return new Date(spec).toLocaleString([], {
      month: "numeric",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  const hm = (iso: string | null) =>
    iso ? new Date(iso).toLocaleString([], { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false }) : "";
  const dt = (iso: string) => new Date(iso).toLocaleString([], { month: "numeric", day: "numeric", hour: "2-digit", minute: "2-digit" });
</script>

<div class="m-backdrop" onclick={onclose} aria-hidden="true"></div>
<div class="m-sheet" role="dialog">
  <div class="m-grip"></div>
  <div class="m-sheet-head">
    <span class="typebadge" style="--c: {item.type === 'event' ? 'var(--type-event)' : item.type === 'task' ? 'var(--type-task)' : 'var(--type-log)'}">
      {item.type === "event" ? t("type.event") : item.type === "task" ? t("type.task") : t("type.log")}
    </span>
    <h2 class="m-sheet-title">{displayTitle(item)}</h2>
    <button class="m-iconbtn" onclick={onclose}>✕</button>
  </div>
  <div class="m-sheet-body">
    <section>
      <h3>{t("mobile.detail_time")}</h3>
      {#if item.type === "log"}
        <p>{t("mobile.occurred")}：{hm(item.occurred_at)}</p>
      {:else if item.type === "task"}
        <p>
          {t("mobile.due")}：{item.due_all_day ? hm(item.due_at)?.slice(0, 8) ?? t("common.none") : hm(item.due_at) ?? t("common.none")}
        </p>
      {:else}
        <p>
          {item.all_day
            ? `${t("calendar.all_day")} · ${hm(item.start_at)?.slice(0, 8)}`
            : `${hm(item.start_at)} → ${hm(item.end_at)}`}
        </p>
      {/if}
      {#if item.recurrence}
        <p>{t("mobile.detail_repeat")}：{recurrenceLabel(item.recurrence)}</p>
      {/if}
      {#if item.reminders.length}
        <p>{t("mobile.detail_reminder")}：</p>
        {#each item.reminders as r (r.id)}
          <!-- notify = 系统通知档；alarm = 闹钟档（遗留 sound/popup 值按闹钟处理） -->
          <p class="remrow">{r.channel === "notify" ? "🔔" : "⏰"} {remLabel(r.spec)}</p>
        {/each}
      {/if}
    </section>

    {#if item.note}
      <section>
        <h3>{t("mobile.detail_note")}</h3>
        <p class="note">{item.note}</p>
      </section>
    {/if}

    {#if fieldBadges(item).length > 0}
      <section>
        <h3>{t("mobile.detail_fields")}</h3>
        <div class="badges">
          {#each fieldBadges(item) as b}
            <span class="badge">{b.name}: {b.text}</span>
          {/each}
        </div>
      </section>
    {/if}

    {#if item.tags.length > 0}
      <section>
        <h3>{t("mobile.detail_tags")}</h3>
        <div class="badges">
          {#each item.tags as tag}
            <span class="badge tag">{tag}</span>
          {/each}
        </div>
      </section>
    {/if}

    <p class="meta">
      {t("mobile.detail_meta")}：{dt(item.created_at)} / {dt(item.updated_at)}
    </p>
    <p class="rohint">{t("mobile.detail_hint")}</p>

    <div class="actions">
      <button class="m-primary" disabled={busy} onclick={() => onedit(item)}>
        {t("common.edit")}
      </button>
      {#if item.type === "task"}
        <button
          class="m-primary"
          disabled={busy}
          onclick={async () => {
            busy = true;
            try {
              if (item.status === "done") await api.updateItem(item.id, { status: "todo" });
              else await api.completeTask(item.id);
              onchange();
              onclose();
            } finally {
              busy = false;
            }
          }}
        >
          {item.status === "done" ? t("common.undo") : `✓ ${t("common.done")}`}
        </button>
      {/if}
      <button class="danger" class:arm={confirmDel} disabled={busy} onclick={askDelete}>
        {confirmDel ? t("mobile.delete_confirm") : t("common.delete")}
      </button>
    </div>
  </div>
</div>

<style>
  section {
    margin-bottom: 14px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 12px;
    color: var(--text-dim);
    font-weight: 600;
  }
  p {
    margin: 0;
    font-size: 14px;
    color: var(--text);
  }
  .note {
    white-space: pre-wrap;
  }
  .badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .badge {
    font-size: 12px;
    padding: 3px 9px;
    border-radius: 999px;
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text);
  }
  .badge.tag {
    color: var(--accent);
    border-color: var(--accent);
  }
  .typebadge {
    font-size: 12px;
    padding: 3px 9px;
    border-radius: 999px;
    color: var(--c);
    background: color-mix(in srgb, var(--c) 14%, transparent);
    flex: none;
  }
  .meta {
    font-size: 12px;
    color: var(--text-dim);
    margin-top: 4px;
  }
  .rohint {
    font-size: 12px;
    color: var(--text-dim);
    opacity: 0.75;
    margin: 2px 0 12px;
  }
  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .danger {
    appearance: none;
    border: 1px solid var(--danger);
    background: none;
    color: var(--danger);
    font: inherit;
    font-size: 15px;
    border-radius: var(--radius);
    padding: 11px;
  }
  .danger.arm {
    background: var(--danger);
    color: var(--accent-fg);
    font-weight: 700;
  }
</style>

<script lang="ts">
  /**
   * 条目详情面板（纯显示）：只读排版条目信息与图片，未填值的字段不显示。
   * 图片 = 自管理附件（粘贴截图等，mime image/*）双击放大预览；
   * 文件链接只展示路径。入口：各视图点击条目行（rowDetail）；
   * 「编辑」按钮进统一面板的编辑模式。
   */
  import {
    api,
    displayTitle,
    FILE_LINKS_KEY,
    fileLinksOf,
    fmtDate,
    fmtDateTime,
    typeLabel,
    type ItemStatus,
  } from "./api";
  import { recurrenceLabel } from "./recurrence";
  import { displayFieldMap, fieldDefMap } from "./fields.svelte";
  import { closePanel, openDetail, openEdit, panelRequest, type DetailRequest } from "./panel.svelte";
  import { toast } from "./toast.svelte";
  import { q, t } from "./i18n";

  // Shell 仅在 mode=detail 时挂载本组件
  const req = panelRequest() as DetailRequest;
  const src = req.item;

  /** 状态徽标文案（随界面语言） */
  const statusLabel = (s: ItemStatus) => t(s === "done" ? "detail.status.done" : "detail.status.todo");

  let previewUrl = $state("");
  /** 转换两步确认（SPRINT2-SPEC §7）："" | "event"（转日程）| "log"（生成记录）。
   *  确认态保持 8 秒：2.5 秒时用户还在读按钮文案就回退了，得从头再点。 */
  let confirmConvert = $state<"" | "event" | "log">("");
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  /** 待办转日程：新建日程承接内容（含附件），原待办删除 */
  async function convertToEvent() {
    if (confirmConvert !== "event") {
      confirmConvert = "event";
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmConvert = ""), 8000);
      return;
    }
    clearTimeout(confirmTimer);
    confirmConvert = "";
    try {
      const ev = await api.convertTaskToEvent(src.id);
      toast.show(t("detail.converted_to_event", { name: q(displayTitle(ev)) }));
      openDetail(ev);
    } catch (e) {
      toast.show(t("detail.convert_event_failed", { error: String(e) }));
    }
  }

  /** 日程生成记录：新记录 occurred_at = 原开始，原日程保留 */
  async function convertToLog() {
    if (confirmConvert !== "log") {
      confirmConvert = "log";
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmConvert = ""), 8000);
      return;
    }
    clearTimeout(confirmTimer);
    confirmConvert = "";
    try {
      const log = await api.eventToLog(src.id);
      toast.show(t("detail.converted_to_log", { name: q(displayTitle(log)) }));
      openDetail(log);
    } catch (e) {
      toast.show(t("detail.convert_log_failed", { error: String(e) }));
    }
  }

  /** 待办两态切换：详情页直达（改回未完成 / 标记完成），切换后关闭面板让列表刷新 */
  async function toggleDone() {
    if (src.status === "done") await api.updateItem(src.id, { status: "todo" });
    else await api.completeTask(src.id);
    closePanel();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // 预览打开时 Esc 只关预览，再按才关面板
      if (previewUrl) {
        previewUrl = "";
        return;
      }
      closePanel();
    }
  }

  // 图片附件 → asset URL（异步逐个取；文件缺失就跳过）
  let images = $state<{ id: number; url: string }[]>([]);
  $effect(() => {
    let cancelled = false;
    void (async () => {
      for (const a of src.attachments.filter((a) => a.mime.startsWith("image/"))) {
        try {
          const url = await api.attachmentUrl(a.rel_path);
          if (cancelled) return;
          images = [...images, { id: a.id, url }];
        } catch {
          /* 附件文件缺失：不显示 */
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  const otherAttachments = src.attachments.filter((a) => !a.mime.startsWith("image/"));
  const links = fileLinksOf(src);

  /** 非空字段行（未填值的不显示；软删字段的历史值仍展示——名字带「已删除」标记） */
  function fieldRows(): { name: string; value: string; deleted: boolean }[] {
    const byId = displayFieldMap();
    const out: { name: string; value: string; deleted: boolean }[] = [];
    for (const [id, v] of Object.entries(src.extra ?? {})) {
      if (id === FILE_LINKS_KEY) continue;
      const def = byId.get(id);
      if (!def) continue;
      const deleted = !fieldDefMap().has(id);
      if (v === null || v === undefined || v === "" || v === false) continue;
      if (Array.isArray(v) && v.length === 0) continue;
      let value: string;
      if (typeof v === "boolean") value = t("detail.bool_yes");
      else if (def.kind === "date") value = fmtDateTime(String(v));
      else if (def.kind === "number")
        value = String(v) + (def.options.unit ? ` ${def.options.unit}` : "");
      else if (Array.isArray(v)) value = v.join(t("detail.array_sep"));
      else value = String(v);
      out.push({
        name: deleted ? t("detail.field_deleted", { name: def.name }) : def.name,
        value,
        deleted,
      });
    }
    return out;
  }

  const fields = fieldRows();
</script>

<svelte:window onkeydown={onKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_interactive_supports_focus -->
<div class="overlay" onclick={closePanel} role="presentation">
  <div
    class="modal"
    role="dialog"
    tabindex="-1"
    aria-label={t("detail.title")}
    onclick={(e) => e.stopPropagation()}
  >
    <header>
      <span class="type-badge">{typeLabel(src.type)}</span>
      {#if src.type === "task" && src.status}
        <span class="status" class:done={src.status === "done"}>{statusLabel(src.status)}</span>
      {/if}
      <span class="meta">
        {t("detail.meta", {
          created: fmtDateTime(src.created_at),
          updated: fmtDateTime(src.updated_at),
        })}
      </span>
    </header>

    <h1 class="title">{displayTitle(src)}</h1>

    {#if src.recurrence}
      <p class="recurrence">{t("detail.recurrence_line", { rule: recurrenceLabel(src.recurrence) })}</p>
    {/if}

    {#if images.length}
      <div class="imgs">
        {#each images as im (im.id)}
          <img
            src={im.url}
            alt={t("detail.image_alt")}
            ondblclick={() => (previewUrl = im.url)}
          />
        {/each}
      </div>
    {/if}

    {#if
      (src.type === "event" && src.start_at) ||
      src.due_at ||
      src.occurred_at ||
      src.completed_at}
      <dl class="rows">
        {#if src.type === "event" && src.start_at}
          <div>
            <dt>{src.all_day ? t("detail.all_day") : t("detail.start")}</dt>
            <dd>{src.all_day ? fmtDate(src.start_at) : fmtDateTime(src.start_at)}</dd>
          </div>
          {#if !src.all_day && src.end_at}
            <div>
              <dt>{t("detail.end")}</dt>
              <dd>{fmtDateTime(src.end_at)}</dd>
            </div>
          {/if}
        {/if}
        {#if src.due_at}
          <div>
            <dt>{t("detail.due")}</dt>
            <dd>{src.due_all_day ? fmtDate(src.due_at) : fmtDateTime(src.due_at)}</dd>
          </div>
        {/if}
        {#if src.occurred_at}
          <div>
            <dt>{t("detail.occurred")}</dt>
            <dd>{fmtDateTime(src.occurred_at)}</dd>
          </div>
        {/if}
        {#if src.completed_at}
          <div>
            <dt>{t("detail.completed")}</dt>
            <dd>{fmtDateTime(src.completed_at)}</dd>
          </div>
        {/if}
      </dl>
    {/if}

    {#if fields.length}
      <dl class="rows">
        {#each fields as f (f.name)}
          <div class:deleted={f.deleted}>
            <dt>{f.name}</dt>
            <dd>{f.value}</dd>
          </div>
        {/each}
      </dl>
    {/if}

    {#if src.tags.length}
      <div class="row">
        <span class="dt">{t("detail.tags")}</span>
        <span class="tags">
          {#each src.tags as tag (tag)}<span class="tag">#{tag}</span>{/each}
        </span>
      </div>
    {/if}

    {#if src.reminders.length}
      <div class="row">
        <span class="dt">{t("detail.reminders")}</span>
        <span class="tags">
          {#each src.reminders as r (r.id)}
            <span class="rem">{r.spec.startsWith("@") ? r.spec : fmtDateTime(r.spec)}</span>
          {/each}
        </span>
      </div>
    {/if}

    {#if src.note}
      <p class="note">{src.note}</p>
    {/if}

    {#if links.length || otherAttachments.length}
      <div class="row">
        <span class="dt">{otherAttachments.length ? t("detail.files_attachments") : t("detail.files")}</span>
        <div class="paths">
          {#each links as p (p)}
            <code class="path" title={p}>{p}</code>
          {/each}
          {#each otherAttachments as a (a.id)}
            <code class="path" title={a.rel_path}>{a.rel_path}</code>
          {/each}
        </div>
      </div>
    {/if}

    <div class="actions">
      {#if src.type === "task"}
        <button class="ghost" onclick={convertToEvent}>
          {confirmConvert === "event" ? t("detail.confirm_convert_event") : t("detail.to_event")}
        </button>
      {:else if src.type === "event"}
        <button class="ghost" onclick={convertToLog}>
          {confirmConvert === "log" ? t("detail.confirm_convert_log") : t("detail.to_log")}
        </button>
      {/if}
      {#if src.type === "task" && src.status}
        <button class="ghost" onclick={toggleDone}>
          {src.status === "done" ? t("detail.mark_todo") : t("detail.mark_done")}
        </button>
      {/if}
      <button class="ghost" onclick={closePanel}>{t("common.close")}</button>
      <button class="primary" onclick={() => openEdit(src)}>{t("common.edit")}</button>
    </div>
  </div>
</div>

{#if previewUrl}
  <!-- 图片大图预览：点任意处 / Esc 关闭 -->
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="preview" role="presentation" onclick={() => (previewUrl = "")}>
    <img src={previewUrl} alt={t("detail.preview_alt")} />
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .modal {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 36px rgb(0 0 0 / 0.28);
    width: 520px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 48px);
    overflow: auto;
    padding: 18px 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .type-badge {
    color: var(--accent);
    font-size: 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 999px;
    padding: 2px 10px;
  }

  .status {
    font-size: 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 10px;
    color: var(--text-dim);
  }

  .status.done {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .meta {
    color: var(--text-dim);
    font-size: 11px;
    margin-left: auto;
  }

  .title {
    margin: 0;
    font-size: 18px;
    line-height: 1.4;
    overflow-wrap: anywhere;
  }

  .recurrence {
    margin: 4px 0 0;
    color: var(--text-dim);
    font-size: 13px;
  }

  .imgs {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .imgs img {
    height: 96px;
    border-radius: 8px;
    border: 1px solid var(--border);
    cursor: zoom-in;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
  }

  .rows > div {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }

  .rows > div.deleted dt,
  .rows > div.deleted dd {
    opacity: 0.55;
  }

  dt,
  .dt {
    color: var(--text-dim);
    font-size: 13px;
    min-width: 56px;
    flex-shrink: 0;
  }

  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }

  .row {
    display: flex;
    gap: 12px;
    align-items: baseline;
  }

  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
  }

  .tag {
    font-size: 12px;
    color: var(--text-dim);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 1px 8px;
  }

  .rem {
    font-size: 12px;
    color: var(--accent);
    border: 1px dashed color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 999px;
    padding: 1px 8px;
  }

  .note {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 12px;
  }

  .paths {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .path {
    font-size: 12px;
    color: var(--text-dim);
    background: color-mix(in srgb, var(--text) 8%, transparent);
    padding: 2px 8px;
    border-radius: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }

  .preview {
    position: fixed;
    inset: 0;
    z-index: 300;
    background: rgb(0 0 0 / 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: zoom-out;
  }

  .preview img {
    max-width: min(92vw, 1200px);
    max-height: 92vh;
    border-radius: 8px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
  }
</style>

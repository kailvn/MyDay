<script lang="ts">
  /**
   * 条目编辑弹层（移动端编辑面）：标题 / 备注 / 时间（按类型）/ 提醒（多行）/
   * 标签。重复条目 = 「修改全部」语义（只动这一期可走桌面编辑面板的作用域
   * 二选一，或详情的拆为单次 / 删除这一期）；字段（extra）与重复规则仍只在桌面编辑。
   * 保存 = 只把改动过的键放进 ItemPatch（无改动不写库）。
   */
  import { t } from "../lib/i18n";
  import { api, type Item, type ItemPatch } from "../lib/api";
  import { toast } from "../lib/toast.svelte";

  let { item, onclose, onsaved }: { item: Item; onclose: () => void; onsaved: () => void } = $props();

  const pad2 = (n: number) => String(n).padStart(2, "0");
  /** ISO → 本地 { day, hm } 两个 input 值（date / time 分开渲染，移动端原生选择器） */
  function splitIso(iso: string | null): { day: string; hm: string } {
    if (!iso) return { day: "", hm: "" };
    const d = new Date(iso);
    return {
      day: `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`,
      hm: `${pad2(d.getHours())}:${pad2(d.getMinutes())}`,
    };
  }
  function combine(dayStr: string, hmStr: string): string {
    const [y, m, d] = dayStr.split("-").map(Number);
    const [hh, mi] = hmStr.split(":").map(Number);
    return new Date(y, m - 1, d, hh || 0, mi || 0).toISOString();
  }
  function dayStart(dayStr: string): string {
    const [y, m, d] = dayStr.split("-").map(Number);
    return new Date(y, m - 1, d).toISOString();
  }

  // 组件由 App.svelte 的 {#if editing} 按次挂载，item 在生命周期内不可变：
  // 以下顶层引用均为有意取挂载时刻的初值（表单种子 / save() 的 diff 基线）。
  // svelte-ignore state_referenced_locally
  let title = $state(item.title ?? "");
  // svelte-ignore state_referenced_locally
  let note = $state(item.note ?? "");
  // svelte-ignore state_referenced_locally
  let tags = $state(item.tags.join(", "));
  let saving = $state(false);

  // ---- 时间（按类型预填自条目现值） --------------------------------------
  // svelte-ignore state_referenced_locally
  const start0 = splitIso(item.start_at);
  // svelte-ignore state_referenced_locally
  const end0 = splitIso(item.end_at);
  // svelte-ignore state_referenced_locally
  const due0 = splitIso(item.due_at);
  // svelte-ignore state_referenced_locally
  const occurred0 = splitIso(item.occurred_at);
  // svelte-ignore state_referenced_locally
  let allDay = $state(item.type === "event" ? item.all_day : item.due_all_day);
  let day = $state(start0.day || due0.day || occurred0.day);
  let startHm = $state(start0.hm || "09:00");
  let endHm = $state(end0.hm || "10:00");
  let dueHm = $state(due0.hm || "18:00");
  /** 待办清空截止 → 退回未排期池（与桌面「无日期」同口径） */
  // svelte-ignore state_referenced_locally
  let dueNone = $state(item.type === "task" && !item.due_at);
  let occurredHm = $state(occurred0.hm || "12:00");

  // ---- 提醒（多行编辑，保存 = 整体替换）：能表达为「类型锚 + 档位」的行给具体
  // 档位；@dailyT / 绝对时刻 / 锚与类型不符的 spec 以原文展示（只可原样保留或删除）；
  // 零行 = 显式「不提醒」。保存与现值 diff，无改动不写库。 --------------------
  type RemChoice = "pt" | "m5" | "m15" | "m30" | "h1" | "d1";
  type RemRow = { choice: RemChoice | null; raw: string; mode: "notify" | "alarm" };
  const REM_OFFSET: Partial<Record<RemChoice, string>> = {
    pt: "",
    m5: "-5m",
    m15: "-15m",
    m30: "-30m",
    h1: "-1h",
    d1: "-1d",
  };
  const REM_LABEL: Record<RemChoice, string> = $derived({
    pt: t("mobile.rem_pt"),
    m5: t("mobile.rem_m5"),
    m15: t("mobile.rem_m15"),
    m30: t("mobile.rem_m30"),
    h1: t("mobile.rem_h1"),
    d1: t("mobile.rem_d1"),
  });
  const SPECIFIC_CHOICES: RemChoice[] = ["pt", "m5", "m15", "m30", "h1", "d1"];
  function remRowFrom(r: { spec: string; channel: string }): RemRow {
    const mode = r.channel === "alarm" ? "alarm" : "notify";
    const m = /^@(start|due)(-[0-9]+[mhd])?$/.exec(r.spec);
    const hit = m
      ? (Object.entries(REM_OFFSET) as [RemChoice, string][]).find(([, v]) => v === (m[2] ?? ""))
      : undefined;
    if (m && hit && m[1] === (item.type === "task" ? "due" : "start")) {
      return { choice: hit[0], raw: "", mode };
    }
    return { choice: null, raw: r.spec, mode };
  }
  // svelte-ignore state_referenced_locally
  let remRows = $state<RemRow[]>(item.reminders.map(remRowFrom));

  /** 追加一档：默认取未被占用的第一个档位，全占用则回落 15 分钟前 */
  function addRemRow() {
    const used = new Set(remRows.map((r) => r.choice));
    const choice = SPECIFIC_CHOICES.find((c) => !used.has(c)) ?? "m15";
    remRows = [...remRows, { choice, raw: "", mode: "notify" }];
  }
  function removeRemRow(i: number) {
    remRows = remRows.filter((_, x) => x !== i);
  }

  // svelte-ignore state_referenced_locally
  const isLog = item.type === "log";
  let canSave = $derived(isLog || title.trim().length > 0);

  function parseTags(v: string): string[] {
    return [...new Set(v.split(/[,，、\s]+/).map((s) => s.trim()).filter(Boolean))];
  }
  const sameInstant = (a: string | null, b: string | null) =>
    (a === null && b === null) || (a !== null && b !== null && Date.parse(a) === Date.parse(b));

  async function save() {
    if (!canSave || saving) return;
    saving = true;
    try {
      const patch: ItemPatch = {};
      if (title.trim() !== (item.title ?? "")) patch.title = title.trim() || null;
      if (note.trim() !== (item.note ?? "")) patch.note = note.trim() || null;
      const nextTags = parseTags(tags);
      if (nextTags.join(",") !== item.tags.join(",")) patch.tags = nextTags;

      if (item.type === "event") {
        const start = allDay ? dayStart(day) : combine(day, startHm);
        const end = allDay ? dayStart(day) : combine(day, endHm);
        if (!sameInstant(start, item.start_at)) patch.start_at = start;
        if (!sameInstant(end, item.end_at)) patch.end_at = end;
        if (allDay !== item.all_day) patch.all_day = allDay;
      } else if (item.type === "task") {
        if (dueNone) {
          if (item.due_at) patch.clear_due_at = true;
        } else {
          const due = allDay ? dayStart(day) : combine(day, dueHm);
          if (!sameInstant(due, item.due_at)) patch.due_at = due;
          if (allDay !== item.due_all_day) patch.due_all_day = allDay;
        }
      } else {
        const occurred = combine(day, occurredHm);
        if (!sameInstant(occurred, item.occurred_at)) patch.occurred_at = occurred;
      }

      if (!isLog) {
        const anchor = item.type === "task" ? "due" : "start";
        const next = remRows.map((r) => ({
          spec: r.choice ? `@${anchor}${REM_OFFSET[r.choice] ?? ""}` : r.raw,
          channel: r.mode,
        }));
        const remChanged =
          next.length !== item.reminders.length ||
          next.some(
            (r, i) =>
              r.spec !== item.reminders[i].spec || r.channel !== item.reminders[i].channel,
          );
        if (remChanged) {
          if (next.length === 0) patch.clear_reminders = true;
          else patch.reminders = next;
        }
      }

      if (Object.keys(patch).length === 0) {
        onclose();
        return;
      }
      await api.updateItem(item.id, patch);
      onsaved();
      onclose();
    } catch (e) {
      toast.show(String(e));
    } finally {
      saving = false;
    }
  }
</script>

<div class="m-backdrop" onclick={onclose} aria-hidden="true"></div>
<div class="m-sheet" role="dialog" aria-label={t("common.edit")}>
  <div class="m-grip"></div>
  <div class="m-sheet-head">
    <h2 class="m-sheet-title">{t("common.edit")}</h2>
    <button class="m-iconbtn" onclick={onclose}>✕</button>
  </div>
  <div class="m-sheet-body">
    <input class="m-input" type="text" bind:value={title} placeholder={t("mobile.quickadd_placeholder")} />

    <textarea
      class="m-input"
      style="margin-top: 8px; min-height: 60px; resize: vertical;"
      bind:value={note}
      placeholder={t("mobile.note_placeholder")}
    ></textarea>

    {#if item.type === "log"}
      <div class="m-formrow">
        <span class="lbl">{t("mobile.occurred")}</span>
        <input type="date" bind:value={day} />
        <input type="time" bind:value={occurredHm} />
      </div>
    {:else}
      <div class="m-formrow">
        <span class="lbl">{t("calendar.all_day")}</span>
        <input class="m-switch" type="checkbox" bind:checked={allDay} />
      </div>
      <div class="m-formrow">
        <span class="lbl">{t("mobile.date")}</span>
        <input type="date" bind:value={day} />
      </div>
      {#if !allDay}
        <div class="m-formrow">
          {#if item.type === "task"}
            <span class="lbl">{t("mobile.due")}</span>
            <input type="time" bind:value={dueHm} />
          {:else}
            <span class="lbl">{t("mobile.start")}</span>
            <input type="time" bind:value={startHm} />
            <span class="lbl">{t("mobile.end")}</span>
            <input type="time" bind:value={endHm} />
          {/if}
        </div>
      {/if}
      {#if item.type === "task"}
        <div class="m-formrow">
          <span class="lbl">{t("mobile.due_none")}</span>
          <input class="m-switch" type="checkbox" bind:checked={dueNone} />
        </div>
      {/if}
      <div class="m-formrow">
        <span class="lbl">{t("mobile.reminder")}</span>
        {#if remRows.length === 0}
          <span class="rem-none">{t("mobile.rem_none")}</span>
        {/if}
      </div>
      {#each remRows as row, i (i)}
        <div class="m-formrow">
          <span class="lbl"></span>
          {#if row.choice}
            <select bind:value={row.choice} aria-label={t("mobile.reminder")}>
              {#each SPECIFIC_CHOICES as c (c)}
                <option value={c}>{REM_LABEL[c]}</option>
              {/each}
            </select>
          {:else}
            <span class="rem-raw" title={row.raw}>{row.raw}</span>
          {/if}
          <select bind:value={row.mode} aria-label={t("mobile.rem_mode")}>
            <option value="notify">🔔 {t("panel.rem.kindNotify")}</option>
            <option value="alarm">⏰ {t("panel.rem.kindAlarm")}</option>
          </select>
          <button class="m-iconbtn" onclick={() => removeRemRow(i)}>×</button>
        </div>
      {/each}
      <button class="m-chip rem-add" onclick={addRemRow}>＋ {t("mobile.rem_add")}</button>
    {/if}

    <div class="m-formrow">
      <span class="lbl">{t("mobile.detail_tags")}</span>
      <input class="m-input" type="text" style="flex: 1;" bind:value={tags} placeholder={t("mobile.tags_placeholder")} />
    </div>

    {#if item.recurrence}
      <p class="m-hint">{t("mobile.edit_recurrence_hint")}</p>
    {/if}

    <button class="m-primary" style="margin-top: 12px;" disabled={!canSave || saving} onclick={save}>
      {t("common.save")}
    </button>
  </div>
</div>

<style>
  .m-formrow select {
    flex: 1;
    min-width: 0;
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 8px 10px;
  }
  .rem-none {
    flex: 1;
    font-size: 13px;
    color: var(--text-dim);
  }
  .rem-raw {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--text-dim);
    align-self: center;
  }
  .rem-add {
    margin: 8px 0 2px;
    align-self: flex-start;
  }
</style>

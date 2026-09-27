<script lang="ts">
  /**
   * 快速添加（Outlook 新建事件的弹层形态 + 我们的特色：单行自然语言）。
   * 标题里识别时间词（复用桌面 timewords）自动预填日期/时刻；手动改过时间
   * 字段后停止自动预填。类型三选：日程（开始+结束+提醒）/ 待办（截止+提醒）/
   * 记录（模板 + 字段表单，体重/服药这类）。模板与字段定义随同步从桌面来，
   * 手机端只消费不生产。
   */
  import { onMount } from "svelte";
  import { t } from "../lib/i18n";
  import { api, type FieldDef, type ItemType, type NewReminder, type Template } from "../lib/api";
  import { fieldDefMap } from "../lib/fields.svelte";
  import { parseTimeHint, type TimeHint } from "../lib/timewords";

  let {
    defaultDay, // YYYY-MM-DD 或 null（FAB 全局添加）
    onclose,
    onsaved,
  }: {
    defaultDay: string | null;
    onclose: () => void;
    onsaved: () => void;
  } = $props();

  const pad2 = (n: number) => String(n).padStart(2, "0");
  const todayKey = (() => {
    const d = new Date();
    return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
  })();
  const nowHm = (() => {
    const d = new Date();
    return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
  })();

  let title = $state("");
  let type = $state<ItemType>("event");
  let allDay = $state(false);
  // svelte-ignore state_referenced_locally
  // 弹层按 {#if} 挂载，每次打开都是新实例，只取初始值是预期行为
  let day = $state(defaultDay ?? todayKey);
  let startHm = $state("09:00");
  let endHm = $state("10:00");
  let logHm = $state(nowHm); // 记录发生时刻（默认现在）
  let saving = $state(false);
  /** 用户手动改过时间字段 / 类型后，不再随标题识别自动预填 */
  let manual = $state(false);
  let typeTouched = $state(false);

  // ---- 记录模板（随同步下发；手机只消费） --------------------------------
  let templates = $state<Template[]>([]);
  let tplId = $state<string | null>(null);
  let fieldVals = $state<Record<string, unknown>>({});
  let lastDefTitle = $state("");

  onMount(async () => {
    try {
      templates = (await api.listTemplates()).filter((tp) => tp.item_type === "log");
    } catch {
      templates = []; // 模板拉不到不阻塞快速添加
    }
  });

  const tpl = $derived(templates.find((tp) => tp.id === tplId) ?? null);
  /** 模板声明了哪些字段；选项/单位以物化的 field_defs 为准（缺失退回内联定义） */
  const tplFields = $derived.by<FieldDef[]>(() => {
    if (!tpl) return [];
    const defs = fieldDefMap();
    return (tpl.fields as FieldDef[]).map((f) => defs.get(f.id) ?? f);
  });

  function pickTemplate(id: string | null) {
    tplId = id;
    fieldVals = {};
    const tp = templates.find((x) => x.id === id);
    const defTitle = tp ? ((tp.defaults?.["title"] as string) ?? tp.name) : "";
    // 标题仍是上一个模板的默认值（用户没改过）才跟随替换
    if (!title.trim() || title === lastDefTitle) title = defTitle;
    lastDefTitle = defTitle;
  }

  function setVal(id: string, v: unknown) {
    fieldVals = { ...fieldVals, [id]: v };
  }
  function toggleMulti(id: string, choice: string) {
    const cur = Array.isArray(fieldVals[id]) ? (fieldVals[id] as string[]) : [];
    setVal(id, cur.includes(choice) ? cur.filter((c) => c !== choice) : [...cur, choice]);
  }

  // ---- 提醒选择（日程锚开始 / 待办锚截止；同类软件惯例的档位） -----------
  type RemChoice = "default" | "none" | "pt" | "m5" | "m15" | "m30" | "h1" | "d1";
  let reminder = $state<RemChoice>("default");
  const REM_OFFSET: Partial<Record<RemChoice, string>> = {
    pt: "",
    m5: "-5m",
    m15: "-15m",
    m30: "-30m",
    h1: "-1h",
    d1: "-1d",
  };
  const REM_LABEL: Record<RemChoice, string> = $derived({
    default: t("mobile.rem_default"),
    none: t("mobile.rem_none"),
    pt: t("mobile.rem_pt"),
    m5: t("mobile.rem_m5"),
    m15: t("mobile.rem_m15"),
    m30: t("mobile.rem_m30"),
    h1: t("mobile.rem_h1"),
    d1: t("mobile.rem_d1"),
  });

  function buildReminders(): { reminders: NewReminder[]; skip_default_reminder?: boolean } {
    if (reminder === "default") return { reminders: [] };
    if (reminder === "none") return { reminders: [], skip_default_reminder: true };
    const anchor = type === "task" ? "due" : "start";
    return {
      reminders: [{ spec: `@${anchor}${REM_OFFSET[reminder] ?? ""}`, channel: "notify" }],
    };
  }

  // 标题实时识别（≤60 字，与桌面同一解析器）
  const hint = $derived<TimeHint | null>(parseTimeHint(title));

  $effect(() => {
    if (manual || !hint) return;
    const d = hint.date;
    day = `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
    if (!hint.dateOnly) {
      const hh = pad2(d.getHours());
      const mi = pad2(d.getMinutes());
      startHm = `${hh}:${mi}`;
      const end = new Date(d.getTime() + 60 * 60 * 1000);
      endHm = `${pad2(end.getHours())}:${pad2(end.getMinutes())}`;
      allDay = false;
      // 有具体钟点 → 默认日程；只有日期词 → 默认待办（用户点过类型则尊重）
      if (!typeTouched) type = "event";
    } else {
      allDay = true;
      if (!typeTouched) type = "task";
    }
  });

  function focusOnMount(node: HTMLInputElement) {
    node.focus();
  }

  function clearHint() {
    if (!hint) return;
    title = title.replace(hint.matched, "").replace(/\s{2,}/g, " ").trim();
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

  let canSave = $derived(title.trim().length > 0 || type === "log");

  async function save() {
    if (!canSave || saving) return;
    saving = true;
    try {
      const trimmed = title.trim() || null;
      const base = {
        item_type: type,
        title: trimmed,
        idempotency_key: crypto.randomUUID(),
      };
      if (type === "event") {
        await api.addItem({
          ...base,
          start_at: combine(day, startHm),
          end_at: combine(day, endHm),
          all_day: allDay,
          ...buildReminders(),
        });
      } else if (type === "task") {
        await api.addItem({
          ...base,
          due_at: allDay ? dayStart(day) : combine(day, startHm),
          due_all_day: allDay,
          ...buildReminders(),
        });
      } else {
        const tp = templates.find((x) => x.id === tplId) ?? null;
        // 空串字段不进 extra（留空 = 不填该项）
        const extra = Object.fromEntries(
          Object.entries(fieldVals).filter(([, v]) => v !== undefined && v !== "" && v !== null),
        );
        await api.addItem({
          ...base,
          title: trimmed ?? (tp ? ((tp.defaults?.["title"] as string) ?? tp.name) : null),
          occurred_at: combine(day, logHm),
          template_id: tp?.id ?? null,
          tags: tp?.tag ? [tp.tag] : [],
          extra,
        });
      }
      onsaved();
      onclose();
    } finally {
      saving = false;
    }
  }
</script>

<div class="m-backdrop" onclick={onclose} aria-hidden="true"></div>
<div class="m-sheet" role="dialog" aria-label={t("mobile.quickadd_title")}>
  <div class="m-grip"></div>
  <div class="m-sheet-head">
    <h2 class="m-sheet-title">{t("mobile.quickadd_title")}</h2>
    <button class="m-iconbtn" onclick={onclose}>✕</button>
  </div>
  <div class="m-sheet-body">
    <input
      class="m-input"
      type="text"
      placeholder={t("mobile.quickadd_placeholder")}
      bind:value={title}
      use:focusOnMount
    />

    {#if hint}
      <p class="parsehit">
        <span class="m-chip">
          ⏱ {hint.label}
          <button onclick={clearHint}>{t("mobile.quickadd_parse_clear")}</button>
        </span>
      </p>
    {:else if title.trim()}
      <p class="m-hint">{t("mobile.quickadd_hint_time")}</p>
    {/if}

    <div class="m-seg" role="tablist" style="margin: 10px 0;">
      <button aria-pressed={type === "event"} onclick={() => { type = "event"; typeTouched = true; }}>{t("type.event")}</button>
      <button aria-pressed={type === "task"} onclick={() => { type = "task"; typeTouched = true; }}>{t("type.task")}</button>
      <button aria-pressed={type === "log"} onclick={() => { type = "log"; typeTouched = true; }}>{t("type.log")}</button>
    </div>

    {#if type === "log"}
      {#if templates.length}
        <div class="tplchips">
          <button class="m-chip" class:on={!tplId} onclick={() => pickTemplate(null)}>{t("mobile.tpl_none")}</button>
          {#each templates as tp (tp.id)}
            <button class="m-chip" class:on={tplId === tp.id} onclick={() => pickTemplate(tp.id)}>
              {#if tp.icon}{tp.icon}{/if} {tp.name}
            </button>
          {/each}
        </div>
      {/if}
      {#if tpl && tplFields.length}
        <div class="fields">
          {#each tplFields as f (f.id)}
            <div class="m-formrow">
              <span class="lbl">{f.name}</span>
              {#if f.kind === "number"}
                <input
                  type="number"
                  inputmode="decimal"
                  value={fieldVals[f.id] ?? ""}
                  oninput={(e) => setVal(f.id, e.currentTarget.value === "" ? undefined : Number(e.currentTarget.value))}
                />
                {#if f.options?.unit}<span class="unit">{f.options.unit}</span>{/if}
              {:else if f.kind === "select"}
                <select
                  value={String(fieldVals[f.id] ?? "")}
                  onchange={(e) => setVal(f.id, e.currentTarget.value || undefined)}
                >
                  <option value="">—</option>
                  {#each f.options?.choices ?? [] as c}
                    <option value={c}>{c}</option>
                  {/each}
                </select>
              {:else if f.kind === "multiselect"}
                <div class="multis">
                  {#each f.options?.choices ?? [] as c}
                    <button
                      class="m-chip"
                      class:on={Array.isArray(fieldVals[f.id]) && (fieldVals[f.id] as string[]).includes(c)}
                      onclick={() => toggleMulti(f.id, c)}
                    >{c}</button>
                  {/each}
                </div>
              {:else if f.kind === "bool"}
                <input
                  class="m-switch"
                  type="checkbox"
                  checked={fieldVals[f.id] === true}
                  onchange={(e) => setVal(f.id, e.currentTarget.checked)}
                />
              {:else if f.kind === "date"}
                <input
                  type="date"
                  value={String(fieldVals[f.id] ?? "")}
                  onchange={(e) => setVal(f.id, e.currentTarget.value || undefined)}
                />
              {:else}
                <input
                  type={f.kind === "url" ? "url" : "text"}
                  value={String(fieldVals[f.id] ?? "")}
                  oninput={(e) => setVal(f.id, e.currentTarget.value || undefined)}
                />
              {/if}
            </div>
          {/each}
        </div>
      {/if}
      <div class="m-formrow">
        <span class="lbl">{t("mobile.occurred")}</span>
        <input type="date" bind:value={day} oninput={() => (manual = true)} />
        <input type="time" bind:value={logHm} oninput={() => (manual = true)} />
      </div>
    {:else}
      <div class="m-formrow">
        <span class="lbl">{t("calendar.all_day")}</span>
        <input class="m-switch" type="checkbox" bind:checked={allDay} />
      </div>
      <div class="m-formrow">
        <span class="lbl">{t("mobile.date")}</span>
        <input
          type="date"
          bind:value={day}
          oninput={() => (manual = true)}
        />
      </div>
      {#if !allDay}
        <div class="m-formrow">
          <span class="lbl">{type === "task" ? t("mobile.due") : t("mobile.start")}</span>
          <input type="time" bind:value={startHm} oninput={() => (manual = true)} />
          {#if type === "event"}
            <span class="lbl">{t("mobile.end")}</span>
            <input type="time" bind:value={endHm} oninput={() => (manual = true)} />
          {/if}
        </div>
      {/if}
      <div class="m-formrow">
        <span class="lbl">{t("mobile.reminder")}</span>
        <select bind:value={reminder}>
          {#each Object.entries(REM_LABEL) as [id, label] (id)}
            <option value={id}>{label}</option>
          {/each}
        </select>
      </div>
    {/if}

    <button class="m-primary" style="margin-top: 12px;" disabled={!canSave || saving} onclick={save}>
      {t("common.save")}
    </button>
  </div>
</div>

<style>
  .tplchips {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin: 10px 0 2px;
  }
  .tplchips .m-chip.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .fields {
    margin-top: 4px;
  }
  .unit {
    font-size: 12px;
    color: var(--text-dim);
    flex: none;
  }
  .multis {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    flex: 1;
  }
  .multis .m-chip.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
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
</style>

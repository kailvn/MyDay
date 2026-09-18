<script lang="ts">
  /**
   * 规则行列表（FILTER-SPEC §11 共享组件）：
   * 「＋ 添加条件」先选字段（FieldMenu，Anytype 式）→ 行 = 字段下拉 → 运算符下拉
   * （随 §4.3 矩阵出集）→ 值控件（随运算符切换，构造不出非法组合）。
   * 标签等自由多值 = chip 输入 + 已有标签建议；「具体日期」默认今天。
   * 筛选编辑器与挂件编辑器的同一套行组件——一致性靠复用保证。
   */
  import type { Cmp, FieldDef, FilterValue, ItemType } from "../api";
  import { t } from "../i18n";
  import {
    BUILTIN_COLUMNS,
    CMP_MATRIX,
    REL_DAY_LABELS,
    REL_RANGE_LABELS,
    defaultValueFor,
    fieldApplicable,
    roleOf,
    valueControlOf,
    type RuleRow,
  } from "../viewmodel";
  import FieldMenu from "./FieldMenu.svelte";

  let {
    rows = $bindable(),
    fields,
    templates = [],
    itemType = "all",
    tagSuggestions = [],
  }: {
    rows: RuleRow[];
    fields: FieldDef[];
    templates?: { id: string; name: string }[];
    itemType?: "all" | ItemType;
    /** 自由多值的候选建议（来自当前结果里的已有标签） */
    tagSuggestions?: string[];
  } = $props();

  const isNoValue = (row: RuleRow) => row.value == null && ["empty", "not_empty", "is_true", "is_false"].includes(row.cmp);

  let menuOpen = $state(false);

  function fieldMenuGroups() {
    return [
      { label: t("vm.field.groupBuiltin"), options: BUILTIN_COLUMNS.map((c) => ({ id: c.id, label: t(c.label) })) },
      {
        label: t("vm.field.groupCustom"),
        options: fields
          .filter((f) => fieldApplicable(f.id, itemType, fields))
          .map((f) => ({ id: f.id, label: f.name })),
      },
    ];
  }

  function addField(field: string) {
    const cmp = CMP_MATRIX[roleOf(field, fields) ?? "text"][0].id;
    rows.push({ field, cmp, value: defaultValueFor(field, cmp, fields) });
    menuOpen = false;
  }

  function onFieldChange(row: RuleRow, e: Event) {
    row.field = (e.target as HTMLSelectElement).value;
    const cmps = CMP_MATRIX[roleOf(row.field, fields) ?? "text"];
    row.cmp = cmps[0].id;
    row.value = defaultValueFor(row.field, row.cmp, fields);
  }

  function onCmpChange(row: RuleRow, e: Event) {
    row.cmp = (e.target as HTMLSelectElement).value as Cmp;
    row.value = defaultValueFor(row.field, row.cmp, fields);
  }

  const controlOf = (row: RuleRow) => valueControlOf(roleOf(row.field, fields) ?? "text", row.cmp);
  const choicesOf = (field: string): string[] => {
    const def = fields.find((f) => f.id === field);
    return (def?.options?.choices as string[] | undefined) ?? [];
  };

  /** 日期值控件状态：{day} ⇄ {rel} */
  type DateCtl = { mode: "day"; day: string } | { mode: "rel"; rel: string };
  const localToday = () => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  };
  function toDateCtl(v: FilterValue | null): DateCtl {
    if (v && typeof v === "object" && "day" in v) return { mode: "day", day: v.day };
    if (v && typeof v === "object" && "rel" in v) return { mode: "rel", rel: v.rel };
    return { mode: "rel", rel: "today" };
  }
  function applyDateCtl(row: RuleRow, ctl: DateCtl, key?: "value" | "from" | "to") {
    const v: FilterValue = ctl.mode === "day" && ctl.day ? { day: ctl.day } : { rel: (ctl.mode === "rel" ? ctl.rel : "today") as never };
    if (key === undefined) row.value = v;
    else if (row.value && typeof row.value === "object" && "from" in row.value) {
      (row.value as { from: FilterValue; to: FilterValue })[key] = v;
    }
  }
  /** 切到「具体日期」时默认今天，不留空值（空 day 查不到任何条目） */
  const dayCtl = (cur: DateCtl): DateCtl =>
    cur.mode === "day" && cur.day ? cur : { mode: "day", day: localToday() };
  const ctlOf = (v: FilterValue | null | undefined, key?: "from" | "to"): DateCtl => {
    if (key && v && typeof v === "object" && "from" in v) {
      return toDateCtl((v as { from: FilterValue; to: FilterValue })[key]);
    }
    return toDateCtl((v as FilterValue) ?? null);
  };

  // —— 自由多值（标签）chip 输入 ——
  function tagAdd(row: RuleRow, text: string) {
    const v = text.trim().replace(/[,，]+$/, "");
    if (!v) return;
    const list = new Set(Array.isArray(row.value) ? (row.value as string[]) : []);
    list.add(v);
    row.value = [...list];
  }
  function tagRemove(row: RuleRow, i: number) {
    const list = [...((Array.isArray(row.value) ? row.value : []) as string[])];
    list.splice(i, 1);
    row.value = list;
  }
</script>

{#each rows as row, i (i)}
  <div class="rule" data-testid="filter-rule">
    <select class="fld" value={row.field} onchange={(e) => onFieldChange(row, e)}>
      <optgroup label={t("vm.field.groupBuiltin")}>
        {#each BUILTIN_COLUMNS as c (c.id)}
          <option value={c.id}>{t(c.label)}</option>
        {/each}
      </optgroup>
      <optgroup label={t("vm.field.groupCustom")}>
        {#each fields.filter((f) => fieldApplicable(f.id, itemType, fields)) as f (f.id)}
          <option value={f.id}>{f.name}</option>
        {/each}
      </optgroup>
    </select>
    <select class="cmp" value={row.cmp} onchange={(e) => onCmpChange(row, e)}>
      {#each CMP_MATRIX[roleOf(row.field, fields) ?? "text"] as c (c.id)}
        <option value={c.id}>{t(c.label)}</option>
      {/each}
    </select>

    {#if !isNoValue(row)}
      {#if controlOf(row) === "text"}
        <input class="val" value={String(row.value ?? "")}
          oninput={(e) => (row.value = (e.target as HTMLInputElement).value)} />
      {:else if controlOf(row) === "number"}
        <input class="val num" type="number" step="any" value={Number(row.value ?? 0)}
          oninput={(e) => (row.value = Number((e.target as HTMLInputElement).value))} />
      {:else if controlOf(row) === "numpair"}
        <span class="pair">
          <input type="number" step="any" value={Number((row.value as {from:number})?.from ?? 0)}
            oninput={(e) => { (row.value as {from:number}).from = Number((e.target as HTMLInputElement).value); }} />
          <span>~</span>
          <input type="number" step="any" value={Number((row.value as {to:number})?.to ?? 0)}
            oninput={(e) => { (row.value as {to:number}).to = Number((e.target as HTMLInputElement).value); }} />
        </span>
      {:else if controlOf(row) === "select"}
        <select class="val" value={String(row.value ?? "")}
          onchange={(e) => (row.value = (e.target as HTMLSelectElement).value)}>
          {#each choicesOf(row.field) as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      {:else if controlOf(row) === "status"}
        <select class="val" value={String(row.value ?? "todo")}
          onchange={(e) => (row.value = (e.target as HTMLSelectElement).value)}>
          <option value="todo">{t("vm.status.todo")}</option>
          <option value="done">{t("vm.status.done")}</option>
        </select>
      {:else if controlOf(row) === "type"}
        <select class="val" value={String(row.value ?? "task")}
          onchange={(e) => (row.value = (e.target as HTMLSelectElement).value)}>
          <option value="event">{t("type.event")}</option>
          <option value="task">{t("type.task")}</option>
          <option value="log">{t("type.log")}</option>
        </select>
      {:else if controlOf(row) === "template"}
        <select class="val" value={String(row.value ?? "")}
          onchange={(e) => (row.value = (e.target as HTMLSelectElement).value)}>
          <option value="">{t("vm.rule.pickTemplate")}</option>
          {#each templates as tp (tp.id)}<option value={tp.id}>{tp.name}</option>{/each}
        </select>
      {:else if controlOf(row) === "multiselect"}
        {@const choices = row.field === "col:tags" ? [] : choicesOf(row.field)}
        {#if choices.length}
          <span class="checks">
            {#each choices as c (c)}
              <label>
                <input
                  type="checkbox"
                  checked={Array.isArray(row.value) && (row.value as string[]).includes(c)}
                  onchange={(e) => {
                    const list = new Set(Array.isArray(row.value) ? (row.value as string[]) : []);
                    if ((e.target as HTMLInputElement).checked) list.add(c);
                    else list.delete(c);
                    row.value = [...list];
                  }}
                />{c}
              </label>
            {/each}
          </span>
        {:else}
          <!-- 标签等自由多值：chip 输入 + 建议点选（Anytype 式） -->
          <span class="taginput">
            {#if Array.isArray(row.value)}
              {#each row.value as tag, ti (tag + ti)}
                <span class="tag">{tag}
                  <button type="button" title={t("vm.rule.removeTag")} onclick={() => tagRemove(row, ti)}>×</button>
                </span>
              {/each}
            {/if}
            <input
              placeholder={Array.isArray(row.value) && row.value.length ? "" : t("vm.rule.tagPlaceholder")}
              onkeydown={(e) => {
                const el = e.currentTarget as HTMLInputElement;
                if (e.key === "Enter" || e.key === "," || e.key === "，") {
                  e.preventDefault();
                  tagAdd(row, el.value);
                  el.value = "";
                } else if (e.key === "Backspace" && el.value === "") {
                  const n = Array.isArray(row.value) ? row.value.length : 0;
                  if (n > 0) tagRemove(row, n - 1);
                }
              }}
              onblur={(e) => {
                const el = e.currentTarget as HTMLInputElement;
                tagAdd(row, el.value);
                el.value = "";
              }} />
          </span>
          {#if tagSuggestions.length}
            <span class="sugs">
              {#each tagSuggestions.filter((s) => !Array.isArray(row.value) || !(row.value as string[]).includes(s)) as s (s)}
                <button type="button" class="sug" title={t("vm.rule.addTag")} onclick={() => tagAdd(row, s)}>+ {s}</button>
              {/each}
            </span>
          {/if}
        {/if}
      {:else if controlOf(row) === "date"}
        <span class="date-ctl">
          <select class="mode" value={ctlOf(row.value).mode === "day" ? "day" : ctlOf(row.value).rel}
            onchange={(e) => {
              const v = (e.target as HTMLSelectElement).value;
              applyDateCtl(row, v === "day" ? dayCtl(ctlOf(row.value)) : { mode: "rel", rel: v });
            }}>
            <option value="day">{t("vm.rule.specificDay")}</option>
            {#each REL_DAY_LABELS as r (r.label)}<option value={String((r.id as { rel: string }).rel)}>{t(r.label)}</option>{/each}
          </select>
          {#if ctlOf(row.value).mode === "day"}
            <input type="date" value={(ctlOf(row.value) as { day: string }).day}
              oninput={(e) => applyDateCtl(row, { mode: "day", day: (e.target as HTMLInputElement).value })} />
          {/if}
        </span>
      {:else if controlOf(row) === "datepair"}
        <span class="date-ctl">
          <select class="mode" value={(row.value as {from: {day?:string; rel?:string}})?.from?.day ? "day" : String((row.value as {from: {rel?: string}})?.from?.rel ?? "today")}
            onchange={(e) => {
              const v = (e.target as HTMLSelectElement).value;
              applyDateCtl(row, v === "day" ? dayCtl(ctlOf(row.value, "from")) : { mode: "rel", rel: v }, "from");
            }}>
            <option value="day">{t("vm.rule.fromDay")}</option>
            {#each REL_DAY_LABELS as r (r.label)}<option value={String((r.id as { rel: string }).rel)}>{t(r.label)}</option>{/each}
          </select>
          {#if (row.value as {from: {day?: string}})?.from?.day}
            <input type="date" value={(row.value as { from: { day: string } }).from.day}
              oninput={(e) => applyDateCtl(row, { mode: "day", day: (e.target as HTMLInputElement).value }, "from")} />
          {/if}
          <span>~</span>
          <select class="mode" value={(row.value as {to: {day?:string; rel?:string}})?.to?.day ? "day" : String((row.value as {to: {rel?: string}})?.to?.rel ?? "tomorrow")}
            onchange={(e) => {
              const v = (e.target as HTMLSelectElement).value;
              applyDateCtl(row, v === "day" ? dayCtl(ctlOf(row.value, "to")) : { mode: "rel", rel: v }, "to");
            }}>
            <option value="day">{t("vm.rule.toDay")}</option>
            {#each REL_DAY_LABELS as r (r.label)}<option value={String((r.id as { rel: string }).rel)}>{t(r.label)}</option>{/each}
          </select>
          {#if (row.value as {to: {day?: string}})?.to?.day}
            <input type="date" value={(row.value as { to: { day: string } }).to.day}
              oninput={(e) => applyDateCtl(row, { mode: "day", day: (e.target as HTMLInputElement).value }, "to")} />
          {/if}
        </span>
      {:else if controlOf(row) === "daterange"}
        <select class="val" value={String((row.value as { rel?: string })?.rel ?? "this_week")}
          onchange={(e) => (row.value = { rel: (e.target as HTMLSelectElement).value as never })}>
          {#each REL_RANGE_LABELS as r (r.label)}<option value={String((r.id as { rel: string }).rel)}>{t(r.label)}</option>{/each}
          <option value="last_days:90">{t("vm.rel.lastDays", { n: 90 })}</option>
          <option value="last_days:365">{t("vm.rel.lastDays", { n: 365 })}</option>
        </select>
      {/if}
    {/if}
    <button class="x" title={t("vm.rule.removeCond")} onclick={() => rows.splice(i, 1)}>×</button>
  </div>
{/each}

<div class="add-wrap">
  <button class="ghost add" onclick={() => (menuOpen = !menuOpen)}>{t("vm.rule.add")}</button>
  {#if menuOpen}
    <FieldMenu groups={fieldMenuGroups()} onpick={addField} onclose={() => (menuOpen = false)} />
  {/if}
</div>

<style>
  .rule {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    background: color-mix(in srgb, var(--border) 22%, transparent);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 7px 9px;
  }

  .rule:hover {
    border-color: color-mix(in srgb, var(--text-dim) 55%, var(--border));
  }

  select,
  input {
    font: inherit;
    font-size: 13px;
  }

  .fld {
    min-width: 118px;
  }

  .cmp {
    min-width: 84px;
  }

  .val {
    min-width: 120px;
    flex: 1;
  }

  .num {
    min-width: 90px;
    flex: 0;
  }

  .pair {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .checks {
    display: inline-flex;
    gap: 8px;
    flex-wrap: wrap;
  }

  .checks label {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 13px;
  }

  .taginput {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    flex: 1;
    min-width: 140px;
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    border-radius: 999px;
    padding: 1px 4px 1px 8px;
    font-size: 12px;
  }

  .tag button {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 12px;
    padding: 0 3px;
    cursor: pointer;
  }

  .tag button:hover {
    color: var(--danger);
  }

  .taginput input {
    flex: 1;
    min-width: 90px;
    border: none;
    background: transparent;
    color: var(--text);
    padding: 3px;
  }

  .taginput input:focus {
    outline: none;
  }

  .sugs {
    display: inline-flex;
    gap: 4px;
    flex-wrap: wrap;
    width: 100%;
  }

  .sug {
    border: 1px dashed var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text-dim);
    font-size: 12px;
    padding: 1px 8px;
    cursor: pointer;
  }

  .sug:hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .date-ctl {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
  }

  .add-wrap {
    position: relative;
    align-self: flex-start;
  }

  .add {
    border: 1px dashed var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text-dim);
    font-size: 13px;
    padding: 5px 12px;
    cursor: pointer;
  }

  .add:hover {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }

  .x {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 15px;
    padding: 0 4px;
    cursor: pointer;
  }

  .x:hover {
    color: var(--danger);
  }
</style>

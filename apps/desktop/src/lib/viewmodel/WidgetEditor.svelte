<script lang="ts">
  /**
   * 挂件编辑器（FILTER-SPEC §8 / §11）：编辑来源 / 指标 / 图型 / 目标。
   * 数据集（类型 + AND 条件行）→ 窗口 → 聚合（分组 + 指标）→ 派生（streak 目标）
   * → 渲染器（不兼容组合置灰 + 建议）。统计页「＋ 挂件」与卡片「编辑」共用。
   */
  import type { Condition, FieldDef, FilterValue, ItemType, WidgetConfig } from "../api";
  import { api } from "../api";
  import { t } from "../i18n";
  import {
    METRIC_FNS,
    RENDER_LABELS,
    cloneConfig,
    defaultValueFor,
    type RuleRow,
  } from "../viewmodel";
  import RuleRows from "./RuleRows.svelte";

  let {
    name = "",
    config,
    fields,
    templates = [],
    onclose,
    onsaved,
  }: {
    /** 视图名（新建挂件视图时用） */
    name?: string;
    config: WidgetConfig;
    fields: FieldDef[];
    templates?: { id: string; name: string }[];
    onclose: () => void;
    onsaved: (title: string, cfg: WidgetConfig) => void;
  } = $props();

  let cfg = $state<WidgetConfig>(cloneConfig(config));
  let title = $state<string>(config.title ?? name);
  let error = $state("");
  let busy = $state(false);

  // 条件行（挂件数据集 = 简化数据集：AND 条件列表，无嵌套组）
  let rows = $state<RuleRow[]>((() => {
    const f = cfg.dataset.filter;
    if (!f) return [];
    if ("field" in f) return [{ field: f.field, cmp: f.cmp, value: f.value ?? null }];
    return f.children
      .filter((c): c is Condition => "field" in c)
      .map((c) => ({ field: c.field, cmp: c.cmp, value: (c.value as FilterValue) ?? null }));
  })());

  let itemType = $state<"all" | ItemType>(cfg.dataset.item_type);
  let windowMode = $state<"days" | "all">(
    cfg.window && "all" in cfg.window ? "all" : "days",
  );
  let windowDays = $state<number>(cfg.window && "days" in cfg.window ? cfg.window.days : 365);

  let groupMode = $state<"time" | "field" | "none">(
    cfg.agg?.group ? (cfg.agg.group.by === "time" ? "time" : "field") : "none",
  );
  let timeBucket = $state<"day" | "week" | "month">(
    cfg.agg?.group && cfg.agg.group.by === "time" ? cfg.agg.group.bucket : "day",
  );
  let timeField = $state<string>(
    cfg.agg?.group && cfg.agg.group.by === "time" ? cfg.agg.group.time_field : "col:occurred_at",
  );
  let groupField = $state<string>(
    cfg.agg?.group && cfg.agg.group.by === "field" ? cfg.agg.group.field : "col:template_id",
  );
  let metricFn = $state<NonNullable<NonNullable<WidgetConfig["agg"]>["metric"]>["fn"]>(
    cfg.agg?.metric?.fn ?? "count",
  );
  let presence = $state<boolean>(cfg.options?.presence ?? false);
  let metricField = $state<string>(cfg.agg?.metric?.field ?? "");
  let derivedOn = $state<boolean>(cfg.derived?.kind === "streak");
  let goalKind = $state<"daily" | "weekly">(
    cfg.derived?.goal && "weekly" in cfg.derived.goal ? "weekly" : "daily",
  );
  let goalN = $state<number>(
    cfg.derived?.goal ? (("daily" in cfg.derived.goal ? cfg.derived.goal.daily : ("weekly" in cfg.derived.goal ? cfg.derived.goal.weekly : 1)) || 1) : 1,
  );

  const numberFields = $derived(fields.filter((f) => f.kind === "number"));
  /** 字段分组允许：select / multiselect / col:template_id / col:tags（§8） */
  const groupableFields = $derived(
    fields.filter((f) => f.kind === "select" || f.kind === "multiselect"),
  );
  /** values 仅 log 且禁 group（§8 有意限制） */
  const metricDisabled = (fn: string) => fn === "values" && (itemType !== "log" || groupMode !== "none");

  /** 渲染器兼容约束（§8 约束表）：不兼容置灰，title 给出一句原因 */
  function renderDisabled(r: string): string | null {
    const timeGroup = groupMode === "time";
    if (metricFn === "values") {
      // 点列：仅 log 且无分组；折线 / 指标卡可吃点列
      if (itemType !== "log" || groupMode !== "none") return t("vm.render.dis.values");
      if (r === "bar") return t("vm.render.dis.bar");
      return r === "line" || r === "card" ? null : t("vm.render.dis.other");
    }
    if (r === "pie") return groupMode === "field" ? null : t("vm.render.dis.pie");
    if (r === "heatmap") return timeGroup && timeBucket === "day" ? null : t("vm.render.dis.heatmap");
    if (r === "line") return timeGroup ? null : t("vm.render.dis.line");
    return null; // bar / card：任意桶形
  }

  function buildConfig(): WidgetConfig {
    const next = cloneConfig(cfg);
    next.kind = "widget";
    next.title = title.trim() || undefined;
    next.dataset = { item_type: itemType, filter: { op: "and", children: rows
      .filter((r) => r.field && (r.value != null || ["empty", "not_empty", "is_true", "is_false"].includes(r.cmp)))
      .map((r) => ({ field: r.field, cmp: r.cmp, ...(r.value != null ? { value: r.value } : {}) })) } };
    next.window = windowMode === "all" ? { all: true } : { days: Math.max(1, windowDays) };
    next.agg = {
      group:
        groupMode === "time" ? { by: "time", bucket: timeBucket, time_field: timeField }
        : groupMode === "field" ? { by: "field", field: groupField }
        : null,
      metric: metricFn === "count" ? { fn: "count" } : { fn: metricFn, field: metricField || undefined },
    };
    next.derived = derivedOn
      ? { kind: "streak", goal: goalKind === "daily" ? { daily: Math.max(1, goalN) } : { weekly: Math.max(1, goalN) } }
      : null;
    if (!next.options) next.options = {};
    next.options.presence = presence;
    return next;
  }

  async function save() {
    busy = true;
    error = "";
    try {
      onsaved(title.trim() || t("vm.widget.defaultName"), buildConfig());
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="overlay" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}>
  <div class="editor" role="dialog" aria-label={t("vm.widget.title")}>
    <h3>{t("vm.widget.title")}</h3>

    <label class="line">
      <span class="k">{t("vm.widget.titleLabel")}</span>
      <input bind:value={title} placeholder={t("vm.widget.titlePh")} />
    </label>

    <div class="line">
      <span class="k">{t("vm.widget.dataType")}</span>
      <select bind:value={itemType} onchange={() => (rows = [])}>
        <option value="log">{t("type.log")}</option>
        <option value="task">{t("type.task")}</option>
        <option value="event">{t("type.event")}</option>
        <option value="all">{t("common.all")}</option>
      </select>
      <span class="k">{t("vm.widget.window")}</span>
      <select bind:value={windowMode}>
        <option value="days">{t("vm.widget.lastDays")}</option>
        <option value="all">{t("vm.widget.allHistory")}</option>
      </select>
      {#if windowMode === "days"}
        <input class="days" type="number" min="1" bind:value={windowDays} /> <span class="dim">{t("vm.widget.days")}</span>
      {/if}
    </div>

    <div class="section">
      <div class="k">{t("vm.widget.sourceLabel")}</div>
      <RuleRows bind:rows {fields} {templates} {itemType} />
    </div>

    <div class="line">
      <span class="k">{t("vm.widget.group")}</span>
      <select bind:value={groupMode}>
        <option value="time">{t("vm.widget.byTime")}</option>
        <option value="field">{t("vm.widget.byField")}</option>
        <option value="none">{t("vm.widget.noGroup")}</option>
      </select>
      {#if groupMode === "time"}
        <select bind:value={timeBucket}>
          <option value="day">{t("vm.widget.bucketDay")}</option>
          <option value="week">{t("vm.widget.bucketWeek")}</option>
          <option value="month">{t("vm.widget.bucketMonth")}</option>
        </select>
        <select bind:value={timeField}>
          <option value="col:occurred_at">{t("vm.col.occurredAt")}</option>
          <option value="col:start_at">{t("vm.col.startAt")}</option>
          <option value="col:due_at">{t("vm.col.dueAt")}</option>
        </select>
      {:else if groupMode === "field"}
        <select bind:value={groupField}>
          <option value="col:template_id">{t("vm.col.template")}</option>
          <option value="col:tags">{t("vm.col.tags")}</option>
          {#each groupableFields as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
        </select>
      {/if}
      <span class="k">{t("vm.widget.metric")}</span>
      <select bind:value={metricFn}>
        {#each METRIC_FNS as m (m.id)}
          <option value={m.id} disabled={metricDisabled(m.id)}>{t(m.label)}{metricDisabled(m.id) ? t("vm.metric.valuesNote") : ""}</option>
        {/each}
      </select>
      {#if metricFn !== "count"}
        <select bind:value={metricField}>
          <option value="">{t("vm.widget.pickNumber")}</option>
          {#each numberFields as f (f.id)}<option value={f.id}>{f.name}</option>{/each}
        </select>
      {/if}
    </div>

    <div class="line">
      <span class="k">{t("vm.widget.goal")}</span>
      <label class="check">
        <input type="checkbox" bind:checked={derivedOn} /> {t("vm.widget.streak")}
      </label>
      {#if derivedOn}
        <select bind:value={goalKind}>
          <option value="daily">{t("vm.widget.goalDaily")}</option>
          <option value="weekly">{t("vm.widget.goalWeekly")}</option>
        </select>
        <input class="days" type="number" min="1" bind:value={goalN} /> <span class="dim">{t("vm.widget.times")}</span>
      {/if}
    </div>

    <div class="line">
      <span class="k">{t("vm.widget.renderLabel")}</span>
      {#each RENDER_LABELS as r (r.id)}
        {@const disabled = renderDisabled(r.id)}
        <button
          class="render"
          class:active={cfg.render === r.id}
          disabled={!!disabled}
          title={disabled ?? t(r.label)}
          onclick={() => (cfg.render = r.id)}
        >{t(r.label)}</button>
      {/each}
      {#if cfg.render === "heatmap"}
        <label class="check">
          <input type="checkbox" bind:checked={presence} /> {t("vm.widget.presence")}
        </label>
      {/if}
    </div>

    {#if error}<p class="error">{error}</p>{/if}

    <div class="foot">
      <button class="primary" disabled={busy} onclick={save}>{t("common.save")}</button>
      <span class="spacer"></span>
      <button class="ghost" onclick={onclose}>{t("common.cancel")}</button>
    </div>
    <p class="fine">{t("vm.widget.fine")}</p>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.3);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    z-index: 160;
  }

  .editor {
    margin-top: 60px;
    width: 680px;
    max-width: calc(100vw - 40px);
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 14px;
    padding: 16px 18px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.3);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h3 {
    margin: 0;
    font-size: 15px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .k {
    color: var(--text-dim);
    font-size: 13px;
  }

  .dim {
    color: var(--text-dim);
    font-size: 13px;
  }

  .section {
    border: 1px dashed var(--border);
    border-radius: 10px;
    padding: 8px 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 13px;
  }

  .render {
    border-radius: 999px;
    font-size: 13px;
  }

  .render.active {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: transparent;
  }

  .render:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .days {
    width: 70px;
  }

  .foot {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .spacer {
    flex: 1;
  }

  .fine {
    color: var(--text-dim);
    font-size: 11px;
    margin: 0;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
    margin: 0;
  }
</style>

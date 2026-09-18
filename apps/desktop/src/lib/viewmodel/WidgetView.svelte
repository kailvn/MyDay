<script lang="ts">
  /**
   * 通用挂件组件（v2.1 容器模型）：预置卡与用户卡走同一渲染路径，
   * 只按 render 分派——card / bar / line / pie / heatmap，无任何面板特判。
   * 编辑挂件模式下：整卡可点（打开编辑器）、右上角 × 移除；普通模式纯展示。
   * 渲染器不兼容时整卡置灰 + 提示。
   */
  import type { WidgetResult } from "../api";
  import { t } from "../i18n";

  let {
    widget: w,
    editMode = false,
    onedit,
    onremove,
  }: {
    widget: WidgetResult;
    /** 编辑挂件模式：卡片可点（打开编辑器）、右上角 × 移除 */
    editMode?: boolean;
    onedit: (w: WidgetResult) => void;
    onremove: (w: WidgetResult) => void;
  } = $props();

  // ---- 热力图 ---------------------------------------------------------------
  const heatColor = (level: number): string => {
    if (level === 0) return "color-mix(in srgb, var(--text) 7%, transparent)";
    return `color-mix(in srgb, var(--accent) ${level * 25}%, var(--card))`;
  };
  function heatCellsOf(buckets: { key: string; value: number }[]) {
    const max = Math.max(1, ...buckets.map((b) => b.value));
    const t0 = buckets.length ? new Date(buckets[0].key + "T12:00:00").getTime() : 0;
    return buckets.map((b) => {
      const date = new Date(b.key + "T12:00:00");
      const col = Math.floor((date.getTime() - t0) / (7 * 86_400_000));
      const row = (date.getDay() + 6) % 7;
      const ratio = b.value / max;
      const level = b.value === 0 ? 0 : ratio <= 0.25 ? 1 : ratio <= 0.5 ? 2 : ratio <= 0.75 ? 3 : 4;
      return { ...b, col, row, level };
    });
  }
  const heatWeeks = (cells: { col: number }[]) => Math.max(1, ...cells.map((c) => c.col + 1));

  // ---- 折线（x = 真实时间刻度）---------------------------------------------
  const LINE_W = 560;
  const LINE_H = 160;
  function lineGeom(points: { t: string; v: number }[]) {
    const ts = points.map((p) => Date.parse(p.t));
    const vs = points.map((p) => p.v);
    const t0 = Math.min(...ts);
    const t1 = Math.max(...ts);
    const vmin = Math.min(...vs);
    const vmax = Math.max(...vs);
    const span = vmax - vmin || 1;
    const tspan = t1 - t0 || 1;
    const xy = points.map((p) => {
      const t = Date.parse(p.t);
      return {
        x: 30 + ((t - t0) / tspan) * (LINE_W - 60),
        y: LINE_H - 24 - ((p.v - vmin) / span) * (LINE_H - 44),
        p,
      };
    });
    return { path: xy.map((q, i) => `${i === 0 ? "M" : "L"}${q.x.toFixed(1)},${q.y.toFixed(1)}`).join(" "), xy, min: vmin, max: vmax };
  }

  // ---- 柱状 / 饼图 ----------------------------------------------------------
  function barOf(buckets: { key: string; value: number }[]) {
    const max = Math.max(1, ...buckets.map((b) => b.value));
    return buckets.map((b) => ({ ...b, pct: (b.value / max) * 100 }));
  }
  function pieOf(w: WidgetResult) {
    const shown = (w.buckets ?? []).slice(0, 8);
    const rest = (w.buckets ?? []).slice(8).reduce((a, b) => a + b.value, 0);
    const total = (w.buckets ?? []).reduce((a, b) => a + b.value, 0) || 1;
    let acc = 0;
    const slices = shown.map((b, i) => {
      const from = (acc / total) * 360;
      acc += b.value;
      const to = (acc / total) * 360;
      return { key: w.labels?.[b.key] ?? b.key, from, to, color: `hsl(${(i * 47) % 360} 55% 60%)`, value: b.value };
    });
    if (rest > 0) {
      slices.push({ key: t("vm.widget.other"), from: (acc / total) * 360, to: 360, color: "color-mix(in srgb, var(--text) 15%, transparent)", value: rest });
    }
    return slices;
  }
  const pct = (v: number) => `${Math.round(v * 100)}%`;
</script>

<div class="widget" data-testid="widget" data-widget-key={w.key} class:broken={!!w.error} class:edit={editMode}>
  {#if editMode}
    <button class="rm" aria-label={t("vm.widget.removeAria", { name: w.title })} title={t("vm.widget.remove")} onclick={() => onremove(w)}>×</button>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
  <div
    class="body"
    class:clickable={editMode}
    role={editMode ? "button" : undefined}
    tabindex={editMode ? 0 : undefined}
    onclick={() => editMode && onedit(w)}
    onkeydown={(e) => { if (editMode && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); onedit(w); } }}
  >
  <div class="head">
    <span class="title">
      {#if w.icon}<span class="icon">{w.icon}</span>{/if}
      {w.title}
      {#if w.unit && w.render !== "card"}<span class="unit">({w.unit})</span>{/if}
    </span>
      {#if editMode}<span class="edit-hint">{t("vm.widget.clickEdit")}</span>{/if}
  </div>

  {#if w.error}
    <p class="dim">{w.error}</p>
  {:else if w.render === "card"}
    <div class="card-body">
      {#if w.derived?.streak}
        <span class="cur" class:alive={(w.derived.streak?.current ?? 0) > 0}>{t("vm.widget.streakCur", { n: w.derived.streak?.current ?? 0 })}</span>
        <span class="dim">
          {t("vm.widget.streakStat", { longest: w.derived.streak?.longest ?? 0, recent: w.derived.streak?.recent ?? 0 })}{w.derived.streak?.week_remaining != null ? ` · ${t("vm.widget.weekLeft", { n: w.derived.streak.week_remaining })}` : ""}
        </span>
      {:else}
        <span class="cur">{t("vm.widget.total", { n: w.derived?.total ?? 0 })}{w.unit ? ` ${w.unit}` : ""}</span>
        <span class="dim">{t("vm.widget.minmax", { min: w.derived?.min ?? 0, max: w.derived?.max ?? 0, last: w.derived?.last ?? 0 })}</span>
      {/if}
    </div>
  {:else if w.render === "heatmap"}
    {@const cells = heatCellsOf(w.buckets ?? [])}
    <div class="heat" style={`grid-template-columns:repeat(${heatWeeks(cells)}, 13px)`}>
      {#each cells as c (c.key)}
        <span
          class="cell"
          style={`grid-column:${c.col + 1};grid-row:${c.row + 1};background:${w.presence ? (c.value > 0 ? "var(--accent)" : heatColor(0)) : heatColor(c.level)}`}
          title={`${c.key} · ${c.value}`}
        ></span>
      {/each}
    </div>
    {#if !w.presence}
      <p class="legend">
        {t("vm.widget.less")}
        {#each [0, 1, 2, 3, 4] as lv (lv)}
          <span class="cell" style={`background:${heatColor(lv)}`}></span>
        {/each}
        {t("vm.widget.more")}
      </p>
    {/if}
  {:else if w.render === "line"}
    {@const pts = w.points ?? w.buckets?.map((b) => ({ t: b.key, v: b.value })) ?? []}
    {#if pts.length >= 2}
      {@const geom = lineGeom(pts)}
      <svg viewBox="0 0 {LINE_W} {LINE_H}" class="trend" role="img" aria-label={t("vm.widget.lineAria", { name: w.title })}>
        <path d={geom.path} fill="none" stroke="var(--accent)" stroke-width="2"></path>
        {#each geom.xy as q, i (i)}
          <circle cx={q.x} cy={q.y} r="3.5" fill="var(--accent)">
            <title>{new Date(q.p.t).getMonth() + 1}/{new Date(q.p.t).getDate()} · {q.p.v}</title>
          </circle>
        {/each}
      </svg>
      <p class="dim">{t("vm.widget.latest")} <b>{pts.at(-1)?.v}</b> · {t("vm.widget.lineStat", { min: geom.min, max: geom.max, n: pts.length })}</p>
    {:else}
      <p class="dim">{t("vm.widget.needTwo")}</p>
    {/if}
  {:else if w.render === "bar"}
    <div class="bars">
      {#each barOf(w.buckets ?? []) as b (b.key)}
        <div class="bar-row" title={`${b.key} · ${b.value}`}>
          <span class="bar-key">{w.labels?.[b.key] ?? b.key}</span>
          <span class="bar-track"><span class="bar-fill" style={`width:${b.pct}%`}></span></span>
          <span class="bar-val">{b.value}</span>
        </div>
      {/each}
    </div>
  {:else if w.render === "pie"}
    <div class="pie-wrap">
      <span class="pie" style={`background:conic-gradient(${pieOf(w).map((s) => `${s.color} ${s.from}deg ${s.to}deg`).join(", ")})`}></span>
      <div class="legend-list">
        {#each pieOf(w) as s (s.key)}
          <span class="li"><i style={`background:${s.color}`}></i>{s.key} · {pct(s.value / ((w.buckets ?? []).reduce((a, b) => a + b.value, 0) || 1))}</span>
        {/each}
      </div>
    </div>
  {/if}
  </div>
</div>

<style>
  .widget {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 14px;
    background: var(--card);
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .widget.broken {
    opacity: 0.55;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon {
    margin-right: 2px;
  }

  .unit {
    color: var(--text-dim);
    font-weight: 400;
    font-size: 12px;
  }

  .card-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .cur {
    color: var(--text-dim);
    font-size: 13px;
  }

  .cur.alive {
    color: var(--accent);
    font-weight: 700;
  }

  .dim {
    color: var(--text-dim);
    font-size: 12px;
  }

  .dim b {
    color: var(--text);
    font-size: 14px;
  }

  .heat {
    display: grid;
    grid-auto-flow: row;
    gap: 3px;
    width: fit-content;
    max-width: 100%;
    overflow: hidden;
  }

  .cell {
    width: 10px;
    height: 10px;
    border-radius: 2.5px;
    display: inline-block;
  }

  .legend {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--text-dim);
    font-size: 11px;
    margin: 0;
  }

  .trend {
    width: 100%;
    background: color-mix(in srgb, var(--text) 3%, transparent);
    border-radius: 8px;
  }

  .bars {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .bar-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .bar-key {
    min-width: 72px;
    max-width: 110px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar-track {
    flex: 1;
    height: 10px;
    border-radius: 5px;
    background: color-mix(in srgb, var(--text) 6%, transparent);
    overflow: hidden;
  }

  .bar-fill {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 5px;
  }

  .bar-val {
    min-width: 36px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .pie-wrap {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .pie {
    width: 96px;
    height: 96px;
    border-radius: 50%;
    display: inline-block;
    flex: none;
  }

  .legend-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 13px;
  }

  .legend-list .li {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .legend-list i {
    width: 10px;
    height: 10px;
    border-radius: 3px;
    display: inline-block;
  }

  .widget.edit {
    outline: 2px dashed color-mix(in srgb, var(--accent) 50%, transparent);
    outline-offset: 2px;
    position: relative;
  }

  .body.clickable {
    cursor: pointer;
  }

  .rm {
    position: absolute;
    top: -9px;
    right: -9px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--card);
    color: var(--danger);
    font-size: 13px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 5;
    box-shadow: 0 2px 8px rgb(0 0 0 / 0.18);
  }

  .edit-hint {
    font-size: 11px;
    color: var(--text-dim);
    font-weight: 400;
  }
</style>

/**
 * E2E 内存 mock 的视图模型引擎（FILTER-SPEC 简化 TS 镜像，仅供浏览器走查）。
 * 语义权威是 myday-core 的 Rust 实现与测试；本模块保证前端在 ?e2e=1 模式下
 * 能走查工具条 / 筛选 / 排序 / 挂件流程（含 localStorage 持久化视图行，
 * 使「保存 → 重开仍在」可测）。视图行数据不随 items 重置。
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyRec = Record<string, any>;

const pad = (n: number) => String(n).padStart(2, "0");
const dayKey = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const isoWeekKey = (d: Date) => {
  const target = new Date(d.valueOf());
  target.setDate(target.getDate() + 3 - ((target.getDay() + 6) % 7));
  const week1 = new Date(target.getFullYear(), 0, 4);
  const week = 1 + Math.round(((target.getTime() - week1.getTime()) / 86_400_000 - 3 + ((week1.getDay() + 6) % 7)) / 7);
  return `${target.getFullYear()}-W${pad(week)}`;
};
const monthKey = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
const monday = (d: Date) => {
  const m = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  m.setDate(m.getDate() - ((m.getDay() + 6) % 7));
  return m;
};

export function initViewEngine(
  getItems: () => AnyRec[],
  fieldDefs: AnyRec[],
  getTemplates: () => AnyRec[],
): Record<string, (args: AnyRec) => unknown> {
  const LS_KEY = "myday-e2e-viewdefs";

  type Row = AnyRec;

  const seedRows = (): Row[] => [
    {
      id: "view_builtin_logs_timeline", name: "时间线", panel: "logs", builtin: true, sort: 0,
      config: {
        dataset: {
          item_type: "log",
          filter: { op: "and", children: [] },
          sort: [{ field: "col:occurred_at", dir: "desc" }],
          group: { bucket: "day", field: "col:occurred_at" },
          limit: 200,
        },
        layout: "timeline",
      },
    },
    {
      id: "view_builtin_tasks_today", name: "今天", panel: "tasks", builtin: true, sort: 1,
      config: {
        dataset: {
          item_type: "task",
          filter: {
            op: "and",
            children: [
              { field: "col:status", cmp: "eq", value: "todo" },
              {
                op: "or",
                children: [
                  { field: "col:anchor", cmp: "before", value: { rel: "tomorrow" } },
                  { field: "col:anchor", cmp: "empty" },
                ],
              },
            ],
          },
          sort: [{ field: "col:anchor", dir: "asc" }],
          limit: 500,
        },
        layout: "list",
      },
    },
    {
      id: "view_builtin_tasks_upcoming", name: "即将到期", panel: "tasks", builtin: true, sort: 2,
      config: {
        dataset: {
          item_type: "task",
          filter: {
            op: "and",
            children: [
              { field: "col:status", cmp: "eq", value: "todo" },
              { field: "col:anchor", cmp: "after", value: { rel: "today" } },
            ],
          },
          sort: [{ field: "col:anchor", dir: "asc" }],
          limit: 100,
        },
        layout: "list",
      },
    },
    {
      id: "view_builtin_tasks_all", name: "全部", panel: "tasks", builtin: true, sort: 3,
      config: {
        dataset: {
          item_type: "task",
          filter: {
            op: "and",
            children: [{ field: "col:status", cmp: "eq", value: "todo" }],
          },
          sort: [{ field: "col:anchor", dir: "asc" }],
          limit: 500,
        },
        layout: "list",
      },
    },
    {
      id: "view_builtin_tasks_done", name: "已完成", panel: "tasks", builtin: true, sort: 4,
      config: {
        dataset: {
          item_type: "task",
          filter: {
            op: "and",
            children: [{ field: "col:status", cmp: "eq", value: "done" }],
          },
          sort: [{ field: "col:completed_at", dir: "desc" }],
          limit: 200,
        },
        layout: "list",
      },
    },
    {
      id: "view_builtin_search_all", name: "全部类型", panel: "search", builtin: true, sort: 5,
      config: {
        dataset: {
          item_type: "all",
          filter: { op: "and", children: [] },
          sort: [{ field: "col:updated_at", dir: "desc" }],
          limit: 50,
        },
        layout: "list",
      },
    },
    {
      id: "view_builtin_stats_heatmap", name: "记录热力图", panel: "stats", builtin: false, sort: 6,
      config: {
        kind: "container", layout: "vertical",
        widgets: [{
          kind: "widget", title: "记录热力图",
          dataset: { item_type: "log", filter: { op: "and", children: [] } },
          window: { days: 365 },
          agg: { group: { by: "time", bucket: "day", time_field: "col:occurred_at" }, metric: { fn: "count" } },
          render: "heatmap", options: { presence: false, top_n: 8 },
        }],
      },
    },
    {
      id: "view_builtin_stats_streaks", name: "打卡连续", panel: "stats", builtin: false, sort: 7,
      config: {
        kind: "container", layout: "horizontal",
        widgets: ["tpl_health", "tpl_run", "tpl_weight"].map((tid) => {
          const tpl = getTemplates().find((t) => t.id === tid)!;
          return {
            kind: "widget", title: tpl.name, icon: tpl.icon,
            dataset: { item_type: "log", filter: { op: "and", children: [{ field: "col:template_id", cmp: "eq", value: tpl.id }] } },
            window: { days: 365 },
            agg: { group: { by: "time", bucket: "day", time_field: "col:occurred_at" }, metric: { fn: "count" } },
            derived: { kind: "streak", goal: { daily: 1 } },
            render: "card", options: { presence: false, top_n: 8 },
          };
        }),
      },
    },
    {
      id: "view_builtin_stats_series", name: "数值趋势", panel: "stats", builtin: false, sort: 8,
      config: {
        kind: "container", layout: "vertical",
        widgets: fieldDefs.filter((f) => f.kind === "number").map((f) => ({
          kind: "widget", title: f.name,
          dataset: { item_type: "log", filter: { op: "and", children: [{ field: f.id, cmp: "not_empty" }] } },
          window: { days: 365 },
          agg: { group: null, metric: { fn: "values", field: f.id } },
          render: "line", options: { top_n: 8 },
        })),
      },
    },
  ];

  const loadRows = (): Row[] => {
    try {
      const raw = localStorage.getItem(LS_KEY);
      if (raw) return JSON.parse(raw) as Row[];
    } catch { /* 忽略：回退种子 */ }
    return seedRows();
  };
  const saveRows = (rows: Row[]) => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify(rows));
    } catch { /* 无痕模式等：仅内存 */ }
  };
  let rows = loadRows();

  // ---- 求值 ----------------------------------------------------------------

  const now = () => new Date();
  const today = () => {
    const n = now();
    return new Date(n.getFullYear(), n.getMonth(), n.getDate());
  };
  const dayBounds = (d: Date) => [d.getTime(), d.getTime() + 86_400_000 - 1];
  const relDay = (r: string) => {
    const t = today();
    if (r === "tomorrow") t.setDate(t.getDate() + 1);
    if (r === "yesterday") t.setDate(t.getDate() - 1);
    return t;
  };
  function rangeBounds(rel: string): [number, number] {
    if (rel === "this_week") {
      const mon = monday(today());
      const sun = new Date(mon);
      sun.setDate(sun.getDate() + 6);
      return [mon.getTime(), sun.getTime() + 86_400_000 - 1];
    }
    if (rel === "this_month") {
      const t = today();
      const first = new Date(t.getFullYear(), t.getMonth(), 1);
      const next = new Date(t.getFullYear(), t.getMonth() + 1, 1);
      return [first.getTime(), next.getTime() - 1];
    }
    const n = Number(rel.slice("last_days:".length));
    const start = today();
    start.setDate(start.getDate() - (Math.max(1, n) - 1));
    return [start.getTime(), today().getTime() + 86_400_000 - 1];
  }
  function dateBounds(v: AnyRec): [number, number] {
    if (v.day) return dayBounds(new Date(v.day + "T00:00:00"));
    const rel = String(v.rel ?? "");
    if (["today", "tomorrow", "yesterday"].includes(rel)) return dayBounds(relDay(rel));
    return rangeBounds(rel);
  }

  const colTime = (it: AnyRec, col: string): number | null => {
    if (col === "col:anchor") {
      if (it.type === "task") return it.due_at ? Date.parse(it.due_at) : it.start_at ? Date.parse(it.start_at) : null;
      if (it.type === "log") return it.occurred_at ? Date.parse(it.occurred_at) : null;
      return it.start_at ? Date.parse(it.start_at) : null;
    }
    const key = col.slice("col:".length);
    const v = it[key];
    return v ? Date.parse(v) : it.type === "task" && key === "anchor" ? null : v ? Date.parse(v) : null;
  };

  function evalCond(it: AnyRec, c: AnyRec): boolean {
    const cmp = c.cmp;
    const f = c.field;
    let v: unknown;
    if (f === "col:type") v = it.type;
    else if (f === "col:status") v = it.status;
    else if (f === "col:title") v = it.title;
    else if (f === "col:note") v = it.note;
    else if (f === "col:tags") v = it.tags;
    else if (f === "col:template_id") v = it.template_id;
    else if (f === "col:recurrence") v = it.recurrence;
    else if (f.startsWith("col:")) v = colTime(it, f);
    else v = (it.extra ?? {})[f];

    const emptyOf = (x: unknown) =>
      x == null || x === "" || (Array.isArray(x) && x.length === 0);
    if (cmp === "empty") return emptyOf(v);
    if (cmp === "not_empty") return !emptyOf(v);
    if (emptyOf(v)) return cmp === "none";

    const target = c.value;
    const str = (x: unknown) => String(x ?? "").toLowerCase();
    switch (cmp) {
      case "eq": return str(v) === str(target);
      case "neq": return str(v) !== str(target);
      case "contains": return str(v).includes(str(target));
      case "not_contains": return !str(v).includes(str(target));
      case "gt": return Number(v) > Number(target);
      case "gte": return Number(v) >= Number(target);
      case "lt": return Number(v) < Number(target);
      case "lte": return Number(v) <= Number(target);
      case "between":
        if (typeof v === "number") return Number(v) >= Number(target.from) && Number(v) <= Number(target.to);
        {
          const [lo] = dateBounds(target.from);
          const [hi] = [dateBounds(target.to)[1]];
          const t = colTime(it, f);
          return t != null && t >= lo && t <= hi;
        }
      case "any": return (v as string[]).some((x) => (target as string[]).some((y) => x.toLowerCase() === y.toLowerCase()));
      case "all": return (target as string[]).every((y) => (v as string[]).some((x) => x.toLowerCase() === y.toLowerCase()));
      case "none": return !(v as string[]).some((x) => (target as string[]).some((y) => x.toLowerCase() === y.toLowerCase()));
      case "is_true": return v === true;
      case "is_false": return v === false;
      case "on": {
        const [lo, hi] = dateBounds(target);
        const t = colTime(it, f);
        return t != null && t >= lo && t <= hi;
      }
      case "before": {
        const t = colTime(it, f);
        const d = target.day ? new Date(target.day + "T00:00:00") : relDay(target.rel);
        return t != null && t < dayBounds(d)[0];
      }
      case "after": {
        const t = colTime(it, f);
        const d = target.day ? new Date(target.day + "T00:00:00") : relDay(target.rel);
        return t != null && t > dayBounds(d)[1];
      }
      case "within": {
        const t = colTime(it, f);
        const [lo, hi] = rangeBounds(String(target.rel));
        return t != null && t >= lo && t <= hi;
      }
      default: return false;
    }
  }

  function evalFilter(it: AnyRec, node: AnyRec): boolean {
    if (!node) return true;
    if ("op" in node) {
      if (!node.children?.length) return true;
      return node.op === "and"
        ? node.children.every((c: AnyRec) => evalFilter(it, c))
        : node.children.some((c: AnyRec) => evalFilter(it, c));
    }
    return evalCond(it, node);
  }

  function compileKeyword(kw: string, source: string): AnyRec {
    const mk = (field: string) => ({ field, cmp: "contains", value: kw });
    const children = [mk("col:title"), mk("col:note"), mk("col:tags")];
    for (const f of fieldDefs) {
      if (source !== "all" && f.scope && f.scope !== source) continue;
      children.push(mk(f.id));
    }
    return { op: "or", children };
  }

  const sortVal = (it: AnyRec, field: string): number | string | null => {
    if (field.startsWith("col:")) {
      if (field === "col:tags") return it.tags.join(",");
      const role = ["col:start_at", "col:end_at", "col:due_at", "col:occurred_at", "col:created_at", "col:updated_at", "col:completed_at", "col:anchor"];
      if (role.includes(field)) return colTime(it, field);
      return (it[field.slice("col:".length)] as string) ?? null;
    }
    const v = (it.extra ?? {})[field];
    return typeof v === "number" ? v : (v as string) ?? null;
  };

  function sortItems(list: AnyRec[], spec: AnyRec[]) {
    list.sort((a, b) => {
      for (const s of spec) {
        const va = sortVal(a, s.field);
        const vb = sortVal(b, s.field);
        if (va == null && vb == null) continue;
        if (va == null) return 1; // 空值恒排该级末尾
        if (vb == null) return -1;
        let c = 0;
        if (typeof va === "number" && typeof vb === "number") c = va - vb;
        else c = String(va).toLowerCase() < String(vb).toLowerCase() ? -1 : String(va).toLowerCase() > String(vb).toLowerCase() ? 1 : 0;
        if (c !== 0) return s.dir === "desc" ? -c : c;
      }
      return 0;
    });
  }

  // ---- 命令 ----------------------------------------------------------------

  const uid = () => `view_${Math.random().toString(36).slice(2, 10)}`;

  const effective = (r: Row) => r.config_user ?? r.config;

  function evalPanelView(r: Row, keyword: string | null, limitOverride: number | null): AnyRec {
    const cfg = effective(r);
    const ds = cfg.dataset;
    let list = getItems().filter((it) => ds.item_type === "all" || it.type === ds.item_type);
    let filter = ds.filter;
    const kw = keyword?.trim();
    if (kw) {
      filter = { op: "and", children: [filter, compileKeyword(kw, ds.item_type)] };
    }
    list = list.filter((it) => evalFilter(it, filter));
    const total = list.length;
    sortItems(list, ds.sort ?? []);
    if (limitOverride != null) ds.limit = limitOverride;
    if (typeof ds.limit === "number") list = list.slice(0, ds.limit);

    const matched: Record<string, string[]> | null = kw
      ? Object.fromEntries(
          list.map((it) => {
            const k = kw.toLowerCase();
            const where: string[] = [];
            if ((it.title ?? "").toLowerCase().includes(k)) where.push("title");
            if ((it.note ?? "").toLowerCase().includes(k)) where.push("note");
            if ((it.tags ?? []).some((t: string) => t.toLowerCase().includes(k))) where.push("tag");
            if (Object.entries(it.extra ?? {}).some(([fk, fv]) => fk !== "文件" && String(fv).toLowerCase().includes(k))) where.push("field");
            return [it.id, where];
          }).filter(([, w]) => (w as string[]).length > 0),
        )
      : null;

    let groups: AnyRec[] | null = null;
    if (ds.group) {
      const out: AnyRec[] = [];
      const sorted = [...list].sort((a, b) => {
        const ka = colTime(a, ds.group.field);
        const kb = colTime(b, ds.group.field);
        if (ka == null && kb == null) return 0;
        if (ka == null) return 1;
        if (kb == null) return -1;
        return kb - ka;
      });
      for (const it of sorted) {
        const t = colTime(it, ds.group.field);
        const key = t != null ? dayKey(new Date(t)) : null;
        const last = out[out.length - 1];
        if (last && last.key === key) last.items.push(it);
        else out.push({ key, label: key ?? "未安排", items: [it] });
      }
      for (const g of out) {
        const inner = [...g.items];
        sortItems(inner, ds.sort ?? []);
        g.items = inner;
      }
      groups = out;
    }

    return {
      view_id: r.id, name: r.name, panel: r.panel,
      evaluated_at: now().toISOString(), tz: Intl.DateTimeFormat().resolvedOptions().timeZone ?? "local",
      customized: !!(r.builtin && r.config_user),
      config: JSON.parse(JSON.stringify(effective(r))),
      group: ds.group ?? null, groups,
      items: groups ? list : list,
      matched, total,
    };
  }

  // ---- 挂件 ----------------------------------------------------------------

  function widgetAgg(ds: AnyRec, windowDays: number | null, ctx: { today: Date }, cfg: AnyRec): AnyRec {
    let list = getItems().filter((it) => (ds.item_type === "all" || it.type === ds.item_type) && evalFilter(it, ds.filter));
    // window 过滤（时间列 = agg.time_field 或锚点）
    const winCol = cfg.agg?.group?.by === "time" ? cfg.agg.group.time_field
      : ds.item_type === "task" ? "col:due_at" : ds.item_type === "event" ? "col:start_at" : "col:occurred_at";
    const inWin = (it: AnyRec) => {
      if (windowDays == null) return true;
      const [lo, hi] = rangeBounds(`last_days:${windowDays}`);
      const t = colTime(it, winCol);
      return t != null && t >= lo && t <= hi;
    };
    const winItems = list.filter(inWin);

    const metricFn = cfg.agg?.metric?.fn ?? "count";
    const metricField = cfg.agg?.metric?.field;
    const group = cfg.agg?.group ?? null;

    if (metricFn === "values") {
      const points = winItems
        .map((it) => ({ t: it.occurred_at, v: Number((it.extra ?? {})[metricField]) }))
        .filter((p) => Number.isFinite(p.v) && p.t)
        .sort((a, b) => Date.parse(a.t) - Date.parse(b.t));
      return { points, buckets: null };
    }
    const metricOf = (it: AnyRec) => (metricFn === "count" ? 1 : Number((it.extra ?? {})[metricField]));
    if (group?.by === "time") {
      const sums = new Map<string, { s: number; n: number }>();
      for (const it of winItems) {
        const t = colTime(it, group.time_field);
        if (t == null) continue;
        const d = new Date(t);
        const key = group.bucket === "week" ? isoWeekKey(d) : group.bucket === "month" ? monthKey(d) : dayKey(d);
        const m = Number.isFinite(metricOf(it)) ? metricOf(it) : 0;
        const e = sums.get(key) ?? { s: 0, n: 0 };
        e.s += m;
        e.n += 1;
        sums.set(key, e);
      }
      // 补零到窗口
      const buckets: { key: string; value: number }[] = [];
      const start = windowDays != null ? new Date(rangeBounds(`last_days:${windowDays}`)[0]) : new Date(Math.min(...winItems.map((it) => colTime(it, group.time_field) ?? Date.now()), Date.now()));
      const cur = new Date(start.getFullYear(), start.getMonth(), start.getDate());
      const end = ctx.today;
      while (cur <= end) {
        const key = group.bucket === "week" ? isoWeekKey(cur) : group.bucket === "month" ? monthKey(cur) : dayKey(cur);
        const e = sums.get(key);
        const value = metricFn === "sum" ? e?.s ?? 0 : metricFn === "avg" ? (e && e.n ? e.s / e.n : 0) : e?.n ?? 0;
        buckets.push({ key, value });
        if (group.bucket === "week") cur.setDate(cur.getDate() + 7);
        else if (group.bucket === "month") cur.setMonth(cur.getMonth() + 1);
        else cur.setDate(cur.getDate() + 1);
      }
      return { buckets, points: null };
    }
    if (group?.by === "field") {
      const counts = new Map<string, number>();
      const labels: Record<string, string> = {};
      const add = (k: string) => counts.set(k, (counts.get(k) ?? 0) + 1);
      for (const it of winItems) {
        if (group.field === "col:template_id") {
          if (it.template_id) {
            add(it.template_id);
            const t = getTemplates().find((t) => t.id === it.template_id);
            if (t) labels[it.template_id] = t.name;
          }
          continue;
        }
        if (group.field === "col:tags") {
          for (const t of it.tags ?? []) add(t);
          continue;
        }
        const v = (it.extra ?? {})[group.field];
        if (Array.isArray(v)) v.forEach((x) => add(String(x)));
        else if (v != null && v !== "") add(String(v));
      }
      const buckets = [...counts.entries()].map(([key, value]) => ({ key, value })).sort((a, b) => b.value - a.value);
      return { buckets, points: null, labels };
    }
    const vals = winItems.map(metricOf).filter(Number.isFinite);
    const value = metricFn === "sum" ? vals.reduce((a, b) => a + b, 0) : metricFn === "avg" ? (vals.length ? vals.reduce((a, b) => a + b, 0) / vals.length : 0) : metricFn === "min" ? Math.min(...vals, 0) : metricFn === "max" ? Math.max(...vals, 0) : vals.length;
    return { buckets: [{ key: "all", value }], points: null };
  }

  function evalWidget(viewId: string, key: string, cfg: AnyRec): AnyRec {
    const ds = cfg.dataset;
    const ctx = { today: today() };
    // 窗口完全由挂件自身配置决定（页面无全局范围档）
    const { buckets, points, labels } = widgetAgg(ds, cfg.window?.days ?? 365, ctx, cfg);
    // derived streak（全历史 day 计数）
    let derived: AnyRec = {};
    if (cfg.derived?.kind === "streak") {
      const full = getItems().filter((it) => (ds.item_type === "all" || it.type === ds.item_type) && evalFilter(it, ds.filter));
      const days = new Map<string, number>();
      for (const it of full) {
        const t = it.occurred_at ? Date.parse(it.occurred_at) : null;
        if (t != null) days.set(dayKey(new Date(t)), (days.get(dayKey(new Date(t))) ?? 0) + 1);
      }
      const goal = cfg.derived.goal ?? { daily: 1 };
      if ("weekly" in goal) {
        const need = Number(goal.weekly) || 1;
        const weeks = new Map<string, number>();
        for (const [k, n] of days) weeks.set(isoWeekKey(new Date(k + "T12:00:00")), (weeks.get(isoWeekKey(new Date(k + "T12:00:00"))) ?? 0) + n);
        const met = (k: string) => (weeks.get(k) ?? 0) >= need;
        let cur = monday(ctx.today);
        const thisMon = dayKey(cur);
        let current = 0;
        if (!met(thisMon)) cur.setDate(cur.getDate() - 7);
        while (met(dayKey(cur))) {
          current += 1;
          cur.setDate(cur.getDate() - 7);
        }
        const sorted = [...weeks.entries()].filter(([, n]) => n >= need).map(([k]) => k).sort();
        let longest = 0;
        let run = 0;
        let prev: string | null = null;
        for (const k of sorted) {
          run = prev && new Date(prev + "T12:00:00").getTime() === new Date(k + "T12:00:00").getTime() - 6 * 86_400_000 ? run + 1 : 1;
          longest = Math.max(longest, run);
          prev = k;
        }
        const remaining = Math.max(0, need - (weeks.get(thisMon) ?? 0));
        derived = { streak: { current, longest, recent: days.size ? full.length : 0, week_remaining: remaining > 0 ? remaining : undefined } };
      } else {
        const need = Number(goal.daily) || 1;
        const met = (k: string) => (days.get(k) ?? 0) >= need;
        const cur = new Date(ctx.today);
        let current = 0;
        if (!met(dayKey(cur))) cur.setDate(cur.getDate() - 1);
        while (met(dayKey(cur))) {
          current += 1;
          cur.setDate(cur.getDate() - 1);
        }
        const sorted = [...days.entries()].filter(([, n]) => n >= need).map(([k]) => k).sort();
        let longest = 0;
        let run = 0;
        let prev: string | null = null;
        for (const k of sorted) {
          const d = new Date(k + "T12:00:00");
          run = prev && new Date(prev + "T12:00:00").getTime() === d.getTime() - 86_400_000 ? run + 1 : 1;
          longest = Math.max(longest, run);
          prev = k;
        }
        derived = { streak: { current, longest, recent: full.length } };
      }
    }
    // total / min / max / last
    const vals = points ? points.map((p: AnyRec) => p.v) : (buckets ?? []).map((b: AnyRec) => b.value);
    if (vals.length) {
      derived.total = vals.reduce((a: number, b: number) => a + b, 0);
      derived.min = Math.min(...vals);
      derived.max = Math.max(...vals);
      derived.last = vals[vals.length - 1];
    }
    // 渲染器兼容
    let error: string | null = null;
    const timeKeys = (buckets ?? []).every((b: AnyRec) => /^\d{4}-\d{2}-\d{2}$/.test(b.key) || /^\d{4}-W\d{2}$/.test(b.key) || /^\d{4}-\d{2}$/.test(b.key));
    if (cfg.render === "pie" && (buckets ?? []).length && timeKeys) error = "饼图需要字段分组（时间桶不适用）：把聚合改为按字段分组";
    if (cfg.render === "heatmap" && !(buckets ?? []).every((b: AnyRec) => /^\d{4}-\d{2}-\d{2}$/.test(b.key))) error = "热力图需要按天时间桶（bucket=day）";
    if (cfg.render === "line" && !points && !(buckets ?? []).length) error = "折线需要点列或有序时间桶";
    const unit = cfg.options?.unit ?? fieldDefs.find((f) => f.id === cfg.agg?.metric?.field)?.options?.unit ?? null;
    return {
      view_id: viewId, key, title: cfg.title ?? key, icon: cfg.icon ?? null,
      render: cfg.render, buckets, points, labels: labels ?? {}, derived,
      unit, presence: !!cfg.options?.presence, error,
    };
  }

  /** 容器解析（v2.1）：kind = container（layout + 挂件列表），无动态 / 隐藏 */
  function parseContainer(row: AnyRec): AnyRec {
    const seed = row.config ?? {};
    if (seed.kind !== "container") throw new Error("统计视图配置必须是容器（kind=container）");
    return {
      layout: seed.layout ?? "vertical",
      widgets: JSON.parse(JSON.stringify(seed.widgets ?? [])),
    };
  }

  /** 预设统计容器定义（恢复默认统计页的唯一事实源） */
  const presetStats: AnyRec[] = [
    { id: "view_builtin_stats_heatmap", name: "记录热力图", layout: "vertical" },
    { id: "view_builtin_stats_streaks", name: "打卡连续", layout: "horizontal" },
    { id: "view_builtin_stats_series", name: "数值趋势", layout: "vertical" },
  ];
  function presetWidgets(id: string): AnyRec[] {
    if (id === "view_builtin_stats_heatmap") {
      return [{
        kind: "widget", title: "记录热力图",
        dataset: { item_type: "log", filter: { op: "and", children: [] } },
        window: { days: 365 },
        agg: { group: { by: "time", bucket: "day", time_field: "col:occurred_at" }, metric: { fn: "count" } },
        render: "heatmap", options: { presence: false, top_n: 8 },
      }];
    }
    if (id === "view_builtin_stats_streaks") {
      return getTemplates()
        .filter((t) => t.item_type === "log" && t.pinned)
        .map((tpl) => ({
          kind: "widget", title: tpl.name, icon: tpl.icon,
          dataset: { item_type: "log", filter: { op: "and", children: [{ field: "col:template_id", cmp: "eq", value: tpl.id }] } },
          window: { days: 365 },
          agg: { group: { by: "time", bucket: "day", time_field: "col:occurred_at" }, metric: { fn: "count" } },
          derived: { kind: "streak", goal: { daily: 1 } },
          render: "card", options: { presence: false, top_n: 8 },
        }));
    }
    return fieldDefs
      .filter((f) => f.kind === "number")
      .map((f) => ({
        kind: "widget", title: f.name,
        dataset: { item_type: "log", filter: { op: "and", children: [{ field: f.id, cmp: "not_empty" }] } },
        window: { days: 365 },
        agg: { group: null, metric: { fn: "values", field: f.id } },
        render: "line", options: { top_n: 8 },
      }));
  }

  function statsPage(): AnyRec {
    const containers: AnyRec[] = [];
    const widgets: AnyRec[] = [];
    for (const r of rows.filter((r) => r.panel === "stats")) {
      const st = parseContainer(r);
      containers.push({ view_id: r.id, name: r.name, layout: st.layout });
      (st.widgets as AnyRec[]).forEach((cfg, i) => {
        widgets.push(evalWidget(r.id, `${r.id}#${i}`, cfg));
      });
    }
    return { evaluated_at: now().toISOString(), tz: Intl.DateTimeFormat().resolvedOptions().timeZone ?? "local", containers, widgets };
  }

  return {
    view_list: (a) =>
      clone(rows.filter((r) => !a.panel || r.panel === a.panel)).map((r) => ({
        ...r,
        ...(r.builtin ? { customized: !!r.config_user } : {}),
      })),
    view_get: (a) => {
      const r = rows.find((x) => x.id === a.id);
      if (!r) throw new Error("not found");
      return clone(r);
    },
    view_create: (a) => {
      const row: Row = {
        id: uid(), name: String(a.name), panel: a.panel, builtin: false,
        sort: Math.max(0, ...rows.map((r) => r.sort)) + 1,
        config: JSON.parse(JSON.stringify(a.config)), config_user: null,
        created_at: now().toISOString(), updated_at: now().toISOString(),
      };
      rows.push(row);
      saveRows(rows);
      return clone(row);
    },
    view_save: (a) => {
      const r = rows.find((x) => x.id === a.id);
      if (!r) throw new Error("not found");
      if (r.builtin) r.config_user = JSON.parse(JSON.stringify(a.config));
      else r.config = JSON.parse(JSON.stringify(a.config));
      if (a.name && !r.builtin) r.name = String(a.name);
      r.updated_at = now().toISOString();
      saveRows(rows);
      return clone(r);
    },
    view_delete: (a) => {
      const i = rows.findIndex((x) => x.id === a.id);
      if (i < 0) throw new Error("not found");
      if (rows[i].builtin) throw new Error("内置视图不可删除（可重置回 seed）");
      const [r] = rows.splice(i, 1);
      saveRows(rows);
      return clone(r);
    },
    view_reset: (a) => {
      const r = rows.find((x) => x.id === a.id);
      if (!r) throw new Error("not found");
      if (!r.builtin) throw new Error("用户视图没有 seed，无法重置");
      delete r.config_user;
      saveRows(rows);
      return clone(r);
    },
    view_duplicate: (a) => {
      const r = rows.find((x) => x.id === a.id);
      if (!r) throw new Error("not found");
      const row: Row = {
        id: uid(), name: String(a.name), panel: r.panel, builtin: false,
        sort: Math.max(0, ...rows.map((x) => x.sort)) + 1,
        config: JSON.parse(JSON.stringify(effective(r))), config_user: null,
        created_at: now().toISOString(), updated_at: now().toISOString(),
      };
      rows.push(row);
      saveRows(rows);
      return clone(row);
    },
    query_view: (a) => {
      const r = rows.find((x) => x.id === a.id);
      if (!r) throw new Error(`view ${a.id} not found`);
      if (r.panel === "stats") {
        const st = parseContainer(r);
        const widgets: AnyRec[] = [];
        (st.widgets as AnyRec[]).forEach((cfg, i) => {
          widgets.push(evalWidget(r.id, `${r.id}#${i}`, cfg));
        });
        return {
          view_id: r.id, name: r.name, panel: r.panel,
          evaluated_at: now().toISOString(), tz: "local",
          customized: !!(r.builtin && r.config_user),
          config: clone(effective(r)), widgets,
        };
      }
      return evalPanelView(r, (a.keyword as string) ?? null, (a.limit as number) ?? null);
    },
    query_stats_page: () => statsPage(),
    stats_restore_defaults: () => {
      // 完全重置：清掉全部统计容器（含用户改的），按当前数据铺回三预设
      const kept = rows.filter((r) => r.panel !== "stats");
      const created: string[] = [];
      for (const p of presetStats) {
        kept.push({
          id: p.id, name: p.name, panel: "stats", builtin: false,
          sort: Math.max(0, ...kept.map((r) => r.sort)) + 1,
          config: { kind: "container", layout: p.layout, widgets: presetWidgets(p.id) },
          config_user: null,
          created_at: now().toISOString(), updated_at: now().toISOString(),
        });
        created.push(p.name);
      }
      rows = kept;
      saveRows(rows);
      return created;
    },
  };
}

function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v));
}

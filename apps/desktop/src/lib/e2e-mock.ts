/**
 * 浏览器 E2E mock（开发/验收用，不影响正常构建产物）：
 * 页面带 `?e2e=1` 打开时，在 mount 前安装 window.__TAURI_INTERNALS__，
 * 用内存数据集实现全部前端用到的 invoke 命令与事件通道 ——
 * 不依赖 Tauri 运行时即可在浏览器里实机走查（拖拽、提醒中心、统计、转换等）。
 * 运行：pnpm dev 后访问 http://localhost:1420/?e2e=1
 *
 * 注意：本内存 mock 的字段/模板命令是静态假实现（不落库、无软删/复活语义），
 * 凡涉及这两块的走查用桥模式 ?e2e=1&bridge=<port> 直连真实 core：
 *   MYDAY_DATA_DIR=/tmp/xxx target/debug/e2e_bridge 17877
 */

import { initViewEngine } from "./e2e-viewmock";
import { expandItems } from "./recurrence";

interface MockItem {
  id: string;
  type: "event" | "task" | "log";
  title: string | null;
  note: string | null;
  start_at: string | null;
  end_at: string | null;
  all_day: boolean;
  due_at: string | null;
  due_all_day: boolean;
  occurred_at: string | null;
  status: "todo" | "done" | null;
  completed_at: string | null;
  recurrence: string | null;
  template_id: string | null;
  reminders: { id: number; item_id: string; spec: string; channel: string }[];
  tags: string[];
  attachments: unknown[];
  idempotency_key: string | null;
  created_at: string;
  updated_at: string;
  extra: Record<string, unknown>;
}

const pad = (n: number) => String(n).padStart(2, "0");
const iso = (d: Date) =>
  `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
const utc = (d: Date) => d.toISOString();
const dayShift = (base: Date, days: number, h = 0, m = 0) => {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate() + days, h, m, 0, 0);
  return d;
};
/** 本周一 */
function monday(base = new Date()): Date {
  const d = new Date(base.getFullYear(), base.getMonth(), base.getDate());
  d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  return d;
}

export function installE2eMock() {
  // 桥模式（?e2e=1&bridge=<port>）：invoke 直通 myday_core_e2e_bridge，
  // 跑在真实 core + 真实 SQLite 上（MYDAY_DATA_DIR 指定隔离数据目录）。
  // 内存 mock 的字段/模板命令是假实现，凡涉及这两块的走查必须走桥。
  const bridgePort = new URLSearchParams(location.search).get("bridge");
  if (bridgePort) return installBridgeClient(bridgePort);
  const now = new Date();
  const base = now.getTime();
  let seq = 1;
  const uid = (p: string) => `${p}_mock${(seq++).toString(36)}${Math.random().toString(36).slice(2, 6)}`;

  const mk = (o: Partial<MockItem> & { id: string; type: MockItem["type"] }): MockItem => ({
    title: null,
    note: null,
    start_at: null,
    end_at: null,
    all_day: false,
    due_at: null,
    due_all_day: false,
    occurred_at: null,
    status: null,
    completed_at: null,
    recurrence: null,
    template_id: null,
    reminders: [],
    tags: [],
    attachments: [],
    idempotency_key: null,
    created_at: utc(now),
    updated_at: utc(now),
    extra: {},
    ...o,
  });

  const thisThu = monday();
  thisThu.setDate(thisThu.getDate() + 3); // 本周四

  const items: MockItem[] = [
    mk({
      id: "evt_review",
      type: "event",
      title: "产品评审",
      start_at: utc(dayShift(now, 0, 10, 0)),
      end_at: utc(dayShift(now, 0, 11, 30)),
      tags: ["工作"],
      reminders: [{ id: 1, item_id: "evt_review", spec: "@start-10m", channel: "notify" }],
    }),
    mk({
      id: "evt_weekly",
      type: "event",
      title: "团队周会",
      start_at: utc(new Date(thisThu.getFullYear(), thisThu.getMonth(), thisThu.getDate(), 14, 0)),
      end_at: utc(new Date(thisThu.getFullYear(), thisThu.getMonth(), thisThu.getDate(), 15, 0)),
      recurrence: "@weekly:4",
    }),
    mk({
      id: "evt_daily",
      type: "event",
      title: "晨跑",
      start_at: utc(dayShift(now, 0, 7, 0)),
      end_at: utc(dayShift(now, 0, 7, 30)),
      recurrence: "@daily",
      tags: ["运动"],
    }),
    mk({
      id: "evt_conflict",
      type: "event",
      title: "临时插会",
      start_at: utc(dayShift(now, 0, 10, 30)),
      end_at: utc(dayShift(now, 0, 11, 0)),
    }),
    mk({
      id: "tsk_milk",
      type: "task",
      title: "买牛奶",
      due_at: utc(dayShift(now, 0, 18, 0)),
      status: "todo",
      extra: { fd_priority: "高" },
      reminders: [{ id: 2, item_id: "tsk_milk", spec: "@due-30m", channel: "notify" }],
    }),
    mk({
      id: "tsk_report",
      type: "task",
      title: "交周报",
      due_at: utc(new Date(thisThu.getFullYear(), thisThu.getMonth(), thisThu.getDate(), 18, 0)),
      status: "todo",
      recurrence: "@weekly:4",
    }),
    mk({
      id: "tsk_done",
      type: "task",
      title: "已完成的事",
      due_at: utc(dayShift(now, -1, 12, 0)),
      status: "done",
      completed_at: utc(dayShift(now, -1, 13, 0)),
    }),
    ...Array.from({ length: 6 }, (_, i) =>
      mk({
        id: `log_w${i}`,
        type: "log",
        title: "体重",
        occurred_at: utc(dayShift(now, -i * 2, 8, 0)),
        template_id: "tpl_weight",
        extra: { fd_weight_kg: 70 + i * 0.4 },
      }),
    ),
    mk({
      id: "log_pill",
      type: "log",
      title: "服药",
      occurred_at: utc(dayShift(now, 0, 9, 0)),
      template_id: "tpl_health",
    }),
  ];

  const settings: Record<string, string> = {
    // 走查环境预置：跳过首启引导（引导流程本身一轮已验收）
    onboarded: "1",
  };
  /** 提醒历史（已处理 occurrence）：近几小时的几条 */
  const reminderLog: { remind_at: string; item_id: string }[] = [
    { remind_at: utc(new Date(base - 150 * 60_000)), item_id: "evt_review" },
    { remind_at: utc(new Date(base - 40 * 60_000)), item_id: "tsk_milk" },
  ];

  const fieldDefs = [
    { id: "fd_priority", name: "优先级", kind: "select", options: { choices: ["低", "中", "高"] }, scope: "task", sort: 0, builtin: true },
    { id: "fd_weight_kg", name: "体重(kg)", kind: "number", options: { unit: "kg" }, scope: "log", sort: 1, builtin: true },
    { id: "fd_med_name", name: "药品", kind: "text", options: {}, scope: "log", sort: 2, builtin: true },
  ];
  const templates = [
    { id: "tpl_health", name: "服药", tag: "健康", icon: "💊", item_type: "log", defaults: { title: "服药" }, fields: [], note: null, sort: 0, pinned: true, builtin: true },
    { id: "tpl_run", name: "跑步", tag: "运动", icon: "🏃", item_type: "log", defaults: { title: "跑步" }, fields: [], note: null, sort: 1, pinned: true, builtin: true },
    { id: "tpl_weight", name: "体重", tag: "健康", icon: "⚖️", item_type: "log", defaults: { title: "体重" }, fields: [], note: null, sort: 2, pinned: true, builtin: true },
  ];

  // ---- 事件通道 ------------------------------------------------------------
  const listeners = new Map<number, { event: string; cb: (payload: unknown) => void }>();
  let cbSeq = 1;
  function emitApp(event: string, payload: unknown) {
    // 与真实 Tauri 一致：回调收到完整事件信封 { event, id, payload }
    listeners.forEach((l) => {
      if (l.event === event) l.cb({ event, id: cbSeq, payload });
    });
  }

  const byId = (id: string) => items.find((i) => i.id === id);
  const clone = <T,>(v: T): T => JSON.parse(JSON.stringify(v));

  /** 对齐 core extra_for_type：字段值只保留 scope 匹配目标类型的（全局/文件链接保留） */
  function extraForType(
    extra: Record<string, unknown>,
    type: MockItem["type"],
  ): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(extra ?? {})) {
      if (k === "文件") {
        out[k] = v;
        continue;
      }
      const def = fieldDefs.find((f) => f.id === k);
      if (!def) continue; // 未知键不透传（与 core 一致）
      if (def.scope === null || def.scope === type) out[k] = v;
    }
    return out;
  }

  function listWindow(fromIso: string, toIso: string): MockItem[] {
    const from = Date.parse(fromIso);
    const to = Date.parse(toIso);
    return items.filter((it) => {
      if (it.type === "event" && it.start_at && it.end_at) {
        return Date.parse(it.start_at) <= to && Date.parse(it.end_at) >= from;
      }
      if (it.due_at && Date.parse(it.due_at) >= from && Date.parse(it.due_at) <= to) return true;
      if (it.occurred_at && Date.parse(it.occurred_at) >= from && Date.parse(it.occurred_at) <= to) return true;
      return false;
    });
  }

  const commands: Record<string, (args: Record<string, unknown>) => unknown> = {
    list_items: (a) => {
      const f = (a.filter ?? {}) as Record<string, unknown>;
      let out = [...items];
      if (f.item_type) out = out.filter((i) => i.type === f.item_type);
      if (f.status) out = out.filter((i) => i.status === f.status);
      if (f.changed_on) out = out.slice(0, 20);
      if (f.tag) out = out.filter((i) => i.tags.includes(String(f.tag)));
      if (f.from) out = out.filter((i) => (i.occurred_at ?? i.start_at ?? "") >= String(f.from));
      return clone(out.slice(0, Number(f.limit ?? 200)));
    },
    list_items_window: (a) => clone(listWindow(String(a.from), String(a.to))),
    tasks_view: (a) => {
      // 对齐 core tasks_view：无截止退回 start_at 判断，两者皆无的待办归「今天/全部」
      const view = String(a.view ?? "today");
      const endToday = dayShift(now, 0, 23, 59).getTime();
      const anchor = (i: MockItem) => Date.parse(i.due_at ?? i.start_at ?? "");
      const open = items.filter((i) => i.type === "task" && i.status === "todo");
      if (view === "today")
        return clone(
          open.filter((i) =>
            i.due_at ? Date.parse(i.due_at) <= endToday : !i.start_at || Date.parse(i.start_at) <= endToday,
          ),
        );
      if (view === "upcoming")
        return clone(
          open.filter((i) => i.due_at && Date.parse(i.due_at) > endToday || !i.due_at && !!i.start_at && Date.parse(i.start_at) > endToday),
        );
      if (view === "all") return clone(open);
      return clone(items.filter((i) => i.type === "task" && i.status === "done"));
    },
    get_item: (a) => {
      const it = byId(String(a.id));
      if (!it) throw new Error(`[NOT_FOUND] ${a.id}`);
      return clone(it);
    },
    add_item: (a) => {
      const n = (a.new ?? {}) as Record<string, unknown>;
      const type = (n.item_type as MockItem["type"]) ?? "task";
      const it = mk({
        id: uid(type.slice(0, 3)),
        type,
        title: (n.title as string) ?? null,
        note: (n.note as string) ?? null,
        start_at: (n.start_at as string) ?? null,
        end_at: (n.end_at as string) ?? null,
        all_day: Boolean(n.all_day),
        due_at: (n.due_at as string) ?? null,
        occurred_at: (n.occurred_at as string) ?? (type === "log" ? utc(new Date()) : null),
        status: type === "task" ? "todo" : null,
        recurrence: (n.recurrence as string) ?? null,
        template_id: (n.template_id as string) ?? null,
        tags: (n.tags as string[]) ?? [],
        extra: (n.extra as Record<string, unknown>) ?? {},
      });
      items.push(it);
      return clone(it);
    },
    update_item: (a) => {
      const it = byId(String(a.id));
      if (!it) throw new Error(`[NOT_FOUND] ${a.id}`);
      const p = (a.patch ?? {}) as Record<string, unknown>;
      if (p.title !== undefined && p.title !== null) it.title = String(p.title);
      if (p.note !== undefined && p.note !== null) it.note = String(p.note);
      if (p.start_at !== undefined && p.start_at !== null) it.start_at = String(p.start_at);
      if (p.clear_start_at) it.start_at = it.type === "event" ? it.start_at : null;
      if (p.end_at !== undefined && p.end_at !== null) it.end_at = String(p.end_at);
      if (p.due_at !== undefined && p.due_at !== null) it.due_at = String(p.due_at);
      if (p.clear_due_at) it.due_at = null;
      if (p.occurred_at !== undefined && p.occurred_at !== null) it.occurred_at = String(p.occurred_at);
      if (p.status !== undefined && p.status !== null) it.status = p.status as "todo" | "done";
      if (p.recurrence !== undefined && p.recurrence !== null) it.recurrence = String(p.recurrence);
      if (p.clear_recurrence) it.recurrence = null;
      if (Array.isArray(p.tags)) it.tags = p.tags as string[];
      it.updated_at = utc(new Date());
      return clone(it);
    },
    delete_item: (a) => {
      const idx = items.findIndex((i) => i.id === String(a.id));
      if (idx === -1) throw new Error(`[NOT_FOUND] ${a.id}`);
      const [it] = items.splice(idx, 1);
      return clone(it);
    },
    complete_task: (a) => {
      const it = byId(String(a.id));
      if (!it) throw new Error(`[NOT_FOUND] ${a.id}`);
      if (it.recurrence === "@daily" && it.due_at) {
        it.due_at = utc(new Date(Date.parse(it.due_at) + 86_400_000));
      } else if (it.recurrence === "@weekly:4" && it.due_at) {
        it.due_at = utc(new Date(Date.parse(it.due_at) + 7 * 86_400_000));
      } else {
        it.status = "done";
        it.completed_at = utc(new Date());
      }
      return clone(it);
    },
    snooze: (a) => {
      const it = byId(String(a.id));
      if (!it) throw new Error(`[NOT_FOUND] ${a.id}`);
      reminderLog.push({ remind_at: String(a.until), item_id: it.id });
      return clone(it);
    },
    search_items: (a) => {
      const q = String(a.query ?? "");
      return items
        .filter((i) => (i.title ?? "").includes(q) || (i.note ?? "").includes(q))
        .map((item) => ({ item: clone(item), matched_in: ["title"] }));
    },
    list_templates: () => clone(templates),
    set_template_pinned: (a) => {
      const t = templates.find((x) => x.id === String(a.id));
      if (t) t.pinned = Boolean(a.pinned);
      return null;
    },
    move_template: () => null,
    add_template: () => clone(templates[0]),
    update_template: () => clone(templates[0]),
    delete_template: () => clone(templates[0]),
    list_field_defs: () => clone(fieldDefs),
    // 内存 mock 无软删/清理语义：返回空集即可（真实语义走桥模式）
    list_deleted_field_defs: () => [],
    purge_deleted_field_defs: () => 0,
    get_setting: (a) => settings[String(a.key)] ?? null,
    set_setting: (a) => {
      settings[String(a.key)] = String(a.value);
      return null;
    },
    check_conflict: () => [],
    stats_summary: (a) => {
      const days = Math.min(1095, Math.max(7, Number(a.days ?? 365)));
      const todayKey = iso(now).slice(0, 10);
      const cutoffKey = iso(dayShift(now, -(days - 1))).slice(0, 10);
      const heatmap = Array.from({ length: days }, (_, i) => {
        const d = dayShift(now, -(days - 1 - i));
        const key = iso(d).slice(0, 10);
        return {
          day: key,
          count: items.filter((it) => it.type === "log" && (it.occurred_at ?? "").slice(0, 10) === key).length,
        };
      });
      // 连续天数与 core/stats_summary 同款语义：记录日集合 → 最长连续 + 存活当前连续
      const daySetOf = (tid: string) => {
        const set = new Set<string>();
        for (const it of items) {
          if (it.type === "log" && it.template_id === tid && it.occurred_at) {
            set.add(it.occurred_at.slice(0, 10));
          }
        }
        return set;
      };
      const dayBefore = (key: string) => {
        const [y, m, d] = key.split("-").map(Number);
        return iso(new Date(y, m - 1, d - 1)).slice(0, 10);
      };
      const streakOf = (tid: string) => {
        const set = daySetOf(tid);
        // 当前连续：今天已打含今天；今天没打但昨天打了仍算存活；否则 0
        let current = 0;
        let cursor = set.has(todayKey) ? todayKey : set.has(dayBefore(todayKey)) ? dayBefore(todayKey) : null;
        while (cursor && set.has(cursor)) {
          current += 1;
          cursor = dayBefore(cursor);
        }
        // 最长连续
        const sorted = [...set].sort();
        let longest = 0;
        let run = 0;
        let prev: string | null = null;
        for (const k of sorted) {
          run = prev === dayBefore(k) ? run + 1 : 1;
          longest = Math.max(longest, run);
          prev = k;
        }
        return { current, longest };
      };
      const wpts = items
        .filter((i) => i.type === "log" && typeof i.extra.fd_weight_kg === "number")
        .sort((x, y) => Date.parse(x.occurred_at!) - Date.parse(y.occurred_at!))
        .map((i) => [i.occurred_at!, i.extra.fd_weight_kg as number] as [string, number]);
      return {
        heatmap,
        streaks: templates
          .filter((t) => t.pinned)
          .map((t) => {
            const { current, longest } = streakOf(t.id);
            return {
              template_id: t.id,
              name: t.name,
              icon: t.icon,
              current,
              longest,
              recent: items.filter(
                (i) => i.template_id === t.id && (i.occurred_at ?? "").slice(0, 10) >= cutoffKey,
              ).length,
            };
          }),
        series: wpts.length >= 2 ? [{ field_id: "fd_weight_kg", name: "体重(kg)", unit: "kg", points: wpts }] : [],
      };
    },
    reminder_history: (a) =>
      reminderLog
        .slice()
        .sort((x, y) => Date.parse(y.remind_at) - Date.parse(x.remind_at))
        .slice(0, Number(a.limit ?? 50))
        .map((r) => ({ remind_at: r.remind_at, item: clone(byId(r.item_id)) }))
        .filter((r) => r.item),
    reminder_unread: () => {
      const seen = settings.reminder_seen_at;
      if (!seen) return 0;
      return reminderLog.filter((r) => r.remind_at > seen).length;
    },
    mark_reminders_seen: () => {
      settings.reminder_seen_at = utc(new Date());
      return settings.reminder_seen_at;
    },
    convert_task_to_event: (a) => {
      const it = byId(String(a.id));
      if (!it || it.type !== "task") throw new Error("[INVALID] 不是待办");
      it.type = "event";
      it.start_at = it.due_at;
      it.end_at = it.due_at ? utc(new Date(Date.parse(it.due_at) + 3_600_000)) : null;
      it.due_at = null;
      it.status = null;
      it.completed_at = null;
      // 对齐 core extra_for_type：scope 不匹配的字段不跟去新类型（优先级是待办专属）
      it.extra = extraForType(it.extra, "event");
      // 相对提醒随锚点改写（core：@due → @start）
      for (const r of it.reminders) r.spec = r.spec.replace("@due", "@start");
      return clone(it);
    },
    event_to_log: (a) => {
      const it = byId(String(a.id));
      if (!it || it.type !== "event") throw new Error("[INVALID] 不是日程");
      const log = mk({
        id: uid("log"),
        type: "log",
        title: it.title,
        note: it.note,
        occurred_at: it.start_at,
        tags: [...it.tags],
        extra: extraForType({ ...it.extra }, "log"),
      });
      items.push(log);
      return clone(log);
    },
    export_ics: () => "/tmp/myday-e2e/myday.ics",
    backup_zip: () => "/tmp/myday-e2e/backups/myday-backup-mock.zip",
    // 节假日 JSON：用户文件存 mock settings（window.__HOLIDAYS_USER__ 供测试检查）
    load_holidays_json: () =>
      ((window as unknown as Record<string, unknown>).__HOLIDAYS_USER__ as string | undefined) ?? null,
    save_holidays_json: (a) => {
      (window as unknown as Record<string, unknown>).__HOLIDAYS_USER__ = String(a.text);
      return null;
    },
    reset_holidays_json: () => {
      delete (window as unknown as Record<string, unknown>).__HOLIDAYS_USER__;
      return null;
    },
    app_info: () => ({
      version: "0.1.0-e2e",
      data_root: "/tmp/myday-e2e",
      socket_path: "/tmp/myday-e2e.sock",
      backend: "x11",
    }),
    attachment_abs_path: (a) => `/tmp/myday-e2e/${String(a.relPath ?? a.rel_path ?? "")}`,
    add_attachment_b64: () => null,
    open_quick_add: (a) => {
      window.dispatchEvent(
        new CustomEvent("myday-dev-quick-add", {
          detail: { itemType: a.itemType ?? null, title: a.title ?? null },
        }),
      );
      return null;
    },
    // ---- 今日悬浮窗（OVERLAY-SPEC）----
    // 语义对齐 myday-core::overlay::today_overlay：归属按开始、逾期卡 00:00、
    // 无 due 排除、重复日程展开；重复待办 due 即当前期不展开
    overlay_today: () => {
      const day0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
      const day1 = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
      const t0 = day0.getTime();
      const t1 = day1.getTime();
      const events = expandItems(
        items.filter((i) => i.type === "event") as unknown as Parameters<typeof expandItems>[0],
        day0,
        day1,
      )
        .filter((i) => {
          const s = i.start_at ? Date.parse(i.start_at) : Number.NaN;
          return s >= t0 && s < t1;
        })
        .map((i) => {
          const s = i.start_at ? Date.parse(i.start_at) : null;
          const e = i.end_at ? Date.parse(i.end_at) : null;
          return {
            id: i.id,
            kind: "event" as const,
            title: i.title ?? i.note?.slice(0, 40) ?? "无标题",
            start_at: i.start_at,
            end_at: i.end_at,
            due_at: null,
            all_day: i.all_day,
            due_all_day: false,
            in_progress: s !== null && e !== null && now.getTime() >= s && now.getTime() < e,
            created_at: i.created_at,
          };
        })
        .sort((a, b) => Date.parse(a.start_at!) - Date.parse(b.start_at!) || a.created_at.localeCompare(b.created_at));
      const openTasks = items.filter((i) => i.type === "task" && i.status === "todo");
      const tasks = openTasks
        .filter((i) => {
          const d = i.due_at ? Date.parse(i.due_at) : Number.NaN;
          return d >= t0 && d < t1;
        })
        .map((i) => ({
          id: i.id,
          kind: "task" as const,
          title: i.title ?? i.note?.slice(0, 40) ?? "无标题",
          start_at: i.start_at,
          end_at: null,
          due_at: i.due_at,
          all_day: i.all_day,
          due_all_day: i.due_all_day,
          in_progress: false,
          created_at: i.created_at,
        }))
        .sort((a, b) => Date.parse(a.due_at!) - Date.parse(b.due_at!) || a.created_at.localeCompare(b.created_at));
      return {
        date: iso(now).slice(0, 10),
        events,
        tasks,
        done_count: items.filter(
          (i) => i.type === "task" && i.status === "done" && i.due_at && Date.parse(i.due_at) >= t0 && Date.parse(i.due_at) < t1,
        ).length,
        overdue_count: openTasks.filter((i) => i.due_at && Date.parse(i.due_at) < t0).length,
        unscheduled_count: openTasks.filter((i) => !i.due_at).length,
      };
    },
    get_overlay_config: () => ({
      enabled: settings["overlay.enabled"] === "true",
      corner: settings["overlay.corner"] ?? "tr",
      custom_pos: settings["overlay.custom_pos"] ? JSON.parse(settings["overlay.custom_pos"]) : null,
      size: settings["overlay.size"] ? JSON.parse(settings["overlay.size"]) : null,
      opacity: Number(settings["overlay.opacity"] ?? 0.9),
      locked: settings["overlay.locked"] === "true",
    }),
    set_overlay_config: (a) => {
      const c = (a.config ?? {}) as Record<string, unknown>;
      settings["overlay.enabled"] = String(Boolean(c.enabled));
      settings["overlay.corner"] = String(c.corner ?? "tr");
      settings["overlay.opacity"] = String(Number(c.opacity ?? 0.9));
      settings["overlay.locked"] = String(Boolean(c.locked));
      if (c.custom_pos) settings["overlay.custom_pos"] = JSON.stringify(c.custom_pos);
      else delete settings["overlay.custom_pos"]; // 清除 = 重新吸附
      if (c.size) settings["overlay.size"] = JSON.stringify(c.size);
      else delete settings["overlay.size"]; // null = 默认 264×380
      emitApp("overlay-config", c);
      return null;
    },
    overlay_apply_window_state: () => null,
    overlay_set_visible: (a) => {
      settings["overlay.enabled"] = String(Boolean(a.visible));
      emitApp("overlay-config", { enabled: Boolean(a.visible) });
      return null;
    },
    overlay_save_drag_pos: () => null, // 浏览器无真实窗口坐标，状态断言走 settings
    overlay_save_resize_size: () => null,
    overlay_show_main: () => null,
  };

  // 视图模型命令（FILTER-SPEC §12）：TS 简化镜像 + localStorage 持久化视图行
  Object.assign(
    commands,
    initViewEngine(
      () => items,
      fieldDefs as unknown as Record<string, unknown>[],
      () => templates as unknown as Record<string, unknown>[],
    ),
  );

  const MUTATING = new Set([
    "add_item",
    "update_item",
    "delete_item",
    "complete_task",
    "snooze",
    "convert_task_to_event",
    "event_to_log",
  ]);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Record<string, unknown> | undefined) => {
      if (cmd === "plugin:event|listen") {
        const id = cbSeq++;
        const handler = (window as unknown as Record<string, unknown>)[`_${args?.handler}`] as (
          payload: unknown,
        ) => void;
        listeners.set(id, { event: String(args?.event), cb: handler ?? (() => {}) });
        return id;
      }
      if (cmd === "plugin:event|unlisten") {
        listeners.delete(Number(args?.eventId));
        return null;
      }
      const fn = commands[cmd];
      if (!fn) throw new Error(`[e2e-mock] 未实现命令: ${cmd}`);
      const out = fn(args ?? {});
      if (MUTATING.has(cmd)) emitApp("data-changed", {});
      return out;
    },
    transformCallback: (
      cb: (payload: unknown) => void,
      once?: boolean,
    ) => {
      const id = cbSeq++;
      const w = window as unknown as Record<string, unknown>;
      w[`_${id}`] = once
        ? (p: unknown) => {
            cb(p);
            delete w[`_${id}`];
          }
        : cb;
      return id;
    },
    // 窗口 label 可用 ?window= 覆盖（overlay 路由的浏览器 E2E 钩子）
    metadata: {
      currentWindow: {
        label: new URLSearchParams(location.search).get("window") ?? "main",
      },
      currentWebview: {
        label: new URLSearchParams(location.search).get("window") ?? "main",
      },
    },
    convertFileSrc: (p: string) => `asset://mock/${p}`,
    plugins: {},
  };
  console.info("[e2e-mock] 已安装内存后端（?e2e=1）");
}

/**
 * 桥客户端：__TAURI_INTERNALS__ 形状与内存 mock 一致，invoke 转发到
 * myday_core_e2e_bridge（HTTP POST，信封 {ok,data}/{ok,error}）。
 * 变更命令的应答带 __changed 标记（bridge 注入）→ 剥掉并广播 data-changed。
 */
function installBridgeClient(port: string) {
  const url = `http://127.0.0.1:${port}/`;
  const listeners = new Map<number, { event: string; cb: (payload: unknown) => void }>();
  let cbSeq = 1;
  const emitApp = (event: string, payload: unknown) => {
    // 与真实 Tauri 一致：回调收到完整事件信封 { event, id, payload }
    listeners.forEach((l) => {
      if (l.event === event) l.cb({ event, id: cbSeq, payload });
    });
  };

  (window as any).__TAURI_INTERNALS__ = {
    invoke: async (cmd: string, args: Record<string, unknown> | undefined) => {
      if (cmd === "plugin:event|listen") {
        const id = cbSeq++;
        const handler = (window as unknown as Record<string, unknown>)[`_${args?.handler}`] as (
          payload: unknown,
        ) => void;
        listeners.set(id, { event: String(args?.event), cb: handler ?? (() => {}) });
        return id;
      }
      if (cmd === "plugin:event|unlisten") {
        listeners.delete(Number(args?.eventId));
        return null;
      }
      if (cmd === "open_quick_add") {
        // 桥开不了第二窗口：与内存 mock 一致，落为页面内快速添加面板
        window.dispatchEvent(
          new CustomEvent("myday-dev-quick-add", {
            detail: { itemType: args?.itemType ?? null, title: args?.title ?? null },
          }),
        );
        return null;
      }
      let body: { ok: boolean; data?: unknown; error?: { code: string; message: string } };
      try {
        const res = await fetch(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ cmd, args: args ?? {} }),
        });
        body = await res.json();
      } catch (e) {
        throw new Error(
          `[e2e-bridge] 桥不可达（127.0.0.1:${port}）：先启动 myday_core_e2e_bridge ${port}`,
        );
      }
      if (!body.ok) throw new Error(body.error?.message ?? "未知错误");
      let data = body.data;
      if (data && typeof data === "object" && "__changed" in (data as object)) {
        const { __changed, ...rest } = data as Record<string, unknown>;
        data = rest;
        emitApp("data-changed", {});
      }
      return data;
    },
    transformCallback: (cb: (payload: unknown) => void, once?: boolean) => {
      const id = cbSeq++;
      const w = window as unknown as Record<string, unknown>;
      w[`_${id}`] = once
        ? (p: unknown) => {
            cb(p);
            delete w[`_${id}`];
          }
        : cb;
      return id;
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
    convertFileSrc: (p: string) => `asset://mock/${p}`,
    plugins: {},
  };
  console.info(`[e2e-mock] 已接桥（?e2e=1&bridge=${port}）→ 真实 core`);
}

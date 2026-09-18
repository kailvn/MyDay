/**
 * 拖拽改期的共享规则（SPRINT2-SPEC §2 / §3）。
 * 周/日网格与月视图共用：几何吸附、日程平移 / 调时长、待办改截止、
 * 重复规则智能改写（跨星期 → @weekly:n，跨日 → @monthly:d）。
 * 松手才写库（update_item），返回 undo 补丁供 toast 撤销。
 */
import type { Item, ItemPatch } from "./api";
import { t } from "./i18n";
import { parseRecurrence, weekdayLabel } from "./recurrence";

/** 15 分钟吸附 */
export const SNAP_MIN = 15;

export function snapMinutes(m: number, step = SNAP_MIN): number {
  return Math.round(m / step) * step;
}

export interface RescheduleResult {
  /** 应用改期的补丁 */
  patch: ItemPatch;
  /** 恢复原值的补丁（toast 撤销用） */
  undo: ItemPatch;
  /** 人类可读说明（toast 文案） */
  note: string;
}

const pad = (n: number) => String(n).padStart(2, "0");
/** 日期（随界面语言：中文「9月18日」/ 英文「9/18」） */
const fmtDay = (d: Date) => t("reschedule.day", { m: d.getMonth() + 1, d: d.getDate() });
const fmtClock = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
/** 周一 1 … 周日 7（与 @weekly:n 一致） */
const weekdayNo = (d: Date) => ((d.getDay() + 6) % 7) + 1;

function atClock(day: Date, clock: Date): Date {
  return new Date(day.getFullYear(), day.getMonth(), day.getDate(), clock.getHours(), clock.getMinutes(), clock.getSeconds());
}

/** 恢复日程原值的补丁 */
function eventUndo(item: Item): ItemPatch {
  return {
    start_at: item.start_at ?? undefined,
    end_at: item.end_at ?? undefined,
    recurrence: item.recurrence ?? undefined,
    clear_recurrence: item.recurrence ? false : true,
  } as ItemPatch;
}

/**
 * 移动日程的一次发生到新起点（newStart 已吸附）。`item` 为系列原条目，
 * `occStart/occEnd` 为被拖 occurrence 的起止（无规则时即条目本身）。
 * - 无规则 / @daily：start/end 整体平移；
 * - @weekly:n 拖到别的星期几：改写为 @weekly:<目标>，锚点 = 目标日同时刻；
 * - @monthly:d 拖到别的日：改写为 @monthly:<目标日>。
 */
export function moveEventOccurrence(
  item: Item,
  occStart: Date,
  occEnd: Date,
  newStart: Date,
): RescheduleResult {
  const durMs = occEnd.getTime() - occStart.getTime();
  const deltaDays = Math.round(
    (dayOf(newStart).getTime() - dayOf(occStart).getTime()) / 86_400_000,
  );
  const rec = parseRecurrence(item.recurrence);
  const plainShift = () => {
    const delta = newStart.getTime() - occStart.getTime();
    return {
      start_at: new Date((item.start_at ? Date.parse(item.start_at) : occStart.getTime()) + delta),
      end_at: new Date((item.end_at ? Date.parse(item.end_at) : occEnd.getTime()) + delta),
      recurrence: item.recurrence ?? undefined,
    };
  };
  let next: { start_at: Date; end_at: Date; recurrence?: string };
  let ruleNote = "";
  if (!rec || rec.kind === "daily") {
    next = plainShift();
  } else if (rec.kind === "weekly") {
    if (deltaDays % 7 === 0) {
      next = plainShift();
    } else {
      const target = weekdayNo(newStart);
      next = {
        start_at: newStart,
        end_at: new Date(newStart.getTime() + durMs),
        recurrence: `@weekly:${target}`,
      };
      ruleNote = t("reschedule.rule_weekly", { day: weekdayLabel(target) });
    }
  } else {
    if (newStart.getDate() === occStart.getDate()) {
      next = plainShift();
    } else {
      next = {
        start_at: newStart,
        end_at: new Date(newStart.getTime() + durMs),
        recurrence: `@monthly:${newStart.getDate()}`,
      };
      ruleNote = t("reschedule.rule_monthly", { d: newStart.getDate() });
    }
  }
  const series = item.recurrence ? t("reschedule.series_changed", { rule: ruleNote }) : "";
  return {
    patch: {
      start_at: next.start_at.toISOString(),
      end_at: next.end_at.toISOString(),
      recurrence: next.recurrence ?? null,
      clear_recurrence: !next.recurrence,
    },
    undo: eventUndo(item),
    note: t("reschedule.moved", {
      day: fmtDay(next.start_at),
      start: fmtClock(next.start_at),
      end: fmtClock(next.end_at),
      series,
    }),
  };
}

/** 调整日程一端（15 分钟吸附；最短 15 分钟），另一端不动 */
export function resizeEventOccurrence(
  item: Item,
  occStart: Date,
  occEnd: Date,
  edge: "start" | "end",
  newTime: Date,
): RescheduleResult {
  let s = occStart.getTime();
  let e = occEnd.getTime();
  if (edge === "start") s = Math.min(newTime.getTime(), e - SNAP_MIN * 60_000);
  else e = Math.max(newTime.getTime(), s + SNAP_MIN * 60_000);
  return {
    patch: {
      start_at: new Date(s).toISOString(),
      end_at: new Date(e).toISOString(),
    },
    undo: eventUndo(item),
    note: t("reschedule.resized", { start: fmtClock(new Date(s)), end: fmtClock(new Date(e)) }),
  };
}

/**
 * 待办截止移到目标日（保留原钟点；due_all_day 保持）。
 * 重复待办跨星期 / 跨日同样改写规则；start_at 随 due 平移保持差值。
 */
export function moveTaskDue(item: Item, targetDay: Date): RescheduleResult {
  const due = item.due_at ? new Date(item.due_at) : null;
  if (!due) {
    return { patch: {}, undo: {}, note: "" }; // 无截止不参与拖拽（调用方已过滤）
  }
  const newDue = atClock(targetDay, due);
  const delta = newDue.getTime() - due.getTime();
  const deltaDays = Math.round(
    (dayOf(newDue).getTime() - dayOf(due).getTime()) / 86_400_000,
  );
  const rec = parseRecurrence(item.recurrence);
  let recurrence: string | null = item.recurrence ?? null;
  let ruleNote = "";
  if (rec?.kind === "weekly" && deltaDays % 7 !== 0) {
    recurrence = `@weekly:${weekdayNo(newDue)}`;
    ruleNote = t("reschedule.rule_weekly", { day: weekdayLabel(weekdayNo(newDue)) });
  } else if (rec?.kind === "monthly" && newDue.getDate() !== due.getDate()) {
    recurrence = `@monthly:${newDue.getDate()}`;
    ruleNote = t("reschedule.rule_monthly", { d: newDue.getDate() });
  }
  const patch: ItemPatch = { due_at: newDue.toISOString() };
  if (recurrence) {
    patch.recurrence = recurrence;
    patch.clear_recurrence = false;
  }
  if (item.start_at) patch.start_at = new Date(Date.parse(item.start_at) + delta).toISOString();
  const undo: ItemPatch = {
    due_at: item.due_at ?? undefined,
    start_at: item.start_at ?? undefined,
    recurrence: item.recurrence ?? undefined,
    clear_recurrence: !item.recurrence,
  } as ItemPatch;
  const clock = item.due_all_day ? "" : ` ${fmtClock(newDue)}`;
  return {
    patch,
    undo,
    note: t("reschedule.due_moved", {
      day: fmtDay(newDue),
      clock,
      series: rec ? t("reschedule.series_changed", { rule: ruleNote }) : "",
    }),
  };
}

function dayOf(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

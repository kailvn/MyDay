/**
 * 受控 NL 时间提示（SPRINT-SPEC §8）：从标题文本解析出「待确认」的时间候选。
 * 只出 chip、Tab/点击才应用，绝不静默改写；规则全部锁死进 golden
 * （timewords.golden.ts），不做聪明推断：
 * - 裸「N点」= 24 小时制 N:00；「下午/晚上 N点」= N<12 ? N+12 : N；「N点半」= N:30
 * - 日期词：今天/明天/后天/大后天/下周X/周X（周X = 最近将来的该日，含今天）
 * - 数字日期：M月D日[号] / M/D（可带时刻「9/21 15:00」；已过去的日期滚到明年最近一次）
 * - 相对：N分钟后 / N小时后 / 半小时后 / 一小时后
 * - 组合：日期词 + 时刻词可同时出现（「明天9点」）；只有时刻 → 今天该时刻
 */

export interface TimeHint {
  /** 标题里被匹配的原文 */
  matched: string;
  /** chip 展示文案 */
  label: string;
  /** 具体时刻（dateOnly 时为该日 09:00 占位，应用方按字段决定默认钟点） */
  date: Date;
  /** 只有日期词（应用方写截止而非开始） */
  dateOnly: boolean;
}

/** getDay 语义：0=周日 … 6=周六 */
const WD: Record<string, number> = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 日: 0, 天: 0 };

function dayWithOffset(now: Date, offset: number): Date {
  const d = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  d.setDate(d.getDate() + offset);
  return d;
}

/** 周X：最近将来的该日（含今天）；下周X：严格下一周的该日 */
function nextWeekday(now: Date, target: number, nextWeek: boolean): Date {
  const diff = (target - now.getDay() + 7) % 7;
  return dayWithOffset(now, nextWeek ? diff + 7 : diff);
}

function periodTo(h: number, period: string | undefined): number {
  if ((period === "下午" || period === "晚上") && h < 12) return h + 12;
  return h;
}

const pad = (x: number) => String(x).padStart(2, "0");

/** 扫描标题文本，返回至多一个候选（无候选返回 null） */
export function parseTimeHint(text: string, now: Date = new Date()): TimeHint | null {
  if (!text || text.length > 60) return null;

  // 相对：N分钟后 / N小时后 / 半小时后 / 一小时后
  const rel = text.match(/(半小时|一小时|\d{1,3}\s*分钟|\d{1,2}\s*小时)后/);
  if (rel) {
    const word = rel[1].replace(/\s/g, "");
    const mins =
      word === "半小时" ? 30 : word === "一小时" ? 60 : word.includes("分") ? parseInt(word, 10) : parseInt(word, 10) * 60;
    if (!Number.isFinite(mins) || mins <= 0) return null;
    const d = new Date(now.getTime() + mins * 60_000);
    d.setSeconds(0, 0);
    return { matched: rel[0], label: `${word}后`, date: d, dateOnly: false };
  }

  // 日期词 + （可选）时段词 + （可选）时刻词
  // 数字日期用前后断言防误切长数字（「2026/9/21」不会切出 26/9）
  const re =
    /(今天|明天|后天|大后天|下周[一二三四五六日天]|[周週][一二三四五六日天]|(?<!\d)\d{1,2}月\d{1,2}[日号]|(?<!\d)\d{1,2}\/\d{1,2}(?!\d))?\s*(早上|上午|中午|下午|晚上)?\s*(?:(\d{1,2})点(半)?|(\d{1,2}):(\d{2}))?/g;
  for (const m of text.matchAll(re)) {
    const [full, dayWord, period, hZh, half, hColon, mm] = m;
    if (!dayWord && !period && !hZh && !hColon) continue;
    if (!full.trim()) continue;

    // 日期部分 → 该日 00:00 的 Date；null = 无日期词
    let dayDate: Date | null = null;
    let dayLabel = dayWord ?? "";
    if (dayWord) {
      const num = dayWord.match(/^(?:(\d{1,2})月(\d{1,2})[日号]|(\d{1,2})\/(\d{1,2}))$/);
      if (num) {
        const mo = Number(num[1] ?? num[3]);
        const dd = Number(num[2] ?? num[4]);
        if (mo >= 1 && mo <= 12 && dd >= 1 && dd <= 31) {
          dayDate = new Date(now.getFullYear(), mo - 1, dd);
          // 已过去的数字日期 = 明年的最近一次（周X 同款「最近将来」语义）
          if (dayDate < new Date(now.getFullYear(), now.getMonth(), now.getDate())) {
            dayDate = new Date(now.getFullYear() + 1, mo - 1, dd);
          }
          dayLabel = `${mo}/${dd}`;
        }
      } else if (dayWord === "今天") dayDate = nextWeekday(now, now.getDay(), false);
      else if (dayWord === "明天") dayDate = dayWithOffset(now, 1);
      else if (dayWord === "后天") dayDate = dayWithOffset(now, 2);
      else if (dayWord === "大后天") dayDate = dayWithOffset(now, 3);
      else {
        const target = WD[dayWord[dayWord.length - 1]];
        dayDate = nextWeekday(now, target, dayWord.startsWith("下周"));
      }
    }

    // 时刻部分（裸 N点 = 24 小时制，不因当前时刻猜上午下午）
    let time: { h: number; m: number } | null = null;
    if (hColon) {
      const h = parseInt(hColon, 10);
      const mi = parseInt(mm, 10);
      if (h <= 23 && mi <= 59) time = { h, m: mi };
    } else if (hZh) {
      const h = parseInt(hZh, 10);
      if (h <= 23) time = { h: periodTo(h, period), m: half ? 30 : 0 };
    }

    if (!dayDate && !time) continue;

    const date = new Date(dayDate ?? now);
    date.setSeconds(0, 0);
    let label: string;
    if (dayDate && time) {
      date.setHours(time.h, time.m, 0, 0);
      label = `${dayLabel} ${pad(time.h)}:${pad(time.m)}`;
    } else if (dayDate) {
      label = dayLabel;
    } else {
      date.setHours(time!.h, time!.m, 0, 0);
      label = `${pad(time!.h)}:${pad(time!.m)}`;
    }
    return { matched: full.trim(), label, date, dateOnly: !!dayDate && !time };
  }
  return null;
}

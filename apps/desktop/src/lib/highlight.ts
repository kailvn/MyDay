/**
 * 即时搜索的命中高亮：把文本按查询词切成段（hit = 命中段）。
 * 大小写不敏感、原样返回未命中前后缀；查询为空返回单段原文。
 * 纯函数（无 runes），SearchView 与 CommandPalette 共用。
 */
export interface HighlightSeg {
  text: string;
  hit: boolean;
}

export function highlightText(text: string, query: string): HighlightSeg[] {
  const q = query.trim().toLowerCase();
  if (!q) return [{ text, hit: false }];
  const out: HighlightSeg[] = [];
  const lower = text.toLowerCase();
  let i = 0;
  for (;;) {
    const idx = lower.indexOf(q, i);
    if (idx === -1) {
      if (i < text.length) out.push({ text: text.slice(i), hit: false });
      break;
    }
    if (idx > i) out.push({ text: text.slice(i, idx), hit: false });
    out.push({ text: text.slice(idx, idx + q.length), hit: true });
    i = idx + q.length;
  }
  return out;
}

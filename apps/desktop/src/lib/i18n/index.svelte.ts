/**
 * 界面多语言（中/英）。zh.ts 是 key 基准（语义化 key，`<命名空间>.<名称>`），
 * en.ts 与它同 key 集（类型上锁死，缺漏编译报错）。参数用 `{name}` 占位。
 *
 * - locale 决定取词；t() 在组件模板里使用时随 locale 变化自动重渲染（runes 追踪）。
 * - 初始 locale：`?lang=en|zh` 强制 > 已保存设置 ui_lang > navigator.language 猜测。
 * - `?e2e=1` 模式缺省锁 zh（e2e 可访问名定位基于中文文案）；`?e2e=1&lang=en` 可覆盖。
 */
import { zh } from './zh';
import { en } from './en';

export type Locale = 'zh' | 'en';
export type MessageKey = keyof typeof zh;

const dicts: Record<Locale, Record<MessageKey, string>> = { zh, en };

export const i18n = $state({ locale: 'zh' as Locale });

/** 文案取词。params 替换 `{name}` 占位；缺失参数原样保留，便于发现。 */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const tpl = dicts[i18n.locale][key] ?? key;
  if (!params) return tpl;
  return tpl.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in params ? String(params[name]) : m,
  );
}

/** 标题引用包裹：中文用「」，英文用直引号（拼接进提示文案时用）。 */
export function q(title: string): string {
  return i18n.locale === 'zh' ? `「${title}」` : `"${title}"`;
}

const SETTINGS_KEY = 'ui_lang';

/** main.ts 装配时注入 settings 读写，避免 i18n ↔ api 循环依赖。 */
interface LocalePersist {
  load(key: string): Promise<string | null>;
  save(key: string, value: string): Promise<void>;
}

let persist: LocalePersist | null = null;

function guessLocale(): Locale {
  return (navigator.language ?? '').toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

function urlOverride(): Locale | null {
  const q = new URLSearchParams(location.search).get('lang');
  return q === 'en' || q === 'zh' ? q : null;
}

/** 模块加载即同步猜一次（防首帧闪语言），异步精化交给 initLocale。 */
i18n.locale = guessLocale();

/** mount 前调用：e2e 锁 zh，saved 设置优先于猜测。 */
export async function initLocale(p: LocalePersist): Promise<void> {
  persist = p;
  const forced = urlOverride();
  if (forced) {
    i18n.locale = forced;
    return;
  }
  if (new URLSearchParams(location.search).has('e2e')) {
    i18n.locale = 'zh';
    return;
  }
  try {
    const saved = await p.load(SETTINGS_KEY);
    if (saved === 'en' || saved === 'zh') {
      i18n.locale = saved;
    }
  } catch {
    // 读取失败保持猜测值
  }
}

/** 设置页切换语言：立即生效并持久化（失败不阻断界面切换）。 */
export async function setLocale(l: Locale): Promise<void> {
  i18n.locale = l;
  try {
    await persist?.save(SETTINGS_KEY, l);
  } catch {
    // 持久化失败仅影响下次启动
  }
}

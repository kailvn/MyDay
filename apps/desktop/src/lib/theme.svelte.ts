/**
 * 界面主题：配色（palette）× 形态（mode）两轴，正交组合互不重写。
 * 取色一律在 themes.css 令牌层（全应用唯一取色处）；本模块只管状态与持久化：
 * 把 data-theme / data-mode 写到 <html>，CSS 按 [data-theme=…][data-mode=…] 换肤。
 *
 * - mode='system' 由 matchMedia 实时解析并监听变化；CSS 永远只见 light|dark。
 * - 初始：入口 HTML 头部内联脚本按 localStorage 镜像（myday.theme）在绘制前恢复，
 *   本模块加载时读同一镜像初始化状态（无镜像则缺省纸黄 + 跟随系统），
 *   initTheme 再读 SQLite 设置精化并回写镜像。
 * - 多窗口：主窗口保存后经 Tauri 全局事件广播，副窗口（overlay/quick-add/alarm）
 *   监听跟随（只改状态不回写，避免环）；无 Tauri 事件时静默降级为单窗口。
 */
import { emit, listen } from "@tauri-apps/api/event";

export type Palette = "paper" | "celadon" | "slate" | "graphite" | "rose";
export type ThemeMode = "system" | "light" | "dark";

export const PALETTES: readonly Palette[] = ["paper", "celadon", "slate", "graphite", "rose"];

/** 设置页色板圆点预览（亮/暗双色）+ 揭幕底色来源（仅这两处用——真实取色一律在 themes.css）。 */
export const PALETTE_SWATCH: Record<Palette, { bg: string; darkBg: string }> = {
  paper: { bg: "#f6efdd", darkBg: "#2a251b" },
  celadon: { bg: "#e8eee4", darkBg: "#1f2620" },
  slate: { bg: "#e8ecf1", darkBg: "#1e242e" },
  graphite: { bg: "#ececec", darkBg: "#222222" },
  rose: { bg: "#f6e8ea", darkBg: "#2b2024" },
};

const KEY_PALETTE = "ui_theme";
const KEY_MODE = "ui_theme_mode";
/** 跨窗口跟随事件（payload = { palette, mode }）。 */
const THEME_EVENT = "myday://theme";

const darkQuery =
  typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;

function isDark(): boolean {
  return theme.mode === "dark" || (theme.mode === "system" && (darkQuery?.matches ?? false));
}

/** 当前形态下的主题背景色（揭幕底色用，取自预览注册表）。 */
function themeBg(): string {
  const sw = PALETTE_SWATCH[theme.palette];
  return isDark() ? sw.darkBg : sw.bg;
}

/**
 * 揭幕底色钩子：main.ts 注入（把 webview 底色设为主题 bg——GTK 在 webview 首帧前
 * 露出的就是这层底色，不设则白闪）。theme 模块不 import api（避免循环依赖），
 * 与 i18n 的 persist 注入同款；hook 内部自行容错。
 */
let bgHook: ((hex: string) => void) | null = null;

export function setThemeBgHook(fn: (hex: string) => void): void {
  bgHook = fn;
}

/** 把当前主题同步写到 <html> 属性（CSS 侧的唯一入口）。 */
function apply(): void {
  const el = document.documentElement;
  el.dataset.theme = theme.palette;
  el.dataset.mode = isDark() ? "dark" : "light";
  bgHook?.(themeBg());
}

function isPalette(v: unknown): v is Palette {
  return typeof v === "string" && (PALETTES as readonly string[]).includes(v);
}

function isMode(v: unknown): v is ThemeMode {
  return v === "system" || v === "light" || v === "dark";
}

/**
 * 首帧镜像（localStorage）：index.html / mobile.html 头部内联脚本按它在绘制前
 * 恢复 <html> 属性，消除「先纸黄再跳到已存配色」的启动闪色。
 * SQLite 设置仍是权威——initTheme 异步精化后回写镜像；坏值/不可用按缺省处理。
 * 键名与取值域须与两个入口 HTML 里的内联脚本保持一致（脚本无法 import，靠注释约定）。
 */
const MIRROR_KEY = "myday.theme";

function readMirror(): { palette: Palette; mode: ThemeMode } | null {
  try {
    const raw = JSON.parse(localStorage.getItem(MIRROR_KEY) ?? "null");
    if (raw && isPalette(raw.palette) && isMode(raw.mode)) return raw;
  } catch {
    // localStorage 不可用或坏值：按缺省
  }
  return null;
}

function writeMirror(): void {
  try {
    localStorage.setItem(MIRROR_KEY, JSON.stringify({ palette: theme.palette, mode: theme.mode }));
  } catch {
    // 镜像写失败只影响下次启动首帧（SQLite 仍是权威）
  }
}

// 初始化与头部脚本同源同逻辑：镜像优先，缺省纸黄 + 跟随系统；再写一次属性兜底
//（头部脚本缺失/失败时也不闪）。apply 幂等，与脚本写出相同属性 = 无二次跳变。
const boot = readMirror();
export const theme = $state({ palette: boot?.palette ?? "paper", mode: boot?.mode ?? "system" });
apply();

/** main.ts 装配时注入 settings 读写，避免 theme ↔ api 循环依赖。 */
interface ThemePersist {
  load(key: string): Promise<string | null>;
  save(key: string, value: string): Promise<void>;
}

let persist: ThemePersist | null = null;

/** mount 前调用：读已保存设置精化 + 装系统形态监听与跨窗口跟随。 */
export async function initTheme(p: ThemePersist): Promise<void> {
  persist = p;
  // 系统亮暗翻转时跟随（仅 system 模式生效）
  darkQuery?.addEventListener("change", () => {
    if (theme.mode === "system") apply();
  });
  try {
    const saved = await p.load(KEY_PALETTE);
    if (isPalette(saved)) theme.palette = saved;
    const savedMode = await p.load(KEY_MODE);
    if (isMode(savedMode)) theme.mode = savedMode;
    writeMirror(); // SQLite 精化后回写镜像（升级后首启种子 / 漂移纠正）
  } catch {
    // 读取失败保持镜像值或缺省（纸黄 + 跟随系统）
  }
  apply();
  // 其它窗口改了主题：跟随（只改状态，不回写不广播，避免环）
  try {
    await listen<{ palette: Palette; mode: ThemeMode }>(THEME_EVENT, (e) => {
      if (isPalette(e.payload.palette)) theme.palette = e.payload.palette;
      if (isMode(e.payload.mode)) theme.mode = e.payload.mode;
      apply();
    });
  } catch {
    // 无 Tauri 事件环境（e2e/浏览器）：单窗口即全部
  }
}

/** 广播给其余窗口（fire-and-forget，失败只影响副窗口实时跟随）。 */
function broadcast(): void {
  emit(THEME_EVENT, { palette: theme.palette, mode: theme.mode }).catch(() => {});
}

/** 切换配色：立即生效并持久化（失败不阻断界面切换）。 */
export async function setPalette(pal: Palette): Promise<void> {
  theme.palette = pal;
  apply();
  writeMirror();
  try {
    await persist?.save(KEY_PALETTE, pal);
    broadcast();
  } catch {
    // 持久化失败仅影响下次启动
  }
}

/** 切换形态（跟随系统/浅色/深色）：立即生效并持久化。 */
export async function setMode(mode: ThemeMode): Promise<void> {
  theme.mode = mode;
  apply();
  writeMirror();
  try {
    await persist?.save(KEY_MODE, mode);
    broadcast();
  } catch {
    // 持久化失败仅影响下次启动
  }
}

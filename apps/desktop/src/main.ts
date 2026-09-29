import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { initHolidays } from "./lib/holidays.svelte";
import { api } from "./lib/api";
import { initLocale } from "./lib/i18n";
import { initTheme, setThemeBgHook } from "./lib/theme.svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";

// 界面语言（中/英）：?lang= 强制 > 已保存设置 > 系统语言；e2e 模式锁 zh
initLocale({ load: api.getSetting, save: api.setSetting }).catch(() => {});

// 浏览器 E2E 模式（?e2e=1）：mount 前装内存 mock，替代 Tauri 运行时
if (new URLSearchParams(location.search).has("e2e")) {
  const { installE2eMock } = await import("./lib/e2e-mock");
  installE2eMock();
}

// 揭幕底色钩子：GTK 在 webview 首帧呈现前露的是 webview 底色（默认白），
// 设成主题 bg 后揭幕不再白闪。仅主窗口——quick-add 等透明窗口须保持透明底。
// 无 Tauri 环境（e2e/浏览器）静默跳过。
setThemeBgHook((hex) => {
  try {
    if (getCurrentWindow().label === "main") void api.setWindowBg(hex).catch(() => {});
  } catch {
    // 浏览器 e2e：无窗口系统
  }
});

// 界面主题：mount 前等权威设置精化（HTML 头部脚本已按 localStorage 镜像预置
// 属性，这里做纠偏；apply 时同步经钩子设揭幕底色）。race 2s 兜底——主窗口
// visible:false，这里若意外挂起窗口将永不出现。
await Promise.race([
  initTheme({ load: api.getSetting, save: api.setSetting }).catch(() => {}),
  new Promise<void>((r) => setTimeout(r, 2000)),
]);

// 节假日 JSON 装载（用户文件优先、内置回退；失败保持无角标）。
// 不阻塞首帧：装载完成后角标/文案自然出现。静态导入避免 chunk 拆分。
initHolidays().catch((e) => console.error("myday: 节假日数据装载失败", e));

const app = mount(App, {
  target: document.getElementById("app")!,
});

// 主窗口揭幕：mount 完成 + 揭幕底色已设，直接 show（首个可见帧即主题色底 +
// 完整界面）。注意不能等 requestAnimationFrame——隐藏窗口的渲染管线被挂起，
// rAF 永不触发，等它 = 永不揭幕。quick-add / overlay / alarm 的显隐由 Rust 驱动。
try {
  const win = getCurrentWindow();
  if (win.label === "main") void win.show().catch(() => {});
} catch {
  // 浏览器 e2e：无窗口系统
}

export default app;

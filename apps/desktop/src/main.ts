import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";
import { initHolidays } from "./lib/holidays.svelte";
import { api } from "./lib/api";
import { initLocale } from "./lib/i18n";

// 界面语言（中/英）：?lang= 强制 > 已保存设置 > 系统语言；e2e 模式锁 zh
initLocale({ load: api.getSetting, save: api.setSetting }).catch(() => {});

// 浏览器 E2E 模式（?e2e=1）：mount 前装内存 mock，替代 Tauri 运行时
if (new URLSearchParams(location.search).has("e2e")) {
  const { installE2eMock } = await import("./lib/e2e-mock");
  installE2eMock();
}

// 节假日 JSON 装载（用户文件优先、内置回退；失败保持无角标）。
// 不阻塞首帧：装载完成后角标/文案自然出现。静态导入避免 chunk 拆分。
initHolidays().catch((e) => console.error("myday: 节假日数据装载失败", e));

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;

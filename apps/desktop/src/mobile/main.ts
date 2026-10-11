/**
 * 移动端入口（tauri.android.conf.json 指向 mobile.html → 本文件）。
 *
 * 与桌面入口（src/main.ts）平行：复用 lib/ 的 api / i18n / holidays / fields
 * 与纯逻辑模块（timewords / recurrence），不引入桌面 Shell / 多窗口路由。
 * 启动时顺带申请系统通知权限（移动端提醒弹窗的前提，Android 13+ 必需）。
 */
import { mount } from "svelte";
import { initLocale } from "../lib/i18n";
import { initHolidays } from "../lib/holidays.svelte";
import { initFieldStore } from "../lib/fields.svelte";
import { initTheme } from "../lib/theme.svelte";
import { api } from "../lib/api";
import { t } from "../lib/i18n";
import App from "./App.svelte";
import "./mobile.css";
import "../app.css";

// 浏览器调试 / E2E 模式（?e2e=1）：mount 前装内存 mock，替代 Tauri 运行时。
// 与桌面入口同一套 mock 与编译期门控（__MYDAY_E2E__，见 vite.config.ts：
// dev 构建恒 true、生产恒 false——整段被消除，mock 模块不进产物）。
// 无后端环境（手机之外）申请通知权限会静默失败，mock 模式跳过。
if (__MYDAY_E2E__ && new URLSearchParams(location.search).has("e2e")) {
  const { installE2eMock } = await import("../lib/e2e-mock");
  installE2eMock();
} else {
  void (async () => {
    try {
      const plugin = await import("@tauri-apps/plugin-notification");
      let granted = await plugin.isPermissionGranted();
      if (!granted) {
        granted = (await plugin.requestPermission()) === "granted";
      }
      if (!granted) {
        const { toast } = await import("../lib/toast.svelte");
        toast.show(t("mobile.notif_denied"));
      }
    } catch (e) {
      console.error("myday mobile: 通知权限申请失败", e);
    }
  })();
}

initLocale({ load: api.getSetting, save: api.setSetting }).catch(() => {});
initTheme({ load: api.getSetting, save: api.setSetting }).catch(() => {});
initHolidays().catch((e) => console.error("myday mobile: 节假日数据装载失败", e));
void initFieldStore();

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;

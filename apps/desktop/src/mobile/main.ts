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

initLocale({ load: api.getSetting, save: api.setSetting }).catch(() => {});
initTheme({ load: api.getSetting, save: api.setSetting }).catch(() => {});
initHolidays().catch((e) => console.error("myday mobile: 节假日数据装载失败", e));
void initFieldStore();

// 系统通知权限：优雅降权——未授予只提示一次，不阻塞首帧（提醒循环在 Rust 侧照常跑）
void (async () => {
  try {
    const plugin = await import("@tauri-apps/plugin-notification");
    let granted = await plugin.isPermissionGranted();
    if (!granted) {
      granted = (await plugin.requestPermission()) === "granted";
    }
    if (!granted) {
      const { toast } = await import("../lib/toast.svelte");
      toast(t("mobile.notif_denied"));
    }
  } catch (e) {
    console.error("myday mobile: 通知权限申请失败", e);
  }
})();

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;

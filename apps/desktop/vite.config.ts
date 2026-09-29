import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// 多页入口：index.html = 桌面主窗口（含 quick-add / overlay 路由），
// mobile.html = 移动端入口（tauri.android.conf.json 的 main 窗口指向它）
export default defineConfig(({ mode }) => ({
  // e2e mock 开关（编译期常量，声明见 src/vite-env.d.ts）：只在非生产构建为
  // true。生产构建（vite build，mode=production）下 main.ts 的门控分支被死代码
  // 消除，e2e-mock / e2e-viewmock 不进 dist；playwright e2e 走 dev server
  // （mode=development），mock 照常可用。
  define: {
    __MYDAY_E2E__: JSON.stringify(mode !== "production"),
  },
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Android 真机调试：tauri CLI 用局域网 IP 访问 dev server，
    // 启动时置 TAURI_DEV_HOST=本机局域网IP（桌面开发不设，仍走 localhost）
    host: process.env.TAURI_DEV_HOST || false,
  },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        mobile: fileURLToPath(new URL("./mobile.html", import.meta.url)),
      },
      onwarn(warning, warn) {
        // recurrence / tpltime 是纯模块（golden 用 node 直跑），对 runes 版 i18n
        // 只能动态导入（见两文件头注释）；i18n 本就随主 chunk 静态分发，此条
        // 警告是该兼容设计的已知误报，定向静音，其余照常输出。
        // 注：经 vite:reporter 转发后 code 统一是 PLUGIN_WARNING，只能按文案匹配。
        if (
          warning.code === "PLUGIN_WARNING" &&
          warning.message.includes("src/lib/i18n/index.ts") &&
          warning.message.includes("dynamic import will not move module")
        ) {
          return;
        }
        warn(warning);
      },
    },
  },
}));

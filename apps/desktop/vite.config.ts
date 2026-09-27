import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// 多页入口：index.html = 桌面主窗口（含 quick-add / overlay 路由），
// mobile.html = 移动端入口（tauri.android.conf.json 的 main 窗口指向它）
export default defineConfig({
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
    },
  },
});

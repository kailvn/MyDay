import { defineConfig, devices } from "@playwright/test";

/**
 * 真 Chromium 端到端（前端走 ?e2e=1 内存 mock，替代 Tauri 运行时）：
 *   pnpm e2e          # headless 全量（webServer 自动起 vite dev，1420 已有 dev server 则复用）
 *   pnpm e2e --ui     # 可视化调试（点选用例、看时间线）
 * 失败自动留截图与 trace：test-results/
 * 定位约定见 e2e/flows.spec.ts 头注释。
 */
export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  forbidOnly: !!process.env.CI,
  use: {
    baseURL: "http://localhost:1420",
    // 对齐 Tauri 主窗口尺寸，行为更接近真机
    viewport: { width: 1180, height: 780 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1180, height: 780 } },
    },
  ],
});

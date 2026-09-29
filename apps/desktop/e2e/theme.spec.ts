/**
 * THEME-SPEC 验收用例（E2E，?e2e=1 内存 mock）：
 *   T1 外观模式：深色 ↔ 浅色 → <html data-mode> 即时切换
 *   T2 配色：色板圆点 → <html data-theme> 切换，缺省纸黄
 *   T3 换肤实际生效：配色切换后 body 背景色（var(--bg) 取值）跟着变
 *   T4 防闪色：重载后首帧前恢复配色（localStorage 镜像 + 头部内联脚本）
 *
 * 定位约定同 flows.spec.ts：语义定位（role / testid / 文案）。
 */
import { expect, test, type Page } from "@playwright/test";

const navButton = (page: Page, name: string | RegExp) =>
  page.getByRole("navigation").getByRole("button", { name });

test.beforeEach(async ({ page }) => {
  await page.goto("/?e2e=1");
  await navButton(page, /设置/).click();
  await expect(page.getByTestId("theme-mode")).toBeVisible();
});

test("T1 主题：外观模式深色 ↔ 浅色即时切换", async ({ page }) => {
  const html = page.locator("html");
  // 缺省跟随系统（e2e 环境 prefers-color-scheme: light → data-mode=light）
  await expect(html).toHaveAttribute("data-mode", "light");
  await page.getByTestId("theme-mode").selectOption("dark");
  await expect(html).toHaveAttribute("data-mode", "dark");
  await page.getByTestId("theme-mode").selectOption("light");
  await expect(html).toHaveAttribute("data-mode", "light");
});

test("T2 主题：色板圆点切换配色，缺省纸黄", async ({ page }) => {
  const html = page.locator("html");
  await expect(html).toHaveAttribute("data-theme", "paper");
  await page.getByTestId("palette-celadon").click();
  await expect(html).toHaveAttribute("data-theme", "celadon");
  await page.getByTestId("palette-graphite").click();
  await expect(html).toHaveAttribute("data-theme", "graphite");
  await page.getByTestId("palette-rose").click();
  await expect(html).toHaveAttribute("data-theme", "rose");
  await expect(html).toHaveAttribute("data-mode", "light"); // 配色切换不影响形态轴
});

test("T3 主题：换配色后实际取色变化（--bg 跟随令牌层）", async ({ page }) => {
  const bodyBg = () =>
    page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const paperBg = await bodyBg();
  await page.getByTestId("palette-slate").click();
  const slateBg = await bodyBg();
  expect(slateBg).not.toBe(paperBg);
});

test("T4 主题：重载后首帧前恢复配色（localStorage 镜像防闪色）", async ({ page }) => {
  await page.getByTestId("palette-rose").click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "rose");
  await page.reload();
  // ?e2e=1 内存 mock 的 settings 不跨重载（SQLite 读回 = paper），但头部内联脚本
  // 在绘制前按 localStorage 镜像写入属性——重载后 data-theme 仍应是 rose
  await expect(page.locator("html")).toHaveAttribute("data-theme", "rose");
});

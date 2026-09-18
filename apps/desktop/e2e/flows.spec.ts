/**
 * 主流程端到端（T1–T8，真 Chromium + ?e2e=1 内存 mock）。
 *
 * 定位约定（防布局重构失效的核心）：
 *  - 交互目标一律语义定位：getByRole（按钮/对话框/勾选框，名称取可见文案或 aria-label）、
 *    getByTestId、getByText；改样式、调布局不影响。
 *  - 仅几何计算（拖拽的坐标换算）允许碰结构类：.day-col / .grid-wrap，这些是布局骨架本身。
 *  - 时间网格参数与 WeekGrid 源码耦合：HOUR_H=44、吸附 15 分钟；改网格密度需同步此处。
 *
 * 运行：pnpm e2e（webServer 自动起 vite dev；?e2e=1 由 main.ts 装内存 mock）
 */
import { expect, test, type Page } from "@playwright/test";

const HOUR_H = 44; // WeekGrid 每小时像素高
const SNAP_PX = (HOUR_H / 60) * 15; // 15 分钟吸附对应的像素

const navButton = (page: Page, name: string | RegExp) =>
  page.getByRole("navigation").getByRole("button", { name });

/** 事件块的语义定位（可访问名 = 「时间 + 标题」拼接文本） */
const eventBlock = (page: Page, title: string) =>
  page.getByRole("button", { name: new RegExp(title) });

/** 跳到周视图并等待网格挂载 */
async function gotoWeek(page: Page) {
  await navButton(page, /日历/).click();
  await page.getByRole("button", { name: "周", exact: true }).click();
  await expect(page.getByTestId("week-grid")).toBeVisible();
}

/** 以元素中心为起点按像素偏移拖拽；cancel=true 时先按 Esc 再松手（模拟拖一半放弃） */
async function dragBy(
  page: Page,
  from: { x: number; y: number },
  dx: number,
  dy: number,
  opts: { cancel?: boolean } = {},
) {
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  await page.mouse.move(from.x + dx / 2, from.y + dy / 2, { steps: 6 });
  await page.mouse.move(from.x + dx, from.y + dy, { steps: 6 });
  if (opts.cancel) await page.keyboard.press("Escape");
  await page.mouse.up();
}

const centerOf = (box: { x: number; y: number; width: number; height: number }) => ({
  x: box.x + box.width / 2,
  y: box.y + box.height / 2,
});

test.beforeEach(async ({ page }) => {
  await page.goto("/?e2e=1");
  await expect(page.getByRole("heading", { level: 1 })).toContainText(/今天/);
});

test("T1 首屏：今天视图 + 节假日文案 + 提醒中心入口", async ({ page }) => {
  await expect(page.locator("main")).toContainText(/假期|中秋|国庆|元旦|春节|清明|劳动|端午/);
  await expect(page.getByRole("button", { name: "提醒中心" })).toBeVisible();
});

test("T2 日历：周/日/月切换与网格挂载", async ({ page }) => {
  await navButton(page, /日历/).click();

  // 周视图：时间网格挂载、月网格卸载、7 列、列头节假日角标、事件块渲染
  await page.getByRole("button", { name: "周", exact: true }).click();
  await expect(page.getByTestId("week-grid")).toBeVisible();
  await expect(page.locator(".calendar-layout")).toHaveCount(0);
  await expect(page.locator(".day-col")).toHaveCount(7);
  await expect(page.locator(".day-head .hol").first()).toBeVisible();
  for (const title of ["产品评审", "团队周会", "晨跑"]) {
    // 晨跑 @daily 一周多块，取首个即可
    await expect(eventBlock(page, title).first()).toBeVisible();
  }

  // 日视图：单列
  await page.getByRole("button", { name: "日", exact: true }).click();
  await expect(page.locator(".day-col")).toHaveCount(1);

  // 月视图：月网格恢复 + 休/班角标 + 当天面板有条目
  await page.getByRole("button", { name: "月", exact: true }).click();
  await expect(page.locator(".calendar-layout")).toBeVisible();
  await expect(page.locator(".cell .hol").first()).toBeVisible();
  await expect(page.locator(".day-panel li").first()).toBeVisible();
});

test("T2.5 月视图：整周网格不溢出行 + 已完成到期待办灰化", async ({ page }) => {
  await navButton(page, /日历/).click();
  await page.getByRole("button", { name: "月", exact: true }).click();
  await expect(page.locator(".calendar-layout")).toBeVisible();

  // 网格恒为整周（周一 ~ 月末所在周日）：月末推到周日而非下周一，不产生幻影第 6 行
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), 1);
  start.setDate(start.getDate() - ((start.getDay() + 6) % 7));
  const end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
  end.setDate(end.getDate() + ((7 - end.getDay()) % 7));
  const expectedCells = Math.round((end.getTime() - start.getTime()) / 86_400_000) + 1;
  await expect(page.locator(".grid .cell")).toHaveCount(expectedCells);

  // 完成语义（红=未完成的截止）：种子里昨天完成的「已完成的事」为灰化 done 态，不进红色 due
  const doneChip = page.locator(".cell-ev", { hasText: "已完成的事" });
  await expect(doneChip).toHaveText(/12:00/);
  await expect(doneChip).toHaveClass(/done/);
  await expect(doneChip).not.toHaveClass(/due/);

  // 当天面板就地勾回 / 再勾掉：chip 在红色 due ↔ 灰化 done 间联动
  const y = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  const yKey = `${y.getFullYear()}-${String(y.getMonth() + 1).padStart(2, "0")}-${String(
    y.getDate(),
  ).padStart(2, "0")}`;
  await page.locator(`.cell[data-cellday="${yKey}"]`).click();
  const row = page.locator(".day-panel li").filter({ hasText: "已完成的事" });
  await expect(row.getByRole("checkbox")).toBeChecked();
  await row.getByRole("checkbox").click();
  await expect(doneChip).toHaveClass(/due/);
  await row.getByRole("checkbox").click();
  await expect(doneChip).toHaveClass(/done/);
});

test("T3 提醒中心：历史提醒在列，可开关", async ({ page }) => {
  await page.getByRole("button", { name: "提醒中心" }).click();
  const center = page.getByRole("dialog", { name: "提醒中心" });
  await expect(center).toBeVisible();
  await expect(center).toContainText(/产品评审|买牛奶/);
  await center.getByRole("button", { name: /关闭/ }).click();
  await expect(page.getByRole("dialog", { name: "提醒中心" })).toHaveCount(0);
});

test("T4 详情面板：待办转日程、日程生成记录（两步确认）", async ({ page }) => {
  await navButton(page, /日历/).click();
  await expect(page.locator(".day-panel li").first()).toBeVisible();

  // 待办「买牛奶」→ 转日程
  await page.locator(".day-panel li").filter({ hasText: "买牛奶" }).click();
  const detail = page.getByRole("dialog", { name: "条目详情" });
  await expect(detail).toBeVisible();
  await detail.getByRole("button", { name: "转日程" }).click();
  await detail.getByRole("button", { name: "确认转日程？" }).click();
  const detail2 = page.getByRole("dialog", { name: "条目详情" });
  await expect(detail2.locator(".type-badge")).toContainText(/日程/);
  await detail2.getByRole("button", { name: /关闭/ }).click();

  // 日程「产品评审」→ 生成记录
  await page.locator(".day-panel li").filter({ hasText: "产品评审" }).click();
  const detail3 = page.getByRole("dialog", { name: "条目详情" });
  await expect(detail3).toBeVisible();
  await detail3.getByRole("button", { name: "生成记录" }).click();
  await detail3.getByRole("button", { name: "确认生成记录？" }).click();
  await expect(
    page.getByRole("dialog", { name: "条目详情" }).locator(".type-badge"),
  ).toContainText(/记录/);
});

test("T5 统计：预置容器挂载（窗口由挂件自带，页面无全局范围档）", async ({ page }) => {
  await navButton(page, /统计/).click();

  // 三个预置容器挂载：热力图 / 打卡连续 / 数值趋势（容器模型，§8 v2）
  await expect(page.locator('[data-container-id="view_builtin_stats_heatmap"]')).toBeVisible();
  await expect(page.locator('[data-container-id="view_builtin_stats_streaks"]')).toBeVisible();
  await expect(page.locator('[data-container-id="view_builtin_stats_series"]')).toBeVisible();

  // 热力图桶数 = 挂件自己的 window（预设 365 天）；范围档 chips 已移除
  const heatCells = page.locator('[data-container-id="view_builtin_stats_heatmap"] .heat .cell');
  await expect(heatCells).toHaveCount(365);
  await expect(page.locator("main svg.trend, main .empty").first()).toBeVisible();
});

test("T6 设置：一键备份出现成功 toast", async ({ page }) => {
  await navButton(page, /设置/).click();
  await expect(page.getByRole("button", { name: /导出日历（ICS）/ })).toBeVisible();
  await page.getByRole("button", { name: /一键备份（zip）/ }).click();
  await expect(page.getByRole("status")).toContainText(/备份完成/);
});

test("T7 待办视图：勾选框在列", async ({ page }) => {
  await navButton(page, /待办/).click();
  await expect(page.getByRole("checkbox").first()).toBeVisible();
});

test("T8 设置：节假日 JSON 导入（非法报错 / 生效覆盖 / 恢复内置）", async ({ page }) => {
  await navButton(page, /设置/).click();
  const main = page.locator("main");
  await expect(main).toContainText(/内置默认/);

  // 非法 JSON：报语法错误，来源不变
  const ta = page.getByRole("textbox", { name: /导入自定义 JSON/ });
  await ta.fill('{"holidays": [');
  await page.getByRole("button", { name: "导入并生效" }).click();
  await expect(main).toContainText(/语法错误/);
  await expect(main).toContainText(/内置默认/);

  // 合法 2027 自造数据：生效并覆盖
  await ta.fill(
    JSON.stringify({
      version: 1,
      sources: ["测试用 2027 假数据"],
      holidays: [{ name: "测试节", from: "2027-03-01", to: "2027-03-03" }],
      workdays: [{ date: "2027-03-06", name: "测试节" }],
    }),
  );
  await page.getByRole("button", { name: "导入并生效" }).click();
  await expect(main).toContainText(/自定义导入/);
  await expect(main).toContainText(/2027/);
  await expect(main).toContainText(/测试用 2027 假数据/);

  // 恢复内置
  await page.getByRole("button", { name: "恢复内置" }).click();
  await expect(main).toContainText(/内置默认/);
});

test.describe("T9 周视图拖拽（真实几何，无合成坐标）", () => {
  test("拖拽改期 → toast 撤销 → Esc 取消", async ({ page }) => {
    await gotoWeek(page);
    const ev = eventBlock(page, "产品评审"); // 今天 10:00
    await ev.scrollIntoViewIfNeeded();
    const box = (await ev.boundingBox())!;

    // 下移 225 分钟（15min 吸附整除）→ 13:45
    const dy225 = 225 * (HOUR_H / 60);
    await dragBy(page, centerOf(box), 0, dy225);
    await expect(ev).toContainText("13:45");
    const toast = page.getByRole("status");
    await expect(toast).toContainText(/已改期/);

    // 撤销 → 回 10:00
    await toast.getByRole("button", { name: "撤销" }).click();
    await expect(ev).toContainText("10:00");

    // 拖一半按 Esc：数据不变、不出改期 toast
    const box2 = (await ev.boundingBox())!;
    await dragBy(page, centerOf(box2), 0, 4 * HOUR_H, { cancel: true });
    await expect(ev).toContainText("10:00");
    await expect(page.getByRole("status")).not.toBeVisible();
  });

  test("重复日程跨列拖拽：规则改写为每周三", async ({ page }) => {
    await gotoWeek(page);
    const ev = eventBlock(page, "团队周会"); // 周四 14:00 @weekly:4
    await ev.scrollIntoViewIfNeeded();
    const box = (await ev.boundingBox())!;
    const wed = (await page.locator(".day-col").nth(2).boundingBox())!;

    const from = centerOf(box);
    const dx = wed.x + wed.width / 2 - from.x;
    await dragBy(page, from, dx, 0);
    await expect(page.getByRole("status")).toContainText(/每周三/);
  });

  test("空白处拖选创建：面板预填起止时间", async ({ page }) => {
    await gotoWeek(page);
    await expect(eventBlock(page, "团队周会")).toBeVisible(); // 网格数据就绪

    // 周一 09:00→10:00 空档（mock 里周一仅有 07:00 晨跑）
    const mon = (await page.locator(".day-col").nth(0).boundingBox())!;
    const from = { x: mon.x + mon.width / 2, y: mon.y + 9 * HOUR_H };
    await dragBy(page, from, 0, HOUR_H);

    const panel = page.getByRole("dialog", { name: "新建条目" });
    await expect(panel).toBeVisible();
    await expect(panel).toContainText("09:00");
    await expect(panel).toContainText("10:00");
  });
});

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

test("T8.5 附件：粘路径记链接，文件夹 chip 以 📂 区分（拖放同管道，OS 级拖拽无法模拟）", async ({ page }) => {
  // 快速添加按钮在侧栏但不在 navigation 容器内，不能用 navButton
  await page.getByRole("button", { name: /快速添加/ }).click();
  const panel = page.getByRole("dialog", { name: "新建条目" });
  await expect(panel).toBeVisible();

  const linkInput = panel.getByPlaceholder(/粘贴路径回车/);
  // 尾分隔符 → mock path_is_dir 判为目录；chips 据此切 📂 前缀
  await linkInput.fill("/tmp/项目资料/");
  await linkInput.press("Enter");
  await linkInput.fill("/tmp/周报.pdf");
  await linkInput.press("Enter");
  await expect(panel.getByRole("button", { name: /📂 项目资料/ })).toBeVisible();
  await expect(panel.getByRole("button", { name: /📎 周报\.pdf/ })).toBeVisible();

  // 空标题自动取首个链接名；保存即建条目
  await panel.getByRole("button", { name: "保存" }).click();
  await expect(page.getByRole("status")).toContainText(/项目资料/);
});

test("T8.6 未排期池：拖入月格排期 23:59，顺延与清除闭环", async ({ page }) => {
  const pad2 = (n: number) => String(n).padStart(2, "0");
  const dayKey = (offset: number) =>
    page.evaluate(
      (o) => {
        const d = new Date();
        d.setDate(d.getDate() + o);
        const p = (n: number) => String(n).padStart(2, "0");
        return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
      },
      offset,
    );

  await navButton(page, /日历/).click();
  const panel = page.locator(".day-panel");
  await expect(panel).toContainText("未排期待办");
  const poolRow = panel.locator("li", { hasText: "整理书架" });
  await expect(poolRow).toBeVisible();

  // 池行拖到 (今天+10) 的月格 → 截止排到那天 23:59，行离开池
  const key = await dayKey(10);
  const cell = page.locator(`[data-cellday="${key}"]`);
  const from = centerOf((await poolRow.boundingBox())!);
  const to = centerOf((await cell.boundingBox())!);
  await dragBy(page, from, to.x - from.x, to.y - from.y);
  await expect(page.getByRole("status")).toContainText(/截止已改到/);
  await expect(cell).toContainText("整理书架");
  await expect(panel).not.toContainText("整理书架");

  // 选中落点日 → 到期行悬停顺延 +1天 → 再清除退回池
  await cell.click();
  const dueRow = panel.locator("li", { hasText: "整理书架" });
  await dueRow.hover();
  await dueRow.getByRole("button", { name: "＋1天" }).click();
  await expect(page.getByRole("status")).toContainText(/截止已改到/);

  const nextKey = await dayKey(11);
  await page.locator(`[data-cellday="${nextKey}"]`).click();
  const movedRow = panel.locator("li", { hasText: "整理书架" });
  await movedRow.hover();
  await movedRow.getByRole("button", { name: "清除" }).click();
  await expect(page.getByRole("status")).toContainText(/已清除截止/);
  await expect(panel).toContainText("整理书架");
});

test("T8.7 未排期池 × 周视图：拖入时段格排到具体钟点，拖出取消，可撤销", async ({ page }) => {
  await gotoWeek(page);

  // 池侧栏：与月视图池同一条种子（无 due 无 start）
  const pool = page.locator(".pool-panel");
  await expect(pool).toContainText("未排期待办");
  const row = pool.locator("li", { hasText: "整理书架" });
  await expect(row).toBeVisible();

  // 目标列避开种子恒有的两个到期日——今天（买牛奶）与本周四（交周报）：
  // 恒取周五，周五恰为今天时取周六 → 拖入前列内恒无到期，断言与运行日解耦
  const todayCol = (new Date().getDay() + 6) % 7; // 周一 = 0
  const col = todayCol === 4 ? 5 : 4;
  const sub = page.locator(".day-sub").nth(col);
  await expect(sub).not.toContainText("到期");
  // 拖出网格（仍在池侧栏上）松手 = 取消
  await dragBy(page, centerOf((await row.boundingBox())!), -60, -60);
  await expect(page.getByRole("status")).not.toBeVisible();
  await expect(row).toBeVisible();

  // 拖到目标列 10:30（15 分钟吸附）：落点按网格几何换算（与组件内部同公式：
  // 分钟 = (clientY - 网格顶) / HOUR_H * 60）；1180×780 视口内 10:30 恒可见
  //（时间网格超出视口的部分靠 main 滚动，深时段需先滚 main，这里不必）
  const grid = (await page.getByTestId("week-grid").boundingBox())!;
  const dayCol = (await page.locator(".day-col").nth(col).boundingBox())!;
  const to = { x: dayCol.x + dayCol.width / 2, y: grid.y + 10.5 * HOUR_H };
  const from = centerOf((await row.boundingBox())!);
  await dragBy(page, from, to.x - from.x, to.y - from.y);

  // 写入具体钟点：toast 报「截止已改到 … 10:30」，行离开池（池空则整个侧栏消失），
  // 目标列到期计数 +1
  const toast = page.getByRole("status");
  await expect(toast).toContainText(/截止已改到/);
  await expect(toast).toContainText("10:30");
  await expect(pool).toHaveCount(0);
  await expect(sub).toContainText("到期 1");

  // 撤销 = 清除截止，退回池（undo 补丁 clear_due_at）
  await toast.getByRole("button", { name: "撤销" }).click();
  await expect(pool).toContainText("整理书架");
  await expect(sub).not.toContainText("到期");
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

test("T9 快速添加：入口/行内三档日期 chips，时段相对已填开始日期", async ({ page }) => {
  const dayStr = (offset: number) => {
    const d = new Date();
    d.setDate(d.getDate() + offset);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  };

  // 首页直达（未展开态）：+ 打开即见时间入口，点「今天」一步到位（激活+预填+锁定待办+展开）
  await page.getByRole("button", { name: /＋ 快速添加/ }).click();
  const panel = page.getByRole("dialog", { name: "新建条目" });
  await panel.getByRole("button", { name: "＋ 截止" }).waitFor();
  await page.getByTestId("entry-due-今天").click();
  await expect(page.getByTestId("tp-due")).toHaveText(`${dayStr(0)} 23:59`);
  // 行内三档 chips：点「后天」即改截止（不关面板；锁成待办后时间组整组隐藏，名字唯一）
  await panel.getByRole("button", { name: "后天" }).click();
  await expect(page.getByTestId("tp-due")).toHaveText(`${dayStr(2)} 23:59`);
  // 填标题保存 → 今日活动展开后可见（截止在后天不进今日安排；活动区默认折叠）
  await panel.getByPlaceholder("记点什么…").fill("快捷待办");
  await panel.getByRole("button", { name: "保存" }).click();
  await page.getByTestId("today-activity-fold").click();
  await expect(page.locator("main")).toContainText("快捷待办");

  // 时段相对已填开始日期：开始行点「后天」→ 点「工作时间」= 后天 09:00–18:00
  await page.getByRole("button", { name: /＋ 快速添加/ }).click();
  const evPanel = page.getByRole("dialog", { name: "新建条目" });
  await evPanel.getByRole("button", { name: "＋ 时间", exact: true }).click();
  await evPanel.getByRole("button", { name: "后天" }).click(); // 开始 = 后天 09:00
  await expect(page.getByTestId("tp-start")).toHaveText(`${dayStr(2)} 09:00`);
  await evPanel.getByRole("button", { name: "工作时间" }).click();
  await expect(page.getByTestId("tp-start")).toHaveText(`${dayStr(2)} 09:00`);
  await expect(page.getByTestId("tp-end")).toHaveText(`${dayStr(2)} 18:00`);
  await page.keyboard.press("Escape");

  // 过去时三档（记录）：普通 + 打开 → 展开详情 → 类型切记录 → 激活发生时间 → 昨天/前天
  await page.getByRole("button", { name: /＋ 快速添加/ }).click();
  const logPanel = page.getByRole("dialog", { name: "新建条目" });
  await logPanel.getByRole("button", { name: "＋ 时间 / 字段 / 标签 ▾" }).click();
  await logPanel.getByRole("combobox", { name: /类型/ }).selectOption("log");
  await logPanel.getByRole("button", { name: "＋ 发生时间" }).click();
  await page.getByRole("button", { name: "昨天" }).click();
  await expect(page.getByTestId("tp-occurred")).toContainText(dayStr(-1));
  await page.getByRole("button", { name: "前天" }).click();
  await expect(page.getByTestId("tp-occurred")).toContainText(dayStr(-2));
});

test("T8.8 时间块：预计分钟待办在周网格按时长占位，池行带预计 chip", async ({ page }) => {
  await gotoWeek(page);

  // 种子 tsk_milk：今天 18:00 截止 + 预计 45 分钟 → 网格渲染虚线时间块
  //（高度 = 45min × HOUR_H/60，±2px 几何容差）
  const block = page.locator(".event.timeblock", { hasText: "买牛奶" });
  await block.scrollIntoViewIfNeeded();
  await expect(block).toBeVisible();
  await expect(block).toContainText("18:00");
  const box = (await block.boundingBox())!;
  expect(Math.abs(box.height - (45 / 60) * HOUR_H)).toBeLessThanOrEqual(2);

  // 池行带「预计分钟」chip（拖入时段格时 ghost 按此时长占位）
  const pool = page.locator(".pool-panel");
  await expect(pool).toContainText("未排期待办");
  await expect(pool.locator("li", { hasText: "整理书架" })).toContainText("60分钟");
});

test("T10 回收站：删除入站 → 恢复 → 彻底删除，与活跃列表口径一致", async ({ page }) => {
  // 今天页两步确认删除「买牛奶」：删除即入回收站（无撤销宽限期），行即时消失
  const todayRow = () => page.locator("li", { hasText: "买牛奶" });
  await expect(todayRow()).toBeVisible();
  const del = todayRow().locator('button[title="删除"]');
  await del.click();
  await del.click(); // 「确认？」
  await expect(todayRow()).toHaveCount(0);

  // 回收站页：行可见（类型 + 删除时刻），恢复 → 回到今天页
  await navButton(page, /回收站/).click();
  const trashRow = page.getByTestId("trash-row").filter({ hasText: "买牛奶" });
  await expect(trashRow).toBeVisible();
  await trashRow.getByTestId("trash-restore").click();
  await expect(page.getByRole("status")).toContainText(/已恢复/);
  await expect(trashRow).toHaveCount(0);
  await navButton(page, /今天/).click();
  await expect(todayRow()).toBeVisible();

  // 再删 → 彻底删除（两步确认）→ 活跃与回收站都不再有
  await del.click();
  await del.click();
  await expect(todayRow()).toHaveCount(0);
  await navButton(page, /回收站/).click();
  const again = page.getByTestId("trash-row").filter({ hasText: "买牛奶" });
  await expect(again).toBeVisible();
  await again.getByTestId("trash-purge").click();
  await again.getByRole("button", { name: "确认" }).click();
  await expect(page.getByTestId("trash-empty")).toBeVisible();
  await navButton(page, /今天/).click();
  await expect(todayRow()).toHaveCount(0);
});

test("T11 重复结束条件：面板设「N 次后」保存，详情徽标带「共 N 次」", async ({ page }) => {
  await page.getByRole("button", { name: /＋ 快速添加/ }).click();
  const panel = page.getByRole("dialog", { name: "新建条目" });
  // 激活截止（识别待办）→ 重复每周…
  await panel.getByRole("button", { name: "＋ 截止" }).waitFor();
  await panel.getByPlaceholder("记点什么…").fill("重复三次的事");
  await page.getByTestId("entry-due-今天").click();
  await panel.getByRole("button", { name: "每周…" }).click();
  // 结束条件 = N 次后，3 次
  await page.getByTestId("rec-end").selectOption("count");
  const countInput = page.getByTestId("rec-end-count");
  await countInput.fill("3");
  await countInput.blur();
  await panel.getByRole("button", { name: "保存" }).click();

  // 详情徽标：重复规则带结束条件后缀
  await page.locator("main li", { hasText: "重复三次的事" }).first().click();
  await expect(page.locator(".modal")).toContainText("共 3 次");
});

test("T12 单次例外：虚拟实例详情「拆为单次」，系列其余周不变", async ({ page }) => {
  await gotoWeek(page);

  // 锚点周（本周四 14:00）：实例即系列锚点，不出现「此期操作」
  await eventBlock(page, "团队周会").click();
  await expect(page.locator(".modal")).toContainText(/重复/);
  await expect(page.getByTestId("occ-actions")).toHaveCount(0);
  await page.keyboard.press("Escape");

  // 下一周的实例 = 虚拟实例：拆为单次条目（两步确认）。→ 键盘翻周（› 被表头遮挡）
  await page.keyboard.press("ArrowRight");
  const nextBlock = eventBlock(page, "团队周会");
  await nextBlock.scrollIntoViewIfNeeded();
  await nextBlock.click();
  await expect(page.getByTestId("occ-actions")).toBeVisible();
  await page.getByTestId("occ-detach").click();
  await page.getByTestId("occ-detach").click();
  await expect(page.getByRole("status")).toContainText(/已拆为单次条目/);
  // 详情已切换到新条目：无重复行、无此期操作
  await expect(page.locator(".modal").getByText(/🔁 重复/)).toHaveCount(0);
  await expect(page.getByTestId("occ-actions")).toHaveCount(0);
  await page.keyboard.press("Escape");

  // 该周周四：系列实例被例外剔除，替代为无 🔁 的单次块
  await expect(page.locator(".event", { hasText: "团队周会" })).toHaveCount(1);
  await expect(page.locator(".event", { hasText: "团队周会" }).locator(".ev-flag", { hasText: "🔁" })).toHaveCount(0);
});

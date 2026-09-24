/**
 * FILTER-SPEC §13 验收用例（E2E，?e2e=1 内存 mock；视图行 localStorage 持久化，
 * 页面重载 = 「重开」）：
 *   V1 自建视图 → 筛选 → 保存 → 重开仍在（验收 6）
 *   V2 重置内置恢复（验收 6）
 *   V3 移除打卡卡不再出现 + 恢复（验收 6）
 *   V4 30 秒打卡卡：新建挂件 → 条件来源 → 保存（验收 2）
 *   V5 渲染器切换不改数据配置（验收 5）
 *   V6 搜索页：关键词 + 筛选共用求值路径
 *   V7 容器重命名（⋯ 菜单 → 行内输入）
 *
 * 定位约定同 flows.spec.ts：语义定位（role / testid / 文案）。
 */
import { expect, test, type Page } from "@playwright/test";

const navButton = (page: Page, name: string | RegExp) =>
  page.getByRole("navigation").getByRole("button", { name });

test.beforeEach(async ({ page }) => {
  await page.goto("/?e2e=1");
  // 每个用例独立视图状态：清掉持久化的 view_defs 后重载一次
  //（只在此清理一次；用例内的 reload 需要 localStorage 保留 = 「重开仍在」）
  await page.evaluate(() => localStorage.removeItem("myday-e2e-viewdefs"));
  await page.reload();
  await expect(page.getByRole("heading", { level: 1 })).toContainText(/今天/);
});

test("V1 待办：筛选 → chips → 定制保存 → 另存为 → 重开仍在", async ({ page }) => {
  await navButton(page, /待办/).click();
  const toolbar = page.getByTestId("view-toolbar");
  await expect(toolbar.getByRole("tab", { name: /今天 · 默认/ })).toBeVisible();
  await expect(toolbar.getByRole("tab", { name: /已完成 · 默认/ })).toBeVisible();

  // 打开筛选编辑器（共享组件，Anytype 式 popover：改动即时生效，无需「应用」）
  await toolbar.getByTestId("open-filter").click();
  const dialog = page.getByRole("dialog", { name: "筛选" });
  await expect(dialog).toBeVisible();
  // 种子条件回填：状态 = todo + 子组（anchor before tomorrow / empty）
  await expect(dialog.getByTestId("filter-rule")).toHaveCount(3);

  // 添加条件 = 先选字段（FieldMenu）→ 新行已选「优先级」，直接选值「高」
  // （顶层组与条件组各有一个「＋ 添加条件」；first() = 顶层组的）
  await dialog.getByRole("button", { name: "＋ 添加条件" }).first().click();
  await dialog.getByRole("menuitem", { name: "优先级" }).click();
  const newRow = dialog.getByTestId("filter-rule").nth(1);
  await newRow.locator("select").nth(2).selectOption({ label: "高" });

  // 即时生效：chips 出现；列表只剩高优先级（买牛奶；交周报无优先级被滤掉）
  await expect(page.getByTestId("filter-chip").filter({ hasText: "优先级" })).toBeVisible();
  await expect(page.locator(".rows li")).toHaveCount(1);
  await expect(page.locator(".rows li").first()).toContainText("买牛奶");
  await expect(page.getByText("已定制", { exact: true })).toBeVisible();

  // 完成 = 关闭面板（改动已落库）
  await dialog.getByRole("button", { name: "完成" }).click();
  await expect(dialog).toHaveCount(0);

  // 另存为用户视图
  await toolbar.getByRole("button", { name: "另存为" }).click();
  await page.getByPlaceholder("新视图名").fill("高优待办");
  await page.getByRole("button", { name: "创建副本" }).click();
  await expect(toolbar.getByRole("tab", { name: "高优待办", exact: true })).toBeVisible();
  await expect(page.locator(".rows li")).toHaveCount(1);

  // 重开（重载）仍在；切到用户视图 chips 还在
  await page.reload();
  await navButton(page, /待办/).click();
  const toolbar2 = page.getByTestId("view-toolbar");
  await expect(toolbar2.getByRole("tab", { name: "高优待办", exact: true })).toBeVisible();
  await toolbar2.getByRole("tab", { name: "高优待办", exact: true }).click();
  await expect(page.getByTestId("filter-chip").filter({ hasText: "优先级" })).toBeVisible();
  await expect(page.locator(".rows li")).toHaveCount(1);
});

test("V2 内置视图重置 = 恢复 seed（清除定制）", async ({ page }) => {
  await navButton(page, /待办/).click();
  const toolbar = page.getByTestId("view-toolbar");

  // 定制：加一条恒假条件（优先级 是 低 —— mock 无低优先级待办）
  await toolbar.getByTestId("open-filter").click();
  const dialog = page.getByRole("dialog", { name: "筛选" });
  await dialog.getByRole("button", { name: "＋ 添加条件" }).first().click();
  await dialog.getByRole("menuitem", { name: "优先级" }).click();
  const newRow = dialog.getByTestId("filter-rule").nth(1);
  await newRow.locator("select").nth(2).selectOption({ label: "低" });
  await expect(page.locator(".rows li")).toHaveCount(0);

  // 完成 → 重开面板 → 重置 → seed 恢复（seed 自身的三个条件仍显示为 chips，优先级条件消失）
  await dialog.getByRole("button", { name: "完成" }).click();
  await expect(dialog).toHaveCount(0);
  await toolbar.getByTestId("open-filter").click();
  await page.getByRole("dialog", { name: "筛选" }).getByRole("button", { name: "重置" }).click();
  await expect(page.getByRole("dialog", { name: "筛选" })).toHaveCount(0);
  await expect(page.getByTestId("filter-chip").filter({ hasText: "优先级" })).toHaveCount(0);
  await expect(page.getByTestId("filter-chip")).toHaveCount(3);
  await expect(page.getByText("已定制", { exact: true })).toHaveCount(0);
  // 交周报（@weekly:4）周四到期与买牛奶并列，first() 的排序不稳定——断言恢复即可
  await expect(page.locator(".rows li").filter({ hasText: "买牛奶" })).toHaveCount(1);
});

test("V3 编辑挂件：× 移除卡片；恢复默认统计页补回；预置容器可删可恢复", async ({ page }) => {
  await navButton(page, /统计/).click();
  const streaks = page.locator('[data-container-id="view_builtin_stats_streaks"]');
  await expect(streaks).toHaveAttribute("data-layout", "horizontal");
  await expect(streaks.getByTestId("widget").filter({ hasText: "体重" })).toBeVisible();

  // 编辑挂件模式：卡片右上角 × 移除（预置卡与用户卡同一交互）
  await streaks.getByTestId("edit-widgets").click();
  const weight = streaks.getByTestId("widget").filter({ hasText: "体重" });
  await weight.getByLabel("移除 体重").click();
  await expect(streaks.getByTestId("widget").filter({ hasText: "体重" })).toHaveCount(0);
  await streaks.getByTestId("edit-widgets").click(); // 完成

  // 重开仍在（持久化）
  await page.reload();
  await navButton(page, /统计/).click();
  await expect(
    page
      .locator('[data-container-id="view_builtin_stats_streaks"]')
      .getByTestId("widget")
      .filter({ hasText: "体重" }),
  ).toHaveCount(0);

  // 恢复默认统计页 = 完全重置（确认弹窗）：铺回三预设，体重卡回来
  page.once("dialog", (d) => d.accept());
  await page.getByTestId("restore-defaults").click();
  await expect(
    page
      .locator('[data-container-id="view_builtin_stats_streaks"]')
      .getByTestId("widget")
      .filter({ hasText: "体重" }),
  ).toBeVisible();
  await expect(page.locator("[data-container-id]")).toHaveCount(3);

  // 删除预置容器（物理删除，与用户容器同权）
  const streaks2 = page.locator('[data-container-id="view_builtin_stats_streaks"]');
  await streaks2.getByLabel("容器菜单 打卡连续").click();
  page.once("dialog", (d) => d.accept());
  await streaks2.getByRole("button", { name: "删除容器" }).click();
  await expect(page.locator('[data-container-id="view_builtin_stats_streaks"]')).toHaveCount(0);
  await expect(page.locator("[data-container-id]")).toHaveCount(2);
  // 再恢复 → 整个铺回
  page.once("dialog", (d) => d.accept());
  await page.getByTestId("restore-defaults").click();
  await expect(page.locator('[data-container-id="view_builtin_stats_streaks"]')).toBeVisible();
  await expect(
    page.locator('[data-container-id="view_builtin_stats_streaks"]').getByTestId("widget"),
  ).toHaveCount(3);

  // 热力图（纵向）与数值趋势容器都在；预置挂件 = 普通挂件
  const heat = page.locator('[data-container-id="view_builtin_stats_heatmap"]');
  await expect(heat).toHaveAttribute("data-layout", "vertical");
  await expect(heat.getByTestId("widget")).toHaveCount(1);
  await expect(page.locator('[data-container-id="view_builtin_stats_series"]')).toBeVisible();
});

test("V4 统计：＋容器 → 编辑挂件 → ＋空位（条件来源 → 保存）= 30 秒打卡卡", async ({ page }) => {
  await navButton(page, /统计/).click();

  // ＋容器（预置容器与用户容器同构）
  await page.getByTestId("add-container").click();
  await page.getByTestId("add-container-form").getByPlaceholder("新容器名").fill("跑步");
  await page.getByRole("button", { name: "创建容器" }).click();
  const container = page
    .locator('[data-testid="stats-container"]')
    .filter({ has: page.getByRole("heading", { name: /^跑步/ }) });
  await expect(container).toBeVisible();
  await expect(container).toHaveAttribute("data-layout", "vertical");

  // 编辑挂件 → ＋ 空位 → 条件来源（先选字段）：标签 含任一 运动 → 保存 → 完成
  await container.getByTestId("edit-widgets").click();
  await container.getByTestId("add-widget-tile").click();
  const editor = page.getByRole("dialog", { name: "挂件编辑" });
  await expect(editor).toBeVisible();
  await editor.locator("input").first().fill("跑步连续");
  await editor.getByRole("button", { name: "＋ 添加条件", exact: true }).click();
  await editor.getByRole("menuitem", { name: "标签" }).click();
  const row = editor.getByTestId("filter-rule").last();
  await row.getByPlaceholder("输入标签，回车添加").fill("运动");
  await row.getByPlaceholder("输入标签，回车添加").press("Enter");
  await editor.getByRole("button", { name: "保存" }).click();
  await expect(editor).toHaveCount(0);
  await container.getByTestId("edit-widgets").click(); // 完成

  await expect(container.getByTestId("widget").filter({ hasText: "跑步连续" })).toContainText("连续 0 天");

  // 重开仍在
  await page.reload();
  await navButton(page, /统计/).click();
  const container2 = page
    .locator('[data-testid="stats-container"]')
    .filter({ has: page.getByRole("heading", { name: /^跑步/ }) });
  await expect(container2.getByTestId("widget").filter({ hasText: "跑步连续" })).toBeVisible();
});

test("V5 编辑模式点卡换图型：数据集不动；容器布局可切换", async ({ page }) => {
  await navButton(page, /统计/).click();

  // 建容器 + 挂件（条件：标签 含任一 运动）
  await page.getByTestId("add-container").click();
  await page.getByTestId("add-container-form").getByPlaceholder("新容器名").fill("跑步");
  await page.getByRole("button", { name: "创建容器" }).click();
  const container = page
    .locator('[data-testid="stats-container"]')
    .filter({ has: page.getByRole("heading", { name: /^跑步/ }) });
  await container.getByTestId("edit-widgets").click();
  await container.getByTestId("add-widget-tile").click();
  const editor = page.getByRole("dialog", { name: "挂件编辑" });
  await editor.locator("input").first().fill("跑步连续");
  await editor.getByRole("button", { name: "＋ 添加条件", exact: true }).click();
  await editor.getByRole("menuitem", { name: "标签" }).click();
  const row = editor.getByTestId("filter-rule").last();
  await row.getByPlaceholder("输入标签，回车添加").fill("运动");
  await row.getByPlaceholder("输入标签，回车添加").press("Enter");
  await editor.getByRole("button", { name: "保存" }).click();
  await expect(editor).toHaveCount(0);

  // 仍在编辑模式：点卡片主体 → 编辑器 → 换图型为柱状 → 保存（数据集不动）
  const card = () => container.getByTestId("widget").filter({ hasText: "跑步连续" });
  await card().click();
  const editor2 = page.getByRole("dialog", { name: "挂件编辑" });
  await editor2.getByRole("button", { name: "柱状" }).click();
  await editor2.getByRole("button", { name: "保存" }).click();
  await expect(editor2).toHaveCount(0);

  // 柱状渲染出现；条件来源不变：再点卡验证规则行还在（标签 chip 输入保留已有标签）
  await expect(card().locator(".bar-row").first()).toBeVisible();
  await card().click();
  const editor3 = page.getByRole("dialog", { name: "挂件编辑" });
  const rule = editor3.getByTestId("filter-rule").last();
  await expect(rule.locator("select").nth(0)).toHaveValue("col:tags");
  await expect(rule.locator(".tag").filter({ hasText: "运动" })).toBeVisible();
  await expect(editor3.getByRole("button", { name: "柱状" })).toHaveClass(/active/);
  await editor3.getByRole("button", { name: "取消" }).click();
  await container.getByTestId("edit-widgets").click(); // 完成

  // 容器布局切换：垂直 → 水平（同一容器、挂件不动）
  await container.getByLabel("容器菜单 跑步").click();
  await container.getByRole("button", { name: "切换为水平布局" }).click();
  await expect(container).toHaveAttribute("data-layout", "horizontal");
  await expect(card()).toBeVisible();
});

test("V6 搜索页：关键词 + 筛选（类型）共用一条求值路径", async ({ page }) => {
  await navButton(page, /搜索/).click();
  await page.locator("#global-search").fill("周报");
  await page.locator("#global-search").press("Enter");
  await expect(page.locator(".rows li")).toHaveCount(1);
  await expect(page.locator(".rows li").first()).toContainText("交周报");

  // 加类型筛选（先选字段）：类型 是 日程 → 交周报（待办）被滤掉（即时生效）
  const toolbar = page.getByTestId("view-toolbar");
  await toolbar.getByTestId("open-filter").click();
  const dialog = page.getByRole("dialog", { name: "筛选" });
  await dialog.getByRole("button", { name: "＋ 添加条件", exact: true }).click();
  await dialog.getByRole("menuitem", { name: "类型" }).click();
  const newRow = dialog.getByTestId("filter-rule").last();
  await newRow.locator("select").nth(2).selectOption({ label: "日程" });
  await expect(page.locator(".rows li")).toHaveCount(0);
  await expect(page.getByText("没有匹配结果")).toBeVisible();
});

test("V7 统计：容器重命名（⋯ 菜单 → 行内输入 → 重开仍在）", async ({ page }) => {
  await navButton(page, /统计/).click();
  const streaks = page.locator('[data-container-id="view_builtin_stats_streaks"]');

  // ⋯ 菜单 → 重命名 → 行内输入 → Enter
  await streaks.getByLabel("容器菜单 打卡连续").click();
  await streaks.getByRole("button", { name: "重命名" }).click();
  const input = streaks.getByTestId("rename-input");
  await expect(input).toBeVisible();
  await input.fill("坚持打卡");
  await input.press("Enter");
  await expect(streaks.getByRole("heading", { name: /坚持打卡/ })).toBeVisible();

  // 重开仍在
  await page.reload();
  await navButton(page, /统计/).click();
  await expect(
    page
      .locator('[data-container-id="view_builtin_stats_streaks"]')
      .getByRole("heading", { name: /坚持打卡/ }),
  ).toBeVisible();
});

test("V8 待办：无日期内置视图 = 未排期池同口径，批量条无「改期」", async ({ page }) => {
  await navButton(page, /待办/).click();
  const toolbar = page.getByTestId("view-toolbar");
  await toolbar.getByRole("tab", { name: /无日期 · 默认/ }).click();

  // 只剩无 due 无 start 的种子待办（其余都有 due），FIFO 即创建序
  const rows = page.locator(".rows li");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("整理书架");
  await expect(rows.first()).not.toContainText("截止");

  // 多选批量条仍在（完成 / 删除 / 全选），「改期」不出现——全部无 due，平移无意义
  await page.getByRole("button", { name: "多选", exact: true }).click();
  await rows.first().locator('input[type="checkbox"]').click();
  const bar = page.locator(".batch-bar");
  await expect(bar).toContainText("完成");
  await expect(bar).toContainText("删除");
  await expect(bar).not.toContainText("改期");

  // 批量完成跑通：行完成即离开无日期视图（status=done 不满足过滤）
  await bar.getByRole("button", { name: "完成", exact: true }).click();
  await expect(page.getByRole("status")).toContainText(/已完成 1 条/);
  await expect(page.locator(".rows li")).toHaveCount(0);
});

test("V9 标签过滤：今天页与日历按标签收窄，全部 = 原样", async ({ page }) => {
  // 今天页：无标签的「买牛奶」在 #工作 下消失，带标签行保留
  const sel = page.getByTestId("today-tag-filter");
  await expect(sel).toBeVisible();
  await sel.selectOption("工作");
  await expect(page.locator("li", { hasText: "买牛奶" })).toHaveCount(0);
  await expect(page.locator("li", { hasText: "产品评审" })).toBeVisible();
  await sel.selectOption("");
  await expect(page.locator("li", { hasText: "买牛奶" })).toBeVisible();

  // 日历月视图：#运动 → 网格只剩晨跑 chip，当天面板的到期行也同步隐藏
  await page.getByRole("navigation").getByRole("button", { name: /日历/ }).click();
  const cal = page.getByTestId("cal-tag-filter");
  await expect(cal).toBeVisible();
  await page.locator(".month").getByRole("button", { name: "今天" }).click(); // 选中今天，面板显示当天
  await cal.selectOption("运动");
  await expect(page.locator(".cell-ev", { hasText: "晨跑" }).first()).toBeVisible();
  await expect(page.locator(".cell-ev", { hasText: "买牛奶" })).toHaveCount(0);
  await expect(page.locator(".day-panel li", { hasText: "买牛奶" })).toHaveCount(0);
  await cal.selectOption("");
  // 复原：当天面板到期节重新出现（面板行无月格 chip 预算截断，断言确定）
  await expect(page.locator(".day-panel li", { hasText: "买牛奶" })).toBeVisible();
});

test("V10 即时搜索：Ctrl+K 与搜索页输入即出 + 命中高亮（不回车）", async ({ page }) => {
  // Ctrl+K 命令面板：输入即出结果，命中段高亮
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "命令面板" });
  await palette.getByRole("textbox").fill("牛奶");
  await expect(palette.getByText("买牛奶")).toBeVisible();
  await expect(palette.locator("mark")).toContainText("牛奶");
  await page.keyboard.press("Escape");

  // 搜索页：输入即出（无回车），标题命中段 <mark> 高亮
  await page.getByRole("navigation").getByRole("button", { name: /搜索/ }).click();
  await page.locator("#global-search").fill("周报");
  await expect(page.getByTestId("search-hit-title").filter({ hasText: "周报" }).first()).toBeVisible();
  await expect(page.locator("mark").first()).toContainText("周报");
});

test("V11 今天页：日程/待办分组，今日活动默认折叠", async ({ page }) => {
  // 分组小标题：日程组恒有 晨跑（每天）/ 产品评审 / 临时插会（周四再展开团队周会，
  // 故不硬编码总数）；待办组 = 今天到期的「买牛奶」（带勾选框）——两类不再混排
  await expect(page.getByRole("heading", { name: "今日安排" })).toBeVisible();
  const evRows = page.locator("h3:has-text('日程') + ul.rows li");
  await expect(evRows.filter({ hasText: "产品评审" })).toBeVisible();
  expect(await evRows.count()).toBeGreaterThanOrEqual(3);
  const taskRows = page.locator("h3:has-text('待办') + ul.rows li");
  await expect(taskRows.filter({ hasText: "买牛奶" })).toBeVisible();
  await expect(taskRows.filter({ hasText: "买牛奶" }).locator('input[type="checkbox"]')).toBeVisible();

  // 今日活动默认折叠：标题（带计数）可见、列表不渲染；点开即出行
  //（种子活动与安排去重后仍有其他今日改动条目，恒非空）
  const fold = page.getByTestId("today-activity-fold");
  const actSection = page.locator("section", { has: fold });
  await expect(fold).toContainText("今日活动");
  await expect(fold).toHaveAttribute("aria-expanded", "false");
  await expect(actSection.locator("li")).toHaveCount(0);
  await fold.click();
  await expect(fold).toHaveAttribute("aria-expanded", "true");
  await expect(actSection.locator("li").first()).toBeVisible();
});

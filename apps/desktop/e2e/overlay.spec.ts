/**
 * 今日悬浮窗 E2E（OVERLAY-SPEC §10，?e2e=1&window=overlay 内存 mock；
 * 真实窗口几何 / 穿透不可脚本化，以状态断言代替——custom_pos / locked 走
 * settings KV，透明度走 CSS 变量）：
 *   O1 渲染：日程/待办分区、头部进度、进行中高亮、摘要行省略
 *   O2 勾选完成：淡出重排、进度更新、摘要计数随 data-changed 变化
 *   O3 设置面板：透明度 CSS 变量 + 持久化、角落重置清除 custom_pos、锁定持久化
 *
 * 种子里「团队周会 / 交周报」锚在每周四（@weekly:4）：周四进今日列表；
 * 周五~周日本周四已过 → 种子自带「逾期 1」；周一~周三本周四在未来 →
 * 今日列表与摘要都不出现。种子另有一条无日期待办（整理书架）恒进摘要
 * 「未安排 1」不进列表。计数按 seedReportState 分支断言。
 * mock 的 settings KV 每次页面加载重置，持久化断言以 get_overlay_config 为准。
 */
import { expect, test, type Page } from "@playwright/test";

/** 与 e2e-mock 同款「本周四」计算：种子两条 @weekly:4 条目是否落在今天 */
function seedThursday(): boolean {
  return seedReportState() === "today";
}

/**
 * 种子交周报 / 团队周会锚定本周四 18:00：周四进今日列表；周五~周日本周四已过
 * = 逾期 1；周一~周三本周四在未来 = 今日列表与摘要都不出现（不算逾期）。
 */
function seedReportState(): "today" | "overdue" | "future" {
  const day = new Date().getDay(); // 0 = 周日
  if (day === 4) return "today";
  if (day === 0 || day >= 5) return "overdue";
  return "future";
}

async function invoke(page: Page, cmd: string, args: Record<string, unknown>) {
  return page.evaluate(
    ([cmd, args]) =>
      (
        window as unknown as {
          __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> };
        }
      ).__TAURI_INTERNALS__.invoke(cmd, args),
    [cmd, args],
  );
}

test.beforeEach(async ({ page }) => {
  await page.goto("/?e2e=1&window=overlay");
  await expect(page.getByTestId("overlay-root")).toBeVisible();
});

test("O1 渲染：分区 + 进度 + 摘要省略 + 进行中高亮", async ({ page }) => {
  const thursday = seedThursday();
  // 种子今天恒有的日程：晨跑 7:00 / 产品评审 10:00 / 临时插会 10:30（周会仅周四）
  await expect(page.getByTestId("overlay-event")).toHaveCount(3 + (thursday ? 1 : 0));
  await expect(page.getByTestId("overlay-event").filter({ hasText: "晨跑" })).toContainText("07:00");

  // 待办：买牛奶恒有；交周报仅周四
  await expect(page.getByTestId("overlay-task").filter({ hasText: "买牛奶" })).toBeVisible();
  await expect(page.getByTestId("overlay-task")).toHaveCount(1 + (thursday ? 1 : 0));
  // 头部进度 = 0 / 今日待办数（§4.4 不含逾期未安排）
  await expect(page.getByTestId("overlay-progress")).toHaveText(`✓ 0/${1 + (thursday ? 1 : 0)}`);

  // 摘要行（§4.3）：种子自带一条无日期待办（整理书架，进摘要不进列表）→「未安排 1」
  // 恒在；周五~周日交周报（锚本周四）转为逾期叠加；周一~周三本周四在未来，
  // 既非今日也非逾期。列表非空 → 无空态。
  if (seedReportState() === "overdue") {
    await expect(page.getByTestId("overlay-summary")).toHaveText("逾期 1 · 未安排 1");
  } else {
    await expect(page.getByTestId("overlay-summary")).toHaveText("未安排 1");
  }
  await expect(page.getByTestId("overlay-empty")).toHaveCount(0);

  // 进行中高亮：起点 = 30 秒前（恒早于 mock 冻结的求值时刻；钳到当天零点，
  // 午夜后运行也不跨日被「归属按开始」滤掉）
  await invoke(page, "add_item", {
    new: {
      item_type: "event",
      title: "正好进行",
      start_at: new Date(
        Math.max(Date.now() - 30_000, new Date().setHours(0, 0, 0, 0)),
      ).toISOString(),
      end_at: new Date(Date.now() + 3_600_000).toISOString(),
    },
  });
  await expect(page.getByTestId("overlay-event").filter({ hasText: "正好进行" })).toHaveClass(/active/);
});

test("O2 勾选完成：淡出重排 + 进度更新 + 摘要计数联动", async ({ page }) => {
  // 造逾期与未安排各一条（列表不出现，只进摘要 §4.1）
  const overdue = (await invoke(page, "add_item", {
    new: { item_type: "task", title: "逾期账", due_at: new Date(Date.now() - 86_400_000).toISOString() },
  })) as { id: string };
  const unsched = (await invoke(page, "add_item", {
    new: { item_type: "task", title: "无安排" },
  })) as { id: string };
  // 非周四且本周四已过（周五~周日）时种子自带交周报逾期 1，叠加后按状态分支；
  // 种子另有一条无日期待办（整理书架），与本次添加的合计未安排 2
  const seedOverdue = seedReportState() === "overdue" ? 1 : 0;
  const seedUnsched = 1;
  await expect(page.getByTestId("overlay-summary")).toHaveText(
    seedOverdue
      ? `逾期 ${seedOverdue + 1} · 未安排 ${seedUnsched + 1}`
      : `逾期 1 · 未安排 ${seedUnsched + 1}`,
  );

  // 勾选买牛奶：300ms 淡出 → complete_task → data-changed → 列表重排、进度 +1
  const milk = page.getByTestId("overlay-task").filter({ hasText: "买牛奶" });
  await milk.locator('input[type="checkbox"]').check();
  await expect(milk).toHaveCount(0);
  await expect(page.getByTestId("overlay-progress")).toHaveText(`✓ 1/${1 + (seedThursday() ? 1 : 0)}`);

  // 摘要计数随 data-changed 联动：完成后剩种子那条未排期待办，「未安排 1」不再省略
  await invoke(page, "complete_task", { id: unsched.id });
  await expect(page.getByTestId("overlay-summary")).toHaveText(
    seedOverdue
      ? `逾期 ${seedOverdue + 1} · 未安排 ${seedUnsched}`
      : `逾期 1 · 未安排 ${seedUnsched}`,
  );
  void overdue;
});

test("O3 设置面板：透明度变量、角落重置、锁定持久化", async ({ page }) => {
  await page.getByRole("button", { name: "悬浮窗设置" }).click();
  const panel = page.getByTestId("overlay-settings");
  await expect(panel).toBeVisible();

  // 透明度：实时 CSS 变量 + 变更即存（§5.5）
  await page.getByTestId("overlay-opacity").fill("0.5");
  await expect(page.getByTestId("overlay-root")).toHaveAttribute("style", /--op:\s*0\.5/);
  let cfg = (await invoke(page, "get_overlay_config", {})) as { opacity: number };
  expect(cfg.opacity).toBe(0.5);

  // 角落重置：清除 custom_pos 重新吸附（§3）；尺寸不受影响（v1.1）
  await invoke(page, "set_overlay_config", {
    config: {
      enabled: true,
      corner: "tr",
      custom_pos: { x: 100, y: 100 },
      size: { w: 320, h: 420 },
      opacity: 0.5,
      locked: false,
    },
  });
  await panel.getByRole("button", { name: "左上" }).click();
  cfg = (await invoke(page, "get_overlay_config", {})) as {
    corner: string;
    custom_pos: { x: number } | null;
    size: { w: number; h: number } | null;
  };
  expect(cfg.corner).toBe("tl");
  expect(cfg.custom_pos).toBeNull();
  expect(cfg.size).toEqual({ w: 320, h: 420 });

  // 锁定：开关状态写入配置（真实窗口的穿透行为不可脚本化，见 OVERLAY-SPEC §10）
  await page.getByTestId("overlay-locked").check();
  cfg = (await invoke(page, "get_overlay_config", {})) as { locked: boolean };
  expect(cfg.locked).toBe(true);
});

/** 整体写配置（补全所有字段：overlay-config 广播会整体替换悬浮窗 cfg） */
async function setOverlay(page: Page, patch: Record<string, unknown>) {
  await invoke(page, "set_overlay_config", {
    config: {
      enabled: true,
      corner: "tr",
      custom_pos: null,
      size: null,
      opacity: 0.9,
      locked: false,
      show: "all",
      expand_summary: false,
      ...patch,
    },
  });
}

test("O4 显示分档 + 折叠摘要 + 展开逾期未安排（v1.2 §4.1/§4.3）", async ({ page }) => {
  const report = seedReportState();
  const eventsN = 3 + (report === "today" ? 1 : 0); // 晨跑/评审/插会 +（周四）团队周会
  const tasksN = 1 + (report === "today" ? 1 : 0); // 买牛奶 +（周四）交周报
  // 折叠摘要中随种子星期变化的段：逾期仅在周五~周日（本周四已过）出现
  const seedOverdue = report === "overdue" ? " · 逾期 1" : "";

  // 待办档：日程区隐藏并折叠成「日程 N」，进度仍只算今日待办（§4.4）
  await setOverlay(page, { show: "tasks" });
  await expect(page.getByTestId("overlay-event")).toHaveCount(0);
  await expect(page.getByTestId("overlay-task")).toHaveCount(tasksN);
  await expect(page.getByTestId("overlay-progress")).toHaveText(`✓ 0/${tasksN}`);
  await expect(page.getByTestId("overlay-summary")).toHaveText(
    `日程 ${eventsN}${seedOverdue} · 未安排 1`,
  );

  // 日程档：待办区（含逾期/未安排的展开区）整体隐藏，折叠成「待办 N」；进度隐藏
  await setOverlay(page, { show: "events" });
  await expect(page.getByTestId("overlay-task")).toHaveCount(0);
  await expect(page.getByTestId("overlay-progress")).toHaveCount(0);
  await expect(page.getByTestId("overlay-event")).toHaveCount(eventsN);
  await expect(page.getByTestId("overlay-summary")).toHaveText(
    `待办 ${tasksN}${seedOverdue} · 未安排 1`,
  );

  // 全部档 + 展开：逾期/未安排成列（与今日待办同款可勾选），摘要行随之消失
  await setOverlay(page, { show: "all", expand_summary: true });
  await invoke(page, "add_item", {
    new: { item_type: "task", title: "逾期账", due_at: new Date(Date.now() - 86_400_000).toISOString() },
  });
  await expect(page.getByTestId("overlay-overdue")).toHaveCount(1);
  await expect(page.getByTestId("overlay-overdue").filter({ hasText: "逾期账" })).toBeVisible();
  await expect(page.getByTestId("overlay-unscheduled").filter({ hasText: "整理书架" })).toBeVisible();
  await expect(page.getByTestId("overlay-summary")).toHaveCount(0);

  // ⚙ 面板分档按钮 / 展开开关即时生效并落库
  await page.getByRole("button", { name: "悬浮窗设置" }).click();
  await page.getByTestId("overlay-show-tasks").click();
  expect(((await invoke(page, "get_overlay_config", {})) as { show: string }).show).toBe("tasks");
  await expect(page.getByTestId("overlay-event")).toHaveCount(0);
  await page.getByTestId("overlay-expand").uncheck();
  const final = (await invoke(page, "get_overlay_config", {})) as { expand_summary: boolean };
  expect(final.expand_summary).toBe(false);
});

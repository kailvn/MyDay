# E2E 与交互调试

两条互补路径,都跑**真实渲染**的前端(`?e2e=1` 内存 mock 替代 Tauri 运行时,见 `main.ts` 与 `src/lib/e2e-mock.ts`):

## 1. 脚本回归

### 单元 golden(纯逻辑,秒级)

```bash
cd apps/desktop
pnpm test    # node:test 直跑 TS:holidays / recurrence / timewords / tpltime 四组 golden
```

Rust 侧对应用例:`cargo test`(crates/myday-core 等)。

### 端到端(Playwright)

```bash
cd apps/desktop
pnpm e2e            # headless 全量;webServer 自动起 vite dev(1420 已有 dev server 则复用)
pnpm e2e --ui       # 可视化调试:点选用例、看时间线/快照
pnpm e2e -- -g T9   # 只跑名字匹配的用例
```

- 用例:`e2e/flows.spec.ts`,覆盖 T1–T8 主流程(视图切换、提醒中心、类型转换、统计范围、备份、节假日导入、周视图拖拽)。
- 失败自动留截图 + trace:`apps/desktop/test-results/`(已 gitignore),`pnpm exec playwright show-trace <zip>` 回放。
- 全程真 Chromium、真实几何:拖拽/重叠布局/滚动都不需要垫片,5~6 秒跑完。

### 定位约定(防"改布局就失效"的核心)

- 交互目标一律**语义定位**:`getByRole("button", { name })`(可访问名 = 可见文案或 aria-label)、`getByRole("dialog", { name })`、`getByRole("textbox")`、`getByTestId`。改样式、调布局不影响。
- 只有**拖拽几何换算**允许碰结构类(`.day-col` 等,它们是布局骨架本身);网格参数与源码耦合:`HOUR_H=44`、吸附 15 分钟,改网格密度需同步 `e2e/flows.spec.ts`。
- 新控件建议顺手补 `aria-label` / `data-testid`(如 WeekGrid 网格的 `data-testid="week-grid"`),既是可访问性也是测试锚点。

### 版本策略

`@playwright/test` **精确锁版**(当前 1.57.0 ↔ Chromium build 1200),这是 Playwright 官方推荐做法。升级 = 改 `package.json` 版本号 + `pnpm exec playwright install chromium`(换机器/Windows 同理,浏览器走官方 CDN 自动下载)。

## 2. 交互式调试(agent 实机点/截图)

让 AI 助手直接操作界面排查问题:

```bash
cd apps/desktop && pnpm dev   # 起在 http://localhost:1420
```

然后让助手打开 `http://localhost:1420/?e2e=1`,即可点击、填表、读无障碍树(ARIA 快照)、截图核对视觉。比脚本灵活:适合复现 bug、验证改版效果、检查样式。

## 真窗口测试(可选,暂缓)

上述都走浏览器 mock,不碰真实 Tauri IPC。若将来要测真窗口:Windows 上 WebView2 支持 CDP(`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`,Playwright `connectOverCDP` 直连);Linux 上 WebKitGTK 需 `tauri-driver`(WebDriver 协议,客户端得换 webdriverio)。建议只给少数 IPC 冒烟用例用,主流程留在浏览器模式。

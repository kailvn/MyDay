# THEME-SPEC：界面主题（配色 × 形态）

> 状态：已落地（2026-09-29）。设置页「通用」区切换；e2e 用例 `theme.spec.ts`。

## 0. 一句话定位

配色（palette）与形态（mode，亮/暗/跟随系统）两轴正交组合：N 套配色 × 2 形态 = 2N 种外观，只需维护 N 个配色块对；组件层只认语义令牌，永不感知具体配色。

## 1. 分层

```
themes.css   令牌层：全应用唯一取色处（:root 基座 + 共享暗色 + 配色块对）
app.css      @import themes.css + 全局元素样式（不含颜色）
组件 <style> 只用 var(--语义令牌) 与 color-mix 派生，禁止 hex
theme.svelte.ts  状态与持久化：写 <html data-theme/data-mode>，不取色
```

两个入口（桌面 `main.ts`、移动 `mobile/main.ts`）都引 `app.css` → 令牌层单点生效；桌面/移动共用同一套令牌。

## 2. 令牌词汇表

| 类别 | 令牌 | 说明 |
|---|---|---|
| 纸色家族 | `--bg` `--sidebar-bg` `--card` `--border` | 每套配色的主体差异所在 |
| 墨色 | `--text` `--text-dim` | 随配色（明暗两套） |
| 功能色 | `--accent` `--accent-fg` `--danger` | 基座持有，全配色共用；`--accent-fg` = 饱和底色上的文字 |
| 类型色 | `--type-event` `--type-task` `--type-log` `--type-hol` `--type-work` | 日程蓝 / 待办橙 / 记录·完成·休绿 / 节假日琥珀 / 班·加班红；暗色提亮在共享暗色块 |
| 时刻 | `--now` | 周视图当前时刻线 |
| 警示 | `--warn-border` `--warn-bg` `--warn-fg` | 横幅三件套 |
| 形态 | `--radius` | 与配色无关 |

## 3. themes.css 覆盖规则（自上而下，后者赢前者）

1. `:root` 基座 —— 形态令牌 + 功能色/类型色缺省 + 纸黄亮色（= 缺省配色，`data-theme` 缺省时兜底）。
2. `[data-mode='dark']` —— 跨配色共享的暗色提亮（类型色/警示色 + `color-scheme: dark`）。
3. 配色块 —— `[data-theme=…]`（亮）与 `[data-theme=…][data-mode='dark']`（暗），只写纸色家族 + 墨色六个变量。

**新增一套配色**：在 themes.css 照抄一个块对（亮 6~8 行 + 暗 6~8 行），再在 `theme.svelte.ts` 的 `PALETTES`/`PALETTE_SWATCH` 各加一行、两个入口 HTML 头部内联脚本的取值域数组加一个 id、i18n 补 `settings.palette.<id>` 中英两条。组件层零改动。

## 4. 形态轴与 mode 解析

- `theme = $state({ palette, mode })`（runes 单例，复刻 i18n 模式）；`setPalette` / `setMode` 立即 apply + 异步持久化，失败不阻断界面。
- `mode='system'` 由 `matchMedia('(prefers-color-scheme: dark)')` 实时解析并监听翻转；**CSS 永远只见 light|dark**，不再使用 `prefers-color-scheme` 媒体查询。
- 模块加载即同步写一次 `<html>` 属性（防首帧闪色）；`initTheme({load, save})` 在 mount 前读设置精化。
- 设置键：`ui_theme`（palette id）、`ui_theme_mode`（`system|light|dark`）——settings KV 表，无 schema 迁移。
- 跨窗口：保存成功后经 Tauri 全局事件 `myday://theme` 广播，副窗口（overlay / quick-add / alarm）监听跟随（只改状态不回写，避免环）；无 Tauri 事件环境（e2e/浏览器）静默降级单窗口。
- 防启动闪色（三层）：① 切换时镜像到 `localStorage["myday.theme"]`，两个入口 HTML 头部内联脚本在**绘制前**按镜像恢复 `<html>` 属性；② 主窗口 `visible: false`（tauri.conf.json），`main.ts` 等 `initTheme` 权威精化 + mount 完成后才 `show()` 揭幕——原生层黑帧 / webview 白帧 / 错色首帧一并消除（托盘「显示主窗口」/ 二次启动唤起走 Rust `show_main`，不受影响；quick-add / overlay / alarm 显隐仍由 Rust 驱动）；③ SQLite 仍为权威——`initTheme` 异步精化后回写镜像。升级后首次启动镜像为空，仍有「揭幕前等待 + 一次纠偏」，但可见帧始终正确。

## 5. 硬编码纪律

- 组件 `<style>` 一律用令牌；新增颜色先问「该进词汇表吗」，再问「进基座还是配色块」。
- 已知例外：SettingsView `.qr` 纯白底（扫码对比度，功能白，非主题色）。
- 派生写法沿用既有惯法：`color-mix(in srgb, var(--…) N%, transparent)`。
- 设置页色板圆点 = 亮/暗形态对半拼预览，色值取自 `PALETTE_SWATCH` 的 bg/darkBg（仅展示与揭幕底色；真实取色仍只在 themes.css）。

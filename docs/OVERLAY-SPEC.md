# MyDay 今日悬浮窗（OVERLAY-SPEC v1.1）

> 状态：P1 已实施（2026-09-18）。实现对照：`myday-core::overlay`（§4 求值 + 单测）、
> `apps/desktop/src-tauri/src/platform.rs`（§2.3 注入）、`commands.rs` overlay_* 命令组
> （§6/§7）、`lib.rs`（托盘 / 启动恢复 / `--overlay`）、`lib/OverlayWindow.svelte`（§3/§5 前端）。
> v1.1（2026-09-18，评审后放开）：窗口尺寸开放自由调整——默认 264×380 不变，拖边/角
> 调整后存 `overlay.size`（§3/§6），最小 220×280。
> 需求来源：用户需求「把当天的待办做成小窗口一直置顶，透明度可调，置顶后可锁定，
> 放右上/左上角，不影响保存文档或点叉叉退出；简洁、常驻、不影响其他工作但可以提醒，
> 只展示今日未完成的日程和待办」。
> v1 评审澄清：逾期和无 due 都只给一行统计摘要（如「逾期 2 · 未安排 3」），不展开
> 列表；悬浮窗应可拖动调整位置；窗口内不需要编辑。
> 平台结论已定：维持 Tauri 窗口技术栈；Linux Wayland 会话下整进程注入 `GDK_BACKEND=x11`
> 走 XWayland（方案 A）。技术选型过程与备选否决理由见 §2，此处自包含。
> 建议在《需求》v3.0 §七（桌面集成）下增补对应小节，编号顺延。

## 0. 设计原则

1. **不挡事优先于好看**：悬浮窗的一切默认值（位置、尺寸、透明度、锁定态）以
   「永不遮挡用户正在操作的界面关键区域、永不抢焦点、永不拦截鼠标」为准绳。
2. **常驻 = 环境提醒**：悬浮窗本身是「一直看得见」的提醒通道；主动提醒（到期通知、
   操作按钮）仍走既有 reminder_loop + 系统通知路径（需求 §9），不重复造提醒逻辑。
3. **简洁 = 只读 + 一个动作**：悬浮窗内唯一高频操作是「勾选完成」。编辑、增删、
   过滤全部留在主窗口，点击条目跳主窗口（P2 定位到条目）。
4. **确定性**：「今日未完成」的求值语义全部可解释（本地日、逾期边界、无 due 排除），
   见 §4；不静默猜测。
5. **零新增概念**：数据、同步、持久化、命令全部复用现有机制（`data-changed` 广播、
   settings KV、commands.rs 模式），悬浮窗不引入第二条数据管线。

## 1. 目标与非目标

**目标（P1）**

- 第三个 Tauri 窗口（label `overlay`）：无边框、透明、置顶、不进任务栏、不抢焦点。
- 只读展示今日未完成的日程与待办（定义见 §4），支持勾选完成。
- 底部一行统计摘要：逾期、未安排（无 due）只计数不成列。
- 置顶 / 锁定（点击穿透）两态开关；锁定后鼠标事件完全落到下层窗口。
- 透明度滑杆（30%–100%）；头部拖动定位、位置持久化，角落吸附重置。
- 尺寸自由调整（v1.1）：默认 264×380 不变，拖边 / 角调整后存 `overlay.size`，
  最小 220×280；内容区（待办滚动、正文字号）随窗高自适应。
- 托盘菜单开关；开启后随应用启动常驻。
- Windows / Linux(X11) / Linux(Wayland·XWayland) 三路径行为一致。

**非目标（明确不做）**

- 不做悬浮窗内条目编辑 / 新增 / 排序——窗口只读，唯一动作是勾选完成
  （用户明确：可拖动定位，但不需要编辑）。
- 不做拖动的边界吸附 / 磁吸（P2 评估）。
- 不做独立进程或非 Tauri 渲染栈（§2 决策 D1/D3）。
- 不做 hover 自动穿透切换的隐式交互（P1 锁定/解锁一律显式开关；§5.3）。
- 不做悬浮窗独享的通知/弹窗（提醒仍走系统通知）。

## 2. 平台策略（决策记录）

### 2.1 能力矩阵（实测结论，tao 0.35.3 / Tauri 2.11）

| 能力 | Windows | Linux X11（含 XWayland） | Linux Wayland 原生 |
|---|---|---|---|
| 无边框 + 透明 | ✅ | ✅ | ✅（quick-add 已验证） |
| 置顶 `set_always_on_top` | ✅ | ✅（主流 WM 均尊重） | ❌ 协议禁止（tao 标注 Unsupported） |
| 点击穿透 `set_ignore_cursor_events` | ✅ WS_EX_TRANSPARENT | ✅ XShape | ✅ GTK `input_shape_combine_region`（tao event_loop.rs:452，Wayland 协议内合法） |
| 自定位 `set_position` | ✅ | ✅ | ❌ `gtk_window_move` 为 no-op |
| skipTaskbar | ✅ | ✅ | 基本可用 |

关键事实：置顶与自定位是 **Wayland 协议级**限制，与 UI 框架无关——winit
`set_window_level(AlwaysOnTop)` 在 Wayland 下同样是静默 no-op。协议内唯一的原生
悬浮层机制是 wlr-layer-shell 扩展，GNOME/Mutter 至今不支持（gnome-shell issue #1141
多年未决），KWin、Sway、Hyprland 等 支持。

### 2.2 决策

| # | 决策 | 否决的备选 | 否决理由 |
|---|---|---|---|
| D1 | **维持 Tauri 窗口，Wayland 会话整进程注入 `GDK_BACKEND=x11` 走 XWayland** | 换纯 Rust 栈（winit/iced/egui） | 撞同一堵协议墙（§2.1），还得养第二套 UI 栈、进程间数据同步，收益为零 |
| D2 | 注入点在 `run()` 内、`tauri::Builder` 构建之前（GTK 初始化读该变量）；仅当会话为 Wayland **且** 用户未显式设置 `GDK_BACKEND` 时注入；`MYDAY_BACKEND=wayland` 可强制原生（降级：无置顶/定位，设置页显示兼容性提示） | per-window 后端 | GDK 后端是进程级选择，无窗口级 API |
| D3 | 悬浮窗与主应用同进程（第三个窗口） | 独立 helper 进程走 X11、主进程留原生 Wayland | P1 不值得多一个进程 + 一条 IPC；XWayland 分数缩放模糊若有实际反馈再拆（§12 风险 R1） |
| D4 | 置顶/穿透等窗口控制全部走自定义 Rust 命令 | 走 JS API + capabilities 授权 | 沿现有 commands.rs 模式，免维护 capability 清单 |
| D5 | 不采用 layer-shell / GNOME Shell 扩展 | 原生 Wayland 悬浮层 | layer-shell GNOME 不支持（目标用户环境即 Ubuntu GNOME）；Shell 扩展是 JS 且 per-DE，违背双平台一致性 |

### 2.3 注入实现要点

```
pub fn run() {
    // 必须在第一次 GTK 初始化（Builder::run/build）之前
    myday_desktop_lib::platform::force_x11_on_wayland();
    let store = Store::open_default()...;
    tauri::Builder::default()...
}
```

判定顺序：环境变量 `MYDAY_BACKEND` 显式指定 > 已有 `GDK_BACKEND` 尊重不覆盖 >
`WAYLAND_DISPLAY` 或 `XDG_SESSION_TYPE=wayland` 成立时设 `GDK_BACKEND=x11` >
其余不动。注入与否与结果写一行日志，便于排查「为什么不置顶」。

## 3. 窗口规格

沿 quick-add 的声明模式（`tauri.conf.json` → `app.windows`），新增：

```json
{
  "label": "overlay",
  "title": "MyDay 今日",
  "width": 264, "height": 380,
  "resizable": true, "minWidth": 220, "minHeight": 280,
  "maximizable": false, "minimizable": false,
  "visible": false, "skipTaskbar": true,
  "alwaysOnTop": true, "decorations": false, "transparent": true,
  "focused": false,
  "shadow": false
}
```

- **尺寸**：默认 264×380 逻辑像素；v1.1 起可拖边 / 角自由调整（前端 8 向隐形
  把手 `startResizeDragging` + `resizable: true`，minWidth/minHeight 220×280 与
  Rust 侧钳制上限 1200×1600 双保险），调整后存 `overlay.size`，show 时恢复；
  内容超高内部滚动（§4.4 上限配合，正常情况不出滚动条）。
  位置持久化同款约束：原生 Wayland 降级时尺寸不读写。
- **不抢焦点**：`focused: false` + Rust 侧 `show()` 前不调 `set_focus`；出现/刷新
  不得打断用户正在输入的窗口。
- **位置**：初始吸附主显示器工作区（work_area，扣任务栏）右上角，边距 12 逻辑
  像素。头部日期行为拖动区（`data-tauri-drag-region`），拖动后的位置存
  `overlay.custom_pos`，此后以自定义位置为准；每次 show 时按当前工作区做边界
  收敛（分辨率 / 缩放变化后窗口不得留在屏外）。设置面板点角落 = 清除
  `custom_pos` 重新吸附。
  位置持久化在 Windows / X11(XWayland) 路径有效；`MYDAY_BACKEND=wayland` 原生
  降级时拖动仍可用（合成器代移），但客户端无法读写坐标，位置不记忆。
- **关闭 = 隐藏**：沿主窗口保活模式（lib.rs `on_window_event` → `window.hide()`），
  不销毁窗口，托盘退出才真正结束。
- 窗口路由：沿 `App.svelte` 现有 label 路由，新增 `{#if label === "overlay"}` 分支
  渲染 `OverlayWindow.svelte`，无新前端入口。

```
┌──────────────────────────────┐ ← 初始右上角贴边，边距 12px；头部可拖动
│ 9月17日 周三      2/5 ✓  ⚙  │  头部：日期 + Task 完成进度 + 设置
├──────────────────────────────┤
│ ▌14:00–15:00 设计评审        │  日程（start_at 升序，进行中高亮左条）
│  19:30 跑步                  │
│ ☐ 16:00 出调研结论           │  今日待办（due 升序，勾选完成）
│ ☐ 交周报                     │
│ ☐ 回复邮件                   │
├──────────────────────────────┤
│ 逾期 2 · 未安排 3            │  统计摘要（只读计数，点击开主窗口）
└──────────────────────────────┘
   空态：今日已清 ✓（完成 n 条）
```

## 4. 内容规格：「今日未完成」的求值语义

本地日语义沿承 view.rs 既有实现；求值放 Rust 侧新命令 `overlay_today`，前端不做
第二份语义拼装。

### 4.1 数据集

| 区块 | 条件 | 说明 |
|---|---|---|
| 今日日程 | `ItemType::Event` 且 `start_at ∈ [今日00:00, 明日00:00)`（本地日） | 日程无完成态；跨到明天的日程按开始时间归属 |
| 今日待办 | `ItemType::Task` 且 `status = todo` 且 `due_at ∈ [今日00:00, 明日00:00)` | 无 `due_at` 的 Task 不进列表，计入「未安排」摘要（§4.3）——将来池不稀释「今日」列表 |
| 统计摘要 | 逾期 = `Task` 且 todo 且 `due_at < 今日00:00`；未安排 = `Task` 且 todo 且无 `due_at` | 仅计数不成列（§4.3），陈账可见但不刷屏 |

软删（`deleted_at`）一律排除；全天（`due_all_day`）Task 正常进入待办区，不显示
时刻。

### 4.2 排序

日程按 `start_at` 升序；待办按 `due_at` 升序。同刻按 `created_at` 稳定排序。

### 4.3 统计摘要行

窗口底部固定一行只读摘要：`逾期 N · 未安排 M`。N 或 M 为 0 时该项省略；两者皆 0
时不显示此行（今日有待办）或显示空态文案（今日全空，§4.4）。**只计数、不展开**
——逾期陈账与将来池容易堆积几十条，平铺违背「简洁」，处理动作在主窗口完成；
但计数常驻可见本身就是提醒。点击摘要行打开主窗口（P1 不做定位/过滤联动）。

### 4.4 容量与空态

今日待办超过 6 条时待办区内部滚动（细滚动条，hover 才显）。头部进度 `n/m` 只统计
今日待办（不含逾期与未安排），完成勾选即更新。列表与摘要全空时显示
「今日已清 ✓（完成 n 条）」，n 为今日已完成数（`status = done` 且今日到期）。

### 4.5 刷新时机

- `listen("data-changed")` → 重查 `overlay_today`（现有命令集全部变更都广播该事件，
  `app.emit` 全窗口可达，悬浮窗零额外接线）。
- 每 60s 本地 tick 重查：覆盖跨午夜清空、日程「进行中」态迁移、自然老化——
  这些不产生 `data-changed`。SQLite 本地查询开销可忽略。
- 两条路径幂等合并：tick 命中 data-changed 后 1s 内不重复查（节流）。

## 5. 交互

### 5.1 两态开关（置顶 / 锁定）

| 态 | `always_on_top` | `ignore_cursor_events` | 鼠标行为 |
|---|---|---|---|
| 默认（置顶） | true | false | 悬浮窗可点、可勾选、可滚 |
| 锁定（穿透） | true | **true** | 一切事件穿透到下层窗口，悬浮窗纯展示 |

锁定不是独立置顶开关的替代，而是置顶的超集；P1 不提供「不置顶的悬浮窗」。

### 5.2 解锁入口（必须在窗口外）

穿透状态下窗口收不到任何鼠标事件，故解锁入口一律在外部，三个入口等价：

1. 托盘菜单「悬浮窗锁定」复选项（P1 主入口，与「显示悬浮窗」开关同组）。
2. 主窗口设置区同款开关。
3. （P2）全局快捷键切换锁定。

### 5.3 显式开关，不做 hover 隐式切换

「鼠标移近自动解除穿透」需要窗口自身感知 hover，而穿透中窗口收不到 hover 事件；
Rust 侧轮询全局光标位置可绕过，但属隐式魔法且 Wayland 下坐标语义不稳，列为 P2
评估项（§12），P1 一律显式开关。

### 5.4 条目与窗口交互

- **拖动**：按住头部日期行拖动即可移动窗口（§3），无磁吸无吸附动画；锁定态下
  自然不可拖（整窗穿透）。
- **勾选**：调既有 `complete_task`；`data-changed` 自动广播，主窗口同步刷新，
  悬浮窗条目淡出（300ms）后列表重排。
- **点击条目 / 摘要行**：show + focus 主窗口（P1）；定位并高亮该条目为 P2。
- **头部 ⚙**：展开内联设置面板（透明度滑杆、角落吸附重置 tl/tr、锁定开关）。
  锁定态下此入口不可达（窗口无交互），符合预期——解锁先走 §5.2。

### 5.5 透明度

窗口本身 `transparent: true`，透明度用 CSS 实现（背景/边框 opacity，文字不透明度
单独保底 ≥85% 保证可读），滑杆范围 30%–100%，默认 90%，实时预览、失焦/变更即存。
不用 `set_background_color`（Windows 忽略 alpha，且无法做渐变边缘）。

## 6. 配置与持久化

沿 settings KV 表（store.rs 已有读写方法），key 统一 `overlay.` 前缀：

| key | 类型 | 默认 | 说明 |
|---|---|---|---|
| `overlay.enabled` | bool | `false` | 开箱不改变现状；用户开启后随启动自动 show |
| `overlay.corner` | `"tr"/"tl"` | `"tr"` | 初始吸附角落；设置面板点击 = 清除 `custom_pos` 重新吸附 |
| `overlay.custom_pos` | `{x,y}` 可选 | 无 | 拖动后的自定义位置（逻辑坐标，§3）；原生 Wayland 降级时不写入 |
| `overlay.size` | `{w,h}` 可选 | 无 | 拖边调整后的自定义尺寸（逻辑坐标，v1.1）；None = 声明默认 264×380 |
| `overlay.opacity` | 0.30–1.00 | `0.90` | CSS 透明度 |
| `overlay.locked` | bool | `false` | 锁定（穿透）态，启动恢复 |

读写走新增命令 `get_overlay_config` / `set_overlay_config`（整体读写 JSON，不做
逐 key 命令）；settings 表已有 upsert 语义，无 schema 变更。

## 7. 数据流与命令清单

```
主窗口/快速添加/CLI/Agent → 既有变更命令 → emit("data-changed") ─┐
reminder_loop（30s tick）───────────────────────────────────────┤
overlay 60s tick（自身定时）────────────────────────────────────┤
                                                                ▼
                                    overlay 窗口 listen → invoke("overlay_today")
                                                                │
                                    ◄── Vec<OverlayEntry>（已按 §4 排序打包）
```

新增 Rust 命令（commands.rs，全部走 `emit_changed` 之外不广播——读命令不广播）：

| 命令 | 职责 |
|---|---|
| `overlay_today` | 求值 §4 数据集，返回 `{ date, events[], tasks[], done_count, overdue_count, unscheduled_count }` |
| `get_overlay_config` / `set_overlay_config` | §6 配置读写 |
| `overlay_apply_window_state` | 把 locked/size/corner/custom_pos 落到真实窗口：`set_ignore_cursor_events` + `set_size` + `set_position`（定位仅 X11 路径生效） |
| `overlay_set_visible` | show/hide（含状态落地与位置重算），托盘与主窗口设置共用 |
| `overlay_save_drag_pos` / `overlay_save_resize_size` | 拖动 / 拖边结束后读回坐标与尺寸持久化（前端防抖调用；原生 Wayland 下为空操作） |

`overlay_today` 的求值逻辑放 myday-core（`pub fn today_overlay(&Store, Local日期)`），
可独立于 Tauri 做单测；命令层只做序列化。

## 8. 生命周期

| 时机 | 行为 |
|---|---|
| 应用启动 | 窗口预建（`visible:false`）；`overlay.enabled=true` 时 `overlay_set_visible(true)` |
| 托盘菜单 | 新增两项：「显示悬浮窗」开关、「悬浮窗锁定」复选；沿 setup_tray 现有 MenuBuilder 模式 |
| 主窗口关闭 | 不影响悬浮窗（主窗本就 hide 保活，进程不退） |
| 悬浮窗关闭（Alt+F4/右键关闭） | hide 保活，`overlay.enabled` 不变（下次启动仍显示）；托盘开关控制的是 enabled |
| 托盘「退出」 | 全部销毁 |
| 单实例二次启动 | 带参数 `--overlay` 时唤起悬浮窗，否则前置主窗口（沿 ipc_bridge 现有 quick-add 分支） |

## 9. 实现拆解（P1）

1. **lib.rs**：`platform::force_x11_on_wayland()` 注入（§2.3）；setup_tray 增两项；
   on_window_event 对 `overlay` 窗口 hide 保活。
2. **tauri.conf.json**：§3 窗口声明。
3. **myday-core**：`today_overlay()` 求值 + 单测（本地日/逾期边界/跨午夜/未安排
   计数边界/软删排除/排序稳定性）。
4. **commands.rs**：§7 四组命令。
5. **前端**：`OverlayWindow.svelte`（只读列表 + 勾选 + 底部摘要行 + 头部拖动区 +
   设置面板，复用现有条目样式变量与 `complete_task` 调用）；App.svelte 加 label
   分支；main.ts 无改动。
6. **设置页**：主窗口设置区加「今日悬浮窗」组（enabled/corner/opacity/locked）。

工作量估计：核心 1.5–2 天（其中 myday-core 求值 + 单测 0.5 天）；含双平台手测
矩阵合计 2–3 天。

## 10. 测试

**单元（myday-core）**：§4.1–4.2 全部边界——本地日跨时区、`due_all_day`、逾期恰好
卡今日 00:00、跨午夜前后快照对比、无 due Task 排除、软删排除、同刻稳定排序。

**E2E（Playwright + e2e mock）**：沿 e2e-viewmock 机制——
- 悬浮窗渲染：区块顺序、进度数字、摘要行文案（`逾期 N · 未安排 M`，全 0 省略）、
  空态文案；
- 勾选完成：条目淡出、进度更新、摘要计数随之变化、data-changed 后主窗口同步
  （断言状态而非点击穿透后的窗口，锁定态下 Playwright 点击会落到下层）；
- 拖动定位：真实 WM 拖拽不可脚本化，以状态断言代替（`custom_pos` 写入、show 后
  恢复、屏外坐标被收敛回工作区）；
- 设置面板：透明度 CSS 变量、角落重置清除 `custom_pos`、锁定开关状态持久化。

**手测矩阵**：

| 环境 | 预期 |
|---|---|
| Windows 10/11 | 全功能；不进任务栏；Alt+Tab 是否可见记录为已知行为 |
| Linux X11（GNOME/KDE） | 全功能 |
| Linux Wayland GNOME（注入后 XWayland） | 置顶/定位/穿透全部生效；日志确认注入发生 |
| `MYDAY_BACKEND=wayland` 强制原生 | 置顶/定位不生效，设置页显示兼容性提示，无报错 |
| 用户已设 `GDK_BACKEND` | 不被覆盖 |

**验收硬线**（来自原始需求）：锁定态下，鼠标点击/滚轮完全作用于下层窗口；右上角
默认位不遮挡下层窗口标题栏按钮（关闭叉）；悬浮窗出现与刷新永不抢焦点。

## 11. 里程碑

- **P1（本 spec §1 目标清单）**：上述 §9 全部 + §10 测试。
- **P2（评估项，按反馈立项）**：点击条目/摘要行定位到主窗口具体条目或过滤视图；
  拖动磁吸 / 边界吸附；Rust 轮询光标实现 hover 临时解除穿透；全局快捷键切换锁定；
  独立 X11 helper 进程（若 XWayland 分数缩放模糊成为普遍反馈）。
- **不做**：见 §1 非目标。

## 12. 风险与开放问题

| # | 风险 | 缓解 |
|---|---|---|
| R1 | XWayland 下 GNOME 实验性分数缩放（125%/150%）会使窗口轻微发虚 | 主流 100%/200% 无影响；普遍反馈则按 D3 拆 helper 进程 |
| R2 | 平铺 WM（i3/sway）可能忽略置顶（tao 对 X11 置顶的表述是「建议」） | 非目标用户环境；`is_always_on_top()` 自检 + 设置页兼容性提示 |
| R3 | 无合成器的老 X11 环境透明失效 | 透明度 CSS 自动退化为不透明深色背景，功能不受损 |
| R4 | 锁定态用户「找不到解锁入口」（窗口点不动） | 首次锁定时通知气泡一次性提示「解锁请用托盘菜单」；托盘项文字直白 |
| R5 | `data-changed` 高频变更（批量导入）造成悬浮窗抖动重排 | 300ms 防抖 + 列表高度固定动画；导入场景本身罕见 |

开放问题：Alt+Tab 中是否显示悬浮窗（Windows skipTaskbar 后仍可能出现在 Alt+Tab），
P1 接受现状，记录到用户文档即可；若要彻底隐藏需 WS_EX_TOOLWINDOW，P2 评估。

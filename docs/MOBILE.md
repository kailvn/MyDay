# 移动端（Android / iOS）骨架与路线

定稿于移动端立项讨论（2026-09）。形态决策：**同一仓库、同一 `src-tauri`，不做独立移动端项目**；
移动端界面走**全新组件**（复用 `lib/` 的 api 层与纯逻辑模块，不复用桌面 Shell），首版只做三件套：

| 界面 | 说明 |
| --- | --- |
| 日历主页 | 月网格窄屏版，点日期弹当天列表；周/日时间网格（WeekGrid 拖拽改期）**明确不做** |
| 快速添加 | FAB → 底部弹层包 ItemPanel（复用表单逻辑，零逻辑改动） |
| 回收站 | TrashView 思路重做窄屏列表 |

## 代码分界（cfg 门禁）

tauri-build 为非 Android/iOS 目标注入 `cfg(desktop)`，为 Android/iOS 注入 `cfg(mobile)`。
依赖表用 `target.'cfg(not(any(target_os = "android", target_os = "ios")))'.dependencies` 对应隔离
（见 `apps/desktop/src-tauri/Cargo.toml`）。

**桌面专属（移动端不编译）：**

- 模块：`ipc_bridge`（Unix socket，CLI 协同）、`platform`（X11/Wayland 注入）、`reminder_loop`（notify-rust/DBus 通知）
- 能力：托盘、单实例、自启、快速添加窗口、今日悬浮窗（移动端单 WebView，多窗口不可用）
- 命令（`commands.rs`）：`open_file_path` / `reveal_file_path` / `open_log_dir` /
  `get_autostart` / `set_autostart` / `open_quick_add` / `overlay_*` 全组。
  命令注册在 `commands::handler()` 一张表内，桌面专属条目用 `#[cfg(desktop)]`
  行级标注（generate_handler 会把属性转写到 match 分支），**无需维护第二张表**。

**两端共用：** `myday-core` 全部、数据面命令（条目/模板/字段/视图/回收站/统计/设置/附件/ICS/备份）、
`app_info`（移动端只回 version + data_root）、`set_ui_lang`（移动端只落库，无托盘/窗口标题联动）。

## 前端入口

- `index.html` → `src/main.ts`：桌面（多窗口按 label 路由，未动）
- `mobile.html` → `src/mobile/main.ts` → `src/mobile/App.svelte`：移动端正式界面
  （2026-09-27 落地，替代初版占位页）
- `vite.config.ts` 配了双入口（main + mobile），`pnpm build` 同时产出两份
- `tauri.android.conf.json`：平台配置覆盖，移动端 main 窗口指向 `/mobile.html`
  （dev 模式自动落到 devUrl 同路径；Windows/macOS/Linux 桌面不受影响）

## 移动端界面（v1：便捷添加 + 日历 + 系统提醒）

形态对标 Outlook 移动端（月网格 + 事件色块 + FAB），特色走自己的：

- **日历主页**（`CalendarMonth.svelte`）：6×7 月网格周一起始，日号 + 休/班角标 +
  节假日名 + 每格至多 2 条类型色块（日程蓝/待办橙/记录绿，超出 +n）；点日期弹
  **日议程**（`DaySheet.svelte`），点条目进**只读详情**（`ItemDetail.svelte`）。
  数据口径与桌面日历一致：`list_items_window` → `expandItems`（重复展开虚拟实例）
  → 按天分组（日程跨天两侧都算 / 待办=开始+截止 / 记录=发生日）。
- **快速添加**（`QuickAdd.svelte`，FAB）：单行自然语言——标题里识别时间词复用桌面
  `timewords.parseTimeHint`，自动预填日期/时刻并给识别 chip（可清除）；手动改过
  时间字段或类型后停止自动预填。类型三选：日程（开始+结束）/ 待办（截止）/
  记录（无时间）。**字段编辑不做**（可看不可编辑原则）。
- **只读详情**：时间 / 备注 / 字段（`fieldBadges` 只读渲染）/ 标签 / 重复规则 /
  创建修改时间；轻操作仅待办就地完成·回退、删除进回收站。
- **回收站**（`Trash.svelte`，顶栏 🗑）：恢复 / 彻底删除 / 清空，复用 `trash.*` 词条。
- 词典分片 `i18n/parts/mobile.ts`（zh+en 同 key 集，i18n:check 通过）；主题变量
  全量复用 `app.css` 纸黄配色（自动深色模式）。

**系统提醒（移动端）**：`reminder_loop` 重构为两端共用循环（30s `tick_once`）+
平台通知器——桌面 `DesktopNotifier`（notify-rust，带动作按钮）不变；新增
`MobileNotifier`（tauri-plugin-notification，无动作按钮，点击即唤起 App）。
插件依赖 `target.'cfg(any(android, ios))'` 门禁，capability 单列
`capabilities/mobile.json`（platforms 限定，桌面构建不受影响）。通知权限由前端
启动时申请（`@tauri-apps/plugin-notification`，Android 13+ 必需），未授予只提示
不阻塞。

## 真机调试（2026-09-27 已跑通，小米 12S Ultra / Android 16）

```bash
export ANDROID_HOME=$HOME/Android/Sdk JAVA_HOME=$HOME/jdks/jdk-21.0.12.1+1
cd apps/desktop && pnpm tauri android dev   # 首次数分钟，之后秒级；手机上即时热重载
```

手机侧一次性配置：开发者选项 → USB 调试；MIUI 另需开 **「USB 安装」**（否则
`INSTALL_FAILED_USER_RESTRICTED`）。

**网络模型（重要）**：`tauri android dev` 对物理设备会**强制**把 devUrl 换成电脑的
局域网 IP（源码 `run_dev`：物理设备无条件走网络地址，`--host` 参数并不存在），并把
该 IP 通过 `TAURI_DEV_HOST` 导出给 vite（本仓 `vite.config.ts` 已读取）。因此要求
**手机能直接访问电脑 IP**：

- 办公室/校园这类开客户端隔离的 Wi-Fi（如 10.5.x.x）不通——表现为 App 黑屏、
  vite 零请求；
- 解法：手机开 **USB 网络共享**（USB Tethering），电脑多出一块 192.168.129.x 网卡
  （网关即手机），CLI 会选中这块网卡的 IP，链路完全走 USB 线，且与将来热点同步
  同一拓扑；
- 备选：两端连同一个普通（无隔离的）Wi-Fi。

调试链路排障顺序：`adb devices` 授权 → `adb reverse --list` → 看 dev 日志里
"Replacing devUrl host with X" 的 X → 手机侧 `adb shell ping <X>` → logcat 过滤
`RustStdoutStderr`（Rust stderr/panic 都在这）与 `Tauri/Console`（WebView 控制台）。

## 真机调试环境坑位备忘（每一项都实测踩过）

1. **`maven.google.com` 不通**：gradle 拉 AGP/Kotlin 必挂（`BUILD FAILED`，日志被
   Io 环境转储淹没，真因要看 `What went wrong` 段）。已配
   `~/.gradle/init.d/mirrors.gradle` 走阿里云镜像（clear() 后整组替换，避免模板里
   的 google() 排在前头）。网络环境变化可删该文件恢复官方源。
2. **系统 Java 是 JRE 无 javac**：`Could not resolve project :buildSrc ...
   does not provide the required capabilities: [JAVA_COMPILER]`。已下载完整
   Temurin JDK 21 到 `~/jdks/jdk-21.0.12.1+1`，用它做 `JAVA_HOME`。
3. **`BuildTask.kt` 的 `pnpm-native`**：cargo-mobile2 生成的 gradle 任务以
   `pnpm-native` 启动 CLI（corepack 的原生 shim，实为存在），但 daemon PATH 可能
   缺 corepack 目录导致 `A problem occurred starting process 'pnpm-native'`。
   已把 `gen/android/buildSrc/.../BuildTask.kt` 的 executable 改为 `pnpm`
   （两者皆可，`tauri android init` 重跑后若复发按此重改）。
4. **移动端数据目录（已修，lib.rs）**：安卓无 HOME/XDG，`dirs::data_dir()` 返回
   None，启动即 `cannot resolve data dir` + 黑屏 + 进程循环重启（且 `exit(1)` 在
   非主线程触发 FORTIFY SIGABRT）。修法：`mobile_setup` 里用 tauri 路径解析器的
   `app_data_dir()`（/data/user/0/<pkg>）设 `MYDAY_DATA_DIR` 再开库——因此移动端
   开库必须晚到 setup 里做。
5. rustup 1.29 忽略 `RUSTUP_DIST_SERVER`（镜像缺组件时从 static.rust-lang.org
   手动装 rust-std，见下）。
6. sdkmanager 非 `latest` 布局要显式 `--sdk_root=$ANDROID_HOME`。
7. **NDK_HOME 不要显式设置**：指向 NDK 目录会触发 cargo-mobile2 的严格版本校验
   （r29 的 `Pkg.Revision = 29.0.13846066-beta3` 被判 invalid）；留空让它从
   `$ANDROID_HOME/ndk/` 自动发现即可。
8. tauri CLI 的组件安装确认在非 TTY 下会静默跳过，要预装好或真 TTY 里跑。
9. `gen/android/app/build.gradle.kts` 要求 **compileSdk 36 / minSdk 24**。

## 构建环境（本机 2026-09-27 已就绪）

| 组件 | 位置 / 说明 |
| --- | --- |
| JDK 21 | `~/jdks/jdk-21.0.12.1+1`（完整 JDK，含 javac；系统自带的 java-21 是 JRE，gradle buildSrc 编不过） |
| Android cmdline-tools | `~/Android/Sdk/cmdline-tools/`（`bin/` 直排 + `latest/` 双布局；cargo-mobile2 查前者，Google 官方文档用后者） |
| SDK 组件 | platform-tools、platforms;android-34/35/36、build-tools;34/35、**ndk;29.0.13846066** |
| Rust Android 目标 | 4 个目标（aarch64 / armv7 / i686 / x86_64-linux-android[eabi]）std 库已装进 stable 工具链 |
| gradle 镜像 | `~/.gradle/init.d/mirrors.gradle`（阿里云，见坑位 1） |

### 安装期补充坑位

1. **rustup 1.29 忽略 `RUSTUP_DIST_SERVER`**（本机自动落到 tuna 镜像，而该镜像缺
   1.98.0 的 Android std 组件）。绕过方式：从 `https://static.rust-lang.org/dist/<date>/`
   手动下载 `rust-std-1.98.0-<target>.tar.xz`，把内层
   `rust-std-<target>/lib/rustlib/<target>` 拷进
   `~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/`。
   （tarball 外层目录带版本前缀 `rust-std-<ver>-<target>/`，别少拷一层。）
2. `cargo check --target aarch64-linux-android` 虽不链接，但 libsqlite3-sys bundled
   要交叉编译 C，需指认 NDK 的 clang/llvm-ar：

   ```bash
   NDKB=$ANDROID_HOME/ndk/29.0.13846066/toolchains/llvm/prebuilt/linux-x86_64/bin
   export CC_aarch64_linux_android=$NDKB/aarch64-linux-android24-clang
   export AR_aarch64_linux_android=$NDKB/llvm-ar
   ```


## 常用命令

```bash
export ANDROID_HOME=$HOME/Android/Sdk
export JAVA_HOME=$HOME/jdks/jdk-21.0.12.1+1
# 注意：NDK_HOME 留空（见真机调试坑位 7），tauri CLI 会自动发现

pnpm tauri android init          # 已生成 gen/android（compileSdk 36 / minSdk 24），无需重跑
pnpm tauri android dev           # 真机调试，见「真机调试」一节
pnpm tauri android build         # 出 APK/AAB
```

建议把 `ANDROID_HOME` / `JAVA_HOME` 两行写进 `~/.bashrc`（NDK_HOME 特意不要写）。

## 局域网同步（v1，2026-09-27 落地）

无云服务器、非实时：**桌面 = 同步站**（托盘常驻时监听 `0.0.0.0:26136`），手机前台手动触发。

- **协议**（`myday-core/src/sync.rs`，两端共用同一实现）：两步全量索引交换——
  `POST /sync/index`（客户端索引 → 服务器算差异 `to_client`/`want`，纯读）→
  `POST /sync/push`（客户端补送服务器缺的更新）。Bearer token 鉴权，token 存
  settings `sync.token`。
- **合并**：条目级 LWW（`updated_at` 新者胜）+ 冲突副本——覆盖存活条目前，若本地
  自上次同步（settings `sync.last_sync_at`）后改过且内容不同，先完整克隆为带
  「同步冲突」标签的普通记录再覆盖，败者内容永不消失；删除以墓碑传播（进回收站
  30 天），同步永不硬删；每条目单事务（含提醒整组替换、附件行原位保留），
  任意中断重跑即收敛。
- **桌面**：`sync_server.rs`（axum）+ 设置页「移动端同步」小节（开关 / 地址 / 配对码
  / 重新生成 / 上次同步），随启动自动恢复（`sync.enabled`）；服务监听中时页面直接
  显示**同步二维码**（`uqr` 渲染，内容 `myday-sync://ip:port/token`）。
- **手机**：日历页顶栏 ⇄ 弹层，首选**「扫码填写」**（`tauri-plugin-barcode-scanner`，
  仅移动端依赖；扫桌面二维码自动填地址 + 配对码并立即同步）；地址也可留空按
  `/proc/net/route` 探测默认网关（USB 网络共享 / 热点拓扑下桌面就在网关上），
  或全手填，配对一次即存。读取侧批量查询（`hydrate_many` / `load_items_full`，
  无每条 N+1），写入仍每条目独立事务保证中断收敛。
- **范围**：条目（含提醒整组、标签、字段值 extra）+ **模板与字段定义**（随索引
  响应单向对齐：桌面权威 → 手机，全量 REPLACE + 缺失行删除，无 LWW——两者没有
  updated_at，手机端只消费不生产）。附件文件、视图不同步；附件行随条目原位保留
  但手机端不渲染（桌面粘贴的图片只存在于桌面）。时钟依赖两端 NTP 大致准确。
- 单测：`cargo test -p myday-core --lib sync::tests`（同时编辑 / 单方编辑 /
  墓碑传播 / 本地编辑胜过墓碑 / 重跑幂等 / 索引三向判定）。

## 已知边界 / 后续路线

- **identifier 仍是 `dev.myday.desktop`**：它就是 Android applicationId，上架前必须定稿
  （改了影响数据目录与升级路径）；当前先不动。
- ~~**提醒未接**~~：已接入——移动端通知走 `tauri-plugin-notification`（main.ts 启动申请
  权限），新建日程/待办可选提醒档位（跟随默认/无/准时/5/15/30 分/1 小时/1 天，
  映射 `@start±` / `@due±` 相对 spec，条目改时间自动跟随），reminder_loop 手机端照常触发。
- **数据不互通**：已解决——局域网同步 v1 已落地（见上节）。
- **移动端编辑能力刻意收敛**：条目字段编辑仍在桌面端；手机端支持轻量创建
  （自然语言快速添加 + 模板记录）与删除/完成，编辑走桌面。
- `gen/android` 目前被仓库 `.gitignore`（`gen/` 整体忽略）。若后续在 Android 工程里做手工
  定制（图标、签名配置、权限），需调整 ignore 规则把 `gen/android` 收进版本控制（Tauri
  官方建议提交），否则换机要重新 init。
- iOS：需要 macOS + Xcode，本机（Linux）不覆盖；`src-tauri` 侧门禁已按两端可移植写好。

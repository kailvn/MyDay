# Windows 版构建（Linux 交叉编译）

在 Linux 上产出 Windows x64 的两个产物：

| 产物 | 路径 | 说明 |
| --- | --- | --- |
| 绿色版 | `target/x86_64-pc-windows-msvc/release/myday-desktop.exe` | 单文件，前端资源已嵌入，拷贝到 Windows 即可运行 |
| 安装包 | `target/x86_64-pc-windows-msvc/release/bundle/nsis/MyDay_0.1.0_x64-setup.exe` | NSIS 安装器，缺 WebView2 运行时会自动下载引导 |

一键构建：`./scripts/build-windows.sh`

## 工具链原理

- **Rust → MSVC**：`cargo-xwin` 拉取 Windows SDK/CRT（缓存在 `~/.cache/cargo-xwin`），用 `clang-cl` 当 C 编译器、`rust-lld` 链接，目标 `x86_64-pc-windows-msvc`，与在 Windows 上原生编译产物同源。
- **C 代码**：`rusqlite` bundled 的 sqlite3.c 由 `clang-cl` 编译，静态库归档用 `llvm-lib`。
- **资源嵌入**（图标/版本/manifest）：`tauri-build` → `embed-resource`，非 Windows 宿主走 `llvm-rc`。
- **NSIS 打包**：tauri bundler 在非 Windows 宿主直接调用**原生 Linux `makensis`**（不需要 wine），额外插件 `nsis_tauri_utils.dll` 缓存在 `~/.cache/tauri/NSIS/`。

## 一次性环境准备

本机没有 root 权限，所有系统工具都解包到用户目录（~/.local）：

```bash
# 1) Rust 的 windows-msvc 标准库
#    清华镜像缺这个组件（404），官方源可下：
#    https://static.rust-lang.org/dist/<date>/rust-std-1.98.0-x86_64-pc-windows-msvc.tar.xz
#    解包后把 rust-std-*/lib/rustlib/x86_64-pc-windows-msvc 拷进
#    ~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/
#    并把组件包内的 manifest.in 复制为同目录 manifest-rust-std-x86_64-pc-windows-msvc，
#    在 lib/rustlib/components 里追加一行组件名。

# 2) cargo-xwin
cargo install cargo-xwin          # 首次构建会自动下载 Windows SDK（~1GB，存 ~/.cache/cargo-xwin）

# 3) LLVM 工具（clang-cl / llvm-rc / llvm-lib），免 root：
mkdir -p /tmp/debs ~/.local/clang-18-root && cd /tmp/debs
apt-get download clang-18 libclang-cpp18 libllvm18 llvm-18 libz3-4
for d in *.deb; do dpkg-deb -x "$d" ~/.local/clang-18-root/; done
ln -sf ~/.local/clang-18-root/usr/lib/llvm-18/bin/clang-18 \
       ~/.local/clang-18-root/usr/lib/llvm-18/bin/clang-cl   # clang 驱动按 argv[0] 切模式
ln -sf ~/.local/clang-18-root/usr/lib/llvm-18/bin/clang-cl ~/.local/bin/
ln -sf ~/.local/clang-18-root/usr/lib/llvm-18/bin/llvm-rc  ~/.local/bin/
ln -sf ~/.local/clang-18-root/usr/lib/llvm-18/bin/llvm-lib ~/.local/bin/

# 4) 原生 makensis（免 root）：
mkdir -p /tmp/nsis-debs ~/.local/nsis-root && cd /tmp/nsis-debs
apt-get download nsis nsis-common
for d in *.deb; do dpkg-deb -x "$d" ~/.local/nsis-root/; done
ln -sf ~/.local/nsis-root/usr/bin/makensis ~/.local/bin/
# makensis 按编译期前缀找数据目录，重定位后靠 NSISDIR 环境变量指向解包位置
# （build-windows.sh 已设置 NSISDIR=~/.local/nsis-root/usr/share/nsis）。
# 注意：tauri bundler 的子进程里 NSISDIR 会丢（makensis 报
# reading stub "/usr/share/nsis/Stubs/zlib-x86-unicode"），所以另有
# ~/.local/nsis-bin/makensis 包装器无条件注入 NSISDIR 后 exec 真 makensis，
# PATH 里排在 nsis-root/usr/bin 之前。

# 5) tauri NSIS 插件（github 直连不通，走代理；SHA1 需与 bundler 内置一致）：
mkdir -p ~/.cache/tauri/NSIS/Plugins/x86-unicode/additional
curl -L -o ~/.cache/tauri/NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll \
  https://gh-proxy.com/https://github.com/tauri-apps/nsis-tauri-utils/releases/download/nsis_tauri_utils-v0.5.3/nsis_tauri_utils.dll
# 校验：SHA1 = 75197FEE3C6A814FE035788D1C34EAD39349B860
```

## 手动构建步骤（脚本等效展开）

```bash
cd apps/desktop && pnpm install && pnpm build
cd src-tauri
export PATH=~/.local/clang-18-root/usr/lib/llvm-18/bin:~/.local/nsis-root/usr/bin:$PATH
export NSISDIR=~/.local/nsis-root/usr/share/nsis
cargo xwin build --release --target x86_64-pc-windows-msvc --features custom-protocol
cd ..
pnpm tauri bundle --bundles nsis --target x86_64-pc-windows-msvc
```

注意：不要用 `pnpm tauri build --runner cargo-xwin`——它会先问 rustup "target 是否已安装"，
本机 rustup 的清单登记不认手动解包的组件（rustc 本身没问题，只是 CLI 预检会拦），所以拆成
"cargo-xwin 编译 + tauri bundle 打包" 两步。

## Windows 端运行差异

- IPC：Unix 上走 UDS（`$XDG_RUNTIME_DIR/myday.sock`）；Windows 的 std AF_UNIX 尚未稳定
  （rust-lang/rust#150487），改为 `127.0.0.1:45987` TCP，协议不变，`MYDAY_IPC_PORT` /
  `MYDAY_SOCKET_PATH`（纯数字）可覆盖。
- 打开文件 / 定位文件：`xdg-open`/`gdbus FileManager1` 对应为 `cmd /c start` 与
  `explorer /select,`（见 `commands.rs` 的 cfg 分支）。
- 通知：`notify-rust` 在 Windows 上走 WinRT Toast。
- 数据目录：`dirs::data_dir()` → `%APPDATA%\myday`（Linux 为 `~/.local/share/myday`）。
- WebView2：Win10/11 一般自带；缺失时 NSIS 安装包用 downloadBootstrapper 引导安装。

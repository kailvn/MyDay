#!/usr/bin/env bash
# 从 Linux 交叉编译 MyDay Windows 版（MSVC 目标 + NSIS 安装包）。
#
# 一次性环境准备（已在本机完成，见 docs/BUILD-WINDOWS.md）：
#   rustup target add x86_64-pc-windows-msvc（镜像缺组件时可从 static.rust-lang.org
#   手动下载 rust-std 包解到工具链 lib/rustlib/ 并补 manifest）
#   cargo install cargo-xwin
#   ~/.local/clang-18-root/  ← clang-18 / llvm-18 deb 免 root 解包（clang-cl/llvm-rc/llvm-lib）
#   ~/.local/nsis-root/      ← nsis / nsis-common deb 免 root 解包（原生 makensis）
#   ~/.cache/tauri/NSIS/Plugins/x86-unicode/additional/nsis_tauri_utils.dll
#                            ← 走 gh 代理从 tauri-apps/nsis-tauri-utils releases 下载
#
# 产物：
#   target/x86_64-pc-windows-msvc/release/myday-desktop.exe          （绿色版，单文件）
#   target/x86_64-pc-windows-msvc/release/bundle/nsis/*-setup.exe    （NSIS 安装包）

set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

LLVM_BIN="$HOME/.local/clang-18-root/usr/lib/llvm-18/bin"
NSIS_BIN="$HOME/.local/nsis-root/usr/bin"
NSIS_DATA="$HOME/.local/nsis-root/usr/share/nsis"
# makensis 包装器：无条件注入 NSISDIR（tauri bundler 子进程里该变量会丢，
# makensis 回退到编译期前缀 /usr/share/nsis → 报 reading stub .../zlib-x86-unicode）
NSIS_WRAPPER_BIN="$HOME/.local/nsis-bin"

for tool in "$LLVM_BIN/clang-cl" "$LLVM_BIN/llvm-rc" "$LLVM_BIN/llvm-lib" "$NSIS_WRAPPER_BIN/makensis" "$NSIS_BIN/makensis"; do
  [ -x "$tool" ] || { echo "缺少工具: $tool（见脚本头部的一次性准备说明）" >&2; exit 1; }
done
[ -d ~/.cache/tauri/NSIS/Plugins/x86-unicode/additional ] || {
  echo "缺少 ~/.cache/tauri/NSIS 插件目录（nsis_tauri_utils.dll）" >&2; exit 1;
}

export PATH="$LLVM_BIN:$NSIS_WRAPPER_BIN:$NSIS_BIN:$PATH"
export NSISDIR="$NSIS_DATA"

# 1) 前端
cd "$ROOT/apps/desktop"
pnpm install --silent
pnpm build

# 2) Rust 交叉编译（custom-protocol 把前端资源嵌进 exe，tauri bundle 不会补这个 feature）
cd "$ROOT/apps/desktop/src-tauri"
cargo xwin build --release --target x86_64-pc-windows-msvc --features custom-protocol

# 3) NSIS 安装包（非 Windows 宿主上 tauri 调用原生 makensis）
cd "$ROOT/apps/desktop"
pnpm tauri bundle --bundles nsis --target x86_64-pc-windows-msvc

echo
echo "产物："
ls -lh "$ROOT"/target/x86_64-pc-windows-msvc/release/myday-desktop.exe
ls -lh "$ROOT"/target/x86_64-pc-windows-msvc/release/bundle/nsis/

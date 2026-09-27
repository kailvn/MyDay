# 网络与环境问题排查手册

记录实际踩过的坑：症状 → 诊断 → 解决。遇到同类问题先来这查。

## 1. git push 到 GitHub 间歇性失败

### 症状（三种都见过，可交替出现）
```
fatal: unable to access 'https://github.com/...': GnuTLS recv error (-110): The TLS connection was non-properly terminated
fatal: unable to access 'https://github.com/...': Failed to connect to github.com port 443 after 134s: Couldn't connect to server
fatal: unable to access 'https://github.com/...': Failure when receiving data from the peer
```
特征：**间歇性**——有时连续几次成功，有时连挂十几分钟；`api.github.com` 可能一直是通的，只有 `github.com`（git push / 网页）不通。

### 诊断
```bash
# DNS 解析到了哪个节点（20.205.243.166 = 新加坡，大陆访问常抖动）
getent hosts github.com

# 分层测试：TCP 通不通 / TLS 通不通
curl -4 -sS -o /dev/null -w "%{http_code} 连接%{time_connect}s\n" --max-time 15 https://github.com

# 对照组：api 子域走不同路由，经常它通而主站不通
curl -sS -o /dev/null -w "%{http_code}\n" --max-time 15 https://api.github.com
```

### 解决（按优先级）
1. **重试**。多数是间歇抖动，等 1–5 分钟再推即可。
2. **强制 HTTP/1.1**（GnuTLS 的 HTTP/2 协商在部分网络下断流，实测有效）：
   ```bash
   git config http.version HTTP/1.1   # 仓库级；全局加 --global
   git push origin main
   ```
3. **绕过 git push：用 GitHub API 达成同样目的**（api.github.com 路径通常独立且可达）：
   - 推 tag 触发 Release 构建（前提：被指的 commit 已在远端）：
     ```bash
     sha=$(gh api repos/<owner>/<repo>/commits/main --jq .sha)
     gh api -X PATCH repos/<owner>/<repo>/git/refs/tags/v1.3.0 -f sha=$sha
     ```
     更新 ref 同样会触发 `on: push: tags` 的 workflow。
4. **换解析入口**：DNS 给的节点死了，但其他 GitHub 入口 IP 可能全通。测一批：
   ```bash
   for ip in 140.82.112.3 140.82.113.3 140.82.116.3 140.82.121.3 20.27.177.113 20.200.245.247; do
     code=$(curl -4 -sS -o /dev/null -w "%{http_code}" --connect-timeout 5 --max-time 8 \
       --resolve github.com:443:$ip https://github.com)
     echo "$ip -> $code"
   done
   ```
   找到 200 的写 `/etc/hosts`：`<可达IP> github.com`（需要 sudo）。
5. SSH 备选（需在 GitHub 配置公钥；本机 ssh 只有 GitLab key，未配 GitHub）。

## 2. Android 真机 dev：应用白屏 / 加载失败

### 症状
手机 WebView 显示 `Failed to load resource ... error sending request for url (http://<IP>:1420/mobile.html)` 或白屏；点击无反应像卡死。

### 原因
dev 模式下手机 WebView 从电脑的 vite 加载页面。**电脑网卡/IP 变化后 vite 仍绑在旧地址**（如 USB 网络共享断开重连后 IP 从 `192.168.129.143` 变为 `.246`；或手机 Wi-Fi 换了网段），手机自然加载不到。

### 诊断
```bash
ss -tlnp | grep 1420                 # vite 实际绑在哪个 IP（是不是已失效的旧 IP）
ip -4 addr                           # 电脑现在有哪些 IP
adb shell ip -4 addr | grep inet     # 手机在哪些网段（找与电脑同网段的）
```
三者对不上就是这个原因。

### 解决
重启 dev 会话，**显式指定手机可达的电脑 IP**（不指定时 CLI 自动探测，可能选中手机访问不到的接口）：
```bash
cd apps/desktop
TAURI_DEV_HOST=<电脑上手机可达的IP> pnpm tauri android dev
```
网络拓扑优先级：**USB 网络共享直连**（最稳，见 §3）> 同一 Wi-Fi（注意客户端隔离）。

### 测手机到电脑的连通性（不装额外工具的土办法）
```bash
adb shell "ping -c 2 <电脑IP>"                                  # ICMP 通不通
# ping 通不等于 TCP 通（客户端隔离常只放 ICMP），用 nc 实测端口：
电脑侧: python3 -m http.server 14299 --bind 0.0.0.0
手机侧: adb shell 'echo "GET /" | nc -w 4 <电脑IP> 14299'       # 有 HTTP 响应即真通
```

## 3. USB 网络共享（推荐拓扑）

手机「设置 → 个人热点 → USB 网络共享」开启后：
- 手机侧多出 `rndis0`（如 192.168.129.244），电脑侧多出 `enx*`/`usb0`（如 192.168.129.246）
- 手机 ↔ 电脑点对点直连，**不受 Wi-Fi 客户端隔离影响**，电脑没外网也能用
- 注意：**USB 网卡重连后 IP 会变**（.143 → .246 这类），dev 会话要按 §2 重启
- 电脑端有时状态栏/手机侧显示共享已开但 `ip addr` 没有 usb 网卡——重新插拔 USB 触发一次

## 4. adb / 真机杂项

- **设备中途掉线**：`adb devices` 为空时轮询 `adb devices` 等它回来；回来后 dev 会话如果还在，直接 `adb shell am start -n dev.myday.desktop/.MainActivity` 拉起即可。
- **MIUI USB 安装开关**：`adb install` 报 `INSTALL_FAILED_USER_RESTRICTED` = 手机上「开发者选项 → USB 安装」没开（需插着 USB 且登录小米账号）。
- **`adb shell input text` 只支持 ASCII**：中文、空格都输不进去（空格要用 `%s`，且直接给空格会被当成参数分隔符——实测 `'Sync test event'` 只输入了 `Sync`）。中文输入需在手机上手动敲，或装 ADBKeyboard 类 IME。
- **双击类交互的自动化测试**：两段式确认有 3 秒超时，「读截图 → 再点」的时间远超窗口，必须**在一条命令里连发两次 tap**（间隔 <1s）。
- **截图坐标换算**：`screencap` 出的图若与 `wm size` 不同，按 `实际 = 截图坐标 × (实际宽/截图宽)` 换算（本机 1080/2400 对截图 900/2000 即 ×1.2）。

## 5. shell 后台清理的自匹配陷阱（多次踩）

`pkill -f "tauri android dev"` / `pgrep -f "vite.js"` 会**匹配到自己所在的复合命令**（命令行里含同样字样），把自己的 shell 杀掉，表现为命令静默无输出、后续步骤全没执行。

规避：
- 用任务管理器接口（如 zcode 的 TaskStop）停后台任务，而不是 pkill；
- 必须用 pkill/pgrep 时，模式写成自匹配不上的形式（如 `pkill -f "tauri[ ]android"`），或先用 `pgrep -fl` 确认目标列表再按 PID kill。

## 6. GitHub Actions 相关（Android 构建三个坑，修复已固化在 release.yml 注释里）

- `android-actions/setup-android@v3` 与最新 cmdline-tools 16 不兼容（装已废弃的 'tools' 包报错）——不用它，runner 预装 SDK 直接用；
- `sdkmanager` 不在 runner PATH、也没有 `cmdline-tools/latest` 软链——探测 `cmdline-tools/*/bin/sdkmanager` 取最新；
- `tauri android build --target` 用 Android 短名（`aarch64`），不认完整 Rust triple；
- `aws-lc-sys`（reqwest 默认加密后端）交叉编译需要 NDK clang——纯 `cargo check --target` 也要装 NDK 并设 `CC_aarch64_linux_android`（见 ci.yml 的 android-check job）；
- GitHub Actions 的 `env:` 上下文引用不到 runner 自带环境变量（如 `$ANDROID_HOME`），要在 `run:` 的 shell 里取。

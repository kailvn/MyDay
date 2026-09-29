# 提醒守护：GUI 关闭时的调度兜底

MyDay 的提醒是「应用内 30 秒循环」（`tick_once`，见 `crates/myday-core/src/reminder.rs`），
桌面端此前完全依赖 GUI 常驻——退出应用 = 提醒停摆。现在核心循环有了无 GUI 的
补跑入口：

```bash
myday reminders tick          # 人类可读：无到期提醒 / 已补发 N 条
myday reminders tick --json   # {"ok":true,"data":{"sent":2}}
```

行为约定：

- **GUI 运行中则自动跳过**（`{"sent":0,"skipped":"gui-running"}`）——应用内循环
  负责，两边同跑会因「先通知后记 `reminder_log`」的窗口重复响铃。因此定时器
  可以放心按分钟跑，无需自己判断应用死活。
- GUI 未运行时直读数据库，经 notify-rust 发系统通知。**不带动作按钮**
  （短命进程等不到 DBus / 系统回调），完成 / 稍后操作请打开应用——
  应用内「提醒中心」仍保留全部历史与操作入口。
- **闹钟档（alarm）以 critical 常驻通知 + 系统提示音交付**，不弹置顶闹钟窗
  （那是 GUI 的呈现层）；宁降级不漏报。
- 补发窗口（默认 120 分钟，设置键 `reminder_catchup_minutes`）与错过聚合摘要
  与应用内循环完全同语义：窗口外的错过静默记日志并合成一条摘要。

## systemd（Linux）用户定时器

`~/.config/systemd/user/myday-reminders.timer`：

```ini
[Unit]
Description=MyDay reminder sweep (GUI-independent fallback)

[Timer]
OnBootSec=2min
OnUnitActiveSec=1min

[Install]
WantedBy=timers.target
```

`~/.config/systemd/user/myday-reminders.service`：

```ini
[Unit]
Description=MyDay reminder sweep (GUI-independent fallback)

[Service]
Type=oneshot
# 按实际安装路径调整；确保能找到 myday 可执行文件
ExecStart=/usr/bin/myday reminders tick
```

启用：

```bash
systemctl --user daemon-reload
systemctl --user enable --now myday-reminders.timer
systemctl --user list-timers myday-reminders.timer   # 验证
```

注意：

- 用户定时器只在你登录后运行；无头服务器上没有 DBus 时通知会发送失败，
  `tick` 仍会照常记 `reminder_log`（宁记不发，不重复轰炸下次登录）。
- 若把数据目录放在非默认位置，在 service 里加
  `Environment=MYDAY_DATA_DIR=/path/to/myday`。

## Windows 计划任务

管理员不需要；当前用户即可（`myday.exe` 换成实际安装路径）：

```powershell
schtasks /Create /SC MINUTE /MO 1 /TN "MyDay 提醒兜底" `
  /TR "C:\Program Files\MyDay\myday.exe reminders tick"
```

删除：`schtasks /Delete /TN "MyDay 提醒兜底" /F`。

## 与 Android 端的关系

Android 端不走本机制：提醒已整体移交系统 `AlarmManager`（Doze / 进程被杀都能
触发，见 CHANGELOG v1.4.0），无需轮询。本文档只覆盖桌面端。

//! MyDay 系统闹钟移交插件（仅 Android 有实现；桌面为 no-op）。
//!
//! 提醒的「调度」交给系统：Rust 提醒循环算出应调度集合（notify = 系统通知档 /
//! alarm = 闹钟档，见 myday-core `ReminderKind`），前端拉取后调用本插件
//! `sync_alarms` 写入 AlarmManager（`setAlarmClock`，Doze / 进程被杀也能触发）。
//! 到点由 `MydayAlarmReceiver` 直接发系统通知（alarm 档为全屏闹铃 + 循环铃声），
//! 开机由 `MydayAlarmBootReceiver` 从 SharedPreferences 重排未触发的闹钟。
//!
//! 为什么不做 Rust → Kotlin 直调：调度时机全在「前端活着」的窗口内（用户刚编辑
//! 数据），JS invoke 通路最短且免去 JNI 样板；Rust 侧状态与命令见应用层
//! `reminder_loop::MobileAlarmSync` / `commands::alarms_sync_payload`。

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "dev.myday.alarm";

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("myday-alarm")
        .setup(|_app, api| {
            // 仅 Android 注册 Kotlin 侧；桌面 / iOS 为 no-op
            #[cfg(target_os = "android")]
            {
                api.register_android_plugin(PLUGIN_IDENTIFIER, "MydayAlarmPlugin")?;
            }
            #[cfg(not(target_os = "android"))]
            let _ = api;
            Ok(())
        })
        .build()
}

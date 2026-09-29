//! CLI 提醒通知器（`myday reminders tick` 用，见 docs/REMINDER-DAEMON.md）。
//!
//! GUI 关闭时的调度兜底：systemd timer / Windows 计划任务周期性补跑一轮
//! 提醒检查，这里经 notify-rust 直发系统通知。不带动作按钮——短命进程
//! 等不到 DBus 回调，完成 / 稍后操作请打开应用（ReminderCenter 仍可操作）。

use chrono::{DateTime, Utc};
use myday_core::model::{display_title, Item, Reminder, ReminderKind};
use myday_core::reminder::Notifier;

pub struct CliNotifier;

impl Notifier for CliNotifier {
    fn notify(
        &self,
        reminder: &Reminder,
        item: &Item,
        at: DateTime<Utc>,
        _actions: &[&'static str],
    ) {
        let time = at
            .with_timezone(&chrono::Local)
            .format("%m-%d %H:%M")
            .to_string();
        // builder 方法返回 &mut Self，链式赋值会借用临时值，须逐条语句调用
        // （与桌面 reminder_loop 同一教训）
        let mut n = notify_rust::Notification::new();
        n.appname("MyDay")
            .summary("MyDay 提醒")
            .body(&format!("{}\n{}", display_title(item), time));
        if reminder.kind() == ReminderKind::Alarm {
            n.urgency(notify_rust::Urgency::Critical)
                .timeout(notify_rust::Timeout::Never)
                .sound_name("alarm-clock-elapsed");
        }
        if let Err(e) = n.show() {
            eprintln!("myday: 发送通知失败: {e}");
        }
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let body = lines
            .iter()
            .map(|(t, at)| format!("· {t} {at}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut n = notify_rust::Notification::new();
        n.appname("MyDay")
            .summary(&format!("MyDay：错过 {count} 条提醒"))
            .body(&body);
        if let Err(e) = n.show() {
            eprintln!("myday: 发送通知失败: {e}");
        }
    }
}

//! 后台提醒循环（需求 §9 / SPRINT-SPEC §1）。
//!
//! 每 30 秒执行一轮扫描：到期未发送的提醒 → 按档位（reminders.channel）交付：
//! - notify = 系统通知；alarm = 闹钟（桌面：置顶弹窗 + 循环提示音 + critical
//!   通知；移动端：AlarmManager 全屏闹铃）。文案在本层按界面语言渲染
//! （core 只传结构化数据，见 `i18n` 模块）。
//!
//! 平台分岔（`cfg(desktop)` / `cfg(mobile)`）：
//! - 桌面：进程常驻（关窗隐藏、托盘保活），轮询即调度，notify-rust / DBus 发
//!   通知（动作按钮），alarm 档另起弹窗（`alarm_window`）；
//! - 移动端：进程随时可能被冻结/杀掉，轮询不能当调度器 —— 全部提醒（notify 与
//!   alarm 档）经「调度同步」移交系统 AlarmManager（本地插件 `myday-alarm`）：
//!   到点由系统 BroadcastReceiver 直接发通知/响铃，App 死活无关。本循环只负责
//!   ① 维护应调度集合（未来 24h ∪ 过去 grace 内）并推送前端写入插件；
//!   ② grace 外的错过聚合成摘要；③ 前端同步未确认时的兜底通知。
//!   前端负责运行时申请通知权限。

#[cfg(mobile)]
use std::sync::Mutex;
use std::time::Duration;

use myday_core::model::{display_title, Item, ItemType, Reminder, ReminderKind};
use myday_core::reminder::Notifier;
use myday_core::Store;

use serde::Serialize;

use crate::i18n::{strings, Lang};
use crate::logging;

/// 提醒扫描节拍（秒）。移动端调度同步亦复用该节拍做增量修正（改时间 / 删除 /
/// 完成 → 取消已排定的系统闹钟）。
const TICK_SECS: u64 = 30;

/// 移动端移交系统的前瞻窗口：把未来 24h 内的发生时刻全部交给 AlarmManager。
#[cfg(mobile)]
const SCHEDULE_LOOKAHEAD: chrono::Duration = chrono::Duration::hours(24);

/// 一条交给「闹钟呈现层」/「系统调度器」的提醒（两端共用结构，key 语义一致）。
/// 字段名 camelCase：与 Kotlin 插件 @InvokeArg 属性一一对应。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmSpec {
    /// "{reminder_id}-{at_millis}"：同一次发生时刻的稳定标识（去重 / 取消）
    pub key: String,
    /// 发生时刻（epoch 毫秒，Kotlin AlarmManager 直用）
    pub rid: i64,
    pub at: i64,
    /// notify | alarm
    pub kind: &'static str,
    pub item_id: String,
    pub title: String,
    pub body: String,
}

fn occ_key(reminder_id: i64, at: chrono::DateTime<chrono::Utc>) -> String {
    format!("{reminder_id}-{}", at.timestamp_millis())
}

#[cfg(mobile)]
fn kind_str(kind: ReminderKind) -> &'static str {
    match kind {
        ReminderKind::Notify => "notify",
        ReminderKind::Alarm => "alarm",
    }
}

/// 通知摘要 / 正文的本地化组装（时间格式两侧统一 `MM-DD HH:MM`）。
fn render(lang: Lang, item: &Item, at: chrono::DateTime<chrono::Utc>) -> (String, String) {
    let s = strings(lang);
    let time = at
        .with_timezone(&chrono::Local)
        .format("%m-%d %H:%M")
        .to_string();
    match item.item_type {
        ItemType::Task => (
            s.sum_task
                .replace("{title}", &display_title(item)),
            item.due_at
                .map(|_| s.body_task_due.replace("{time}", &time))
                .unwrap_or_else(|| s.body_task_plain.into()),
        ),
        ItemType::Event => (
            s.sum_event
                .replace("{title}", &display_title(item)),
            item.start_at
                .map(|_| s.body_event_start.replace("{time}", &time))
                .unwrap_or_else(|| s.body_event_plain.into()),
        ),
        ItemType::Log => (
            s.sum_log
                .replace("{title}", &display_title(item)),
            s.body_log.into(),
        ),
    }
}

/// 发生时刻 → AlarmSpec（文案现渲染；语言改动最多延迟一个节拍生效）。
#[cfg(mobile)]
fn alarm_spec(store: &Store, occ: &myday_core::reminder::DueOccurrence) -> AlarmSpec {
    let (title, body) = render(Lang::from_store(store), &occ.item, occ.at);
    AlarmSpec {
        key: occ_key(occ.reminder.id, occ.at),
        rid: occ.reminder.id,
        at: occ.at.timestamp_millis(),
        kind: kind_str(occ.reminder.kind()),
        item_id: occ.item.id.clone(),
        title,
        body,
    }
}

/// 错过聚合摘要正文（两端共用）。
fn missed_body(lang: Lang, count: usize, lines: &[(String, String)]) -> String {
    let s = strings(lang);
    let mut body = String::new();
    for (title, at) in lines {
        body.push_str(&s.missed_line.replace("{title}", title).replace("{at}", at));
        body.push('\n');
    }
    if count > lines.len() {
        body.push_str(
            &s.missed_more
                .replace("{n}", &(count - lines.len()).to_string()),
        );
    }
    body.trim_end().to_string()
}

/// 提醒触发同时广播前端（提醒中心未读角标；通知无法保证携带回调，条目定位兜底）。
fn emit_reminder_fired(app: &tauri::AppHandle, reminder: &Reminder, item: &Item) {
    let _ = tauri::Emitter::emit(
        app,
        "reminder-fired",
        serde_json::json!({ "id": item.id, "reminder_id": reminder.id }),
    );
}

fn spawn_loop(app: tauri::AppHandle, store: std::sync::Arc<Store>) {
    std::thread::Builder::new()
        .name("myday-reminders".into())
        .spawn(move || {
            // 首轮立即执行：webview 挂载时的拉取（alarms_sync_payload）依赖本循环
            // 先把载荷算好 —— 首轮晚于挂载会让冷启动补闹走兜底通知而非系统闹钟。
            loop {
                if let Err(e) = tick(&app, &store) {
                    logging::log(&format!("myday: 提醒循环错误: {e}"));
                }
                std::thread::sleep(Duration::from_secs(TICK_SECS));
            }
        })
        .expect("spawn reminder loop");
}

#[cfg(desktop)]
fn tick(app: &tauri::AppHandle, store: &std::sync::Arc<Store>) -> Result<(), myday_core::MyDayError> {
    let notifier = DesktopNotifier {
        app: app.clone(),
        store: store.clone(),
    };
    myday_core::reminder::tick_once(store, &notifier).map(|_| ())
}

#[cfg(mobile)]
fn tick(app: &tauri::AppHandle, store: &std::sync::Arc<Store>) -> Result<(), myday_core::MyDayError> {
    tick_mobile(app, store)
}

/// 启动提醒循环（两端共用入口；`mobile_setup` / `desktop_setup` 各自调用）。
pub fn spawn(app: tauri::AppHandle, store: std::sync::Arc<Store>) {
    spawn_loop(app, store);
}

// ----------------------------------------------------------------------
// 桌面：notify-rust / DBus 通知（notify 档）+ 闹钟弹窗（alarm 档）
// ----------------------------------------------------------------------

#[cfg(desktop)]
struct DesktopNotifier {
    app: tauri::AppHandle,
    store: std::sync::Arc<Store>,
}

/// 动作 ID → 本地化标签（`default` = 通知本体点击 = 打开定位）。
#[cfg(desktop)]
fn action_label(lang: Lang, id: &str) -> &'static str {
    let s = strings(lang);
    match id {
        "complete" => s.act_complete,
        "snooze" => s.act_snooze,
        _ => s.act_open,
    }
}

#[cfg(desktop)]
impl Notifier for DesktopNotifier {
    fn notify(
        &self,
        reminder: &Reminder,
        item: &Item,
        at: chrono::DateTime<chrono::Utc>,
        actions: &[&'static str],
    ) {
        let lang = Lang::from_store(&self.store);
        let (summary, body) = render(lang, item, at);
        let app = self.app.clone();
        let store = self.store.clone();
        let item_id = item.id.clone();
        let key = occ_key(reminder.id, at);
        let labels: Vec<(String, String)> = actions
            .iter()
            .map(|id| (id.to_string(), action_label(lang, id).to_string()))
            .collect();

        match reminder.kind() {
            ReminderKind::Alarm => {
                // 闹钟档：置顶弹窗 + 前端循环提示音（关窗即停）+ critical 通知兜底
                // （弹窗可能被工作区/全屏窗口遮挡，通知保证面板里有一条常驻项）
                crate::alarm_window::ring(
                    &app,
                    crate::alarm_window::AlarmEntry {
                        key,
                        item_id: item_id.clone(),
                        title: summary.clone(),
                        body: body.clone(),
                    },
                );
                let labels = labels.clone();
                // wait_for_action 会阻塞到通知关闭，必须独立线程，否则卡死提醒环。
                let _ = std::thread::Builder::new()
                    .name("myday-notify".into())
                    .spawn(move || {
                        let mut n = notify_rust::Notification::new();
                        n.appname("MyDay")
                            .summary(&summary)
                            .body(&body)
                            .urgency(notify_rust::Urgency::Critical)
                            .timeout(notify_rust::Timeout::Never)
                            .sound_name("alarm-clock-elapsed");
                        for (id, label) in &labels {
                            n.action(id, label);
                        }
                        n.action("default", action_label(lang, "default"));
                        match n.show() {
                            Ok(handle) => handle.wait_for_action(|action| {
                                dispatch_action(&app, &store, action, &item_id);
                            }),
                            Err(e) => logging::log(&format!("myday: 发送通知失败: {e}")),
                        }
                    });
            }
            ReminderKind::Notify => {
                // wait_for_action 会阻塞到通知关闭，必须独立线程，否则卡死提醒环。
                let _ = std::thread::Builder::new()
                    .name("myday-notify".into())
                    .spawn(move || {
                        // builder 方法返回 &mut Self，链式赋值会借用临时值，须逐条语句调用
                        let mut n = notify_rust::Notification::new();
                        n.appname("MyDay").summary(&summary).body(&body);
                        for (id, label) in &labels {
                            n.action(id, label);
                        }
                        // 通知本体点击 = 打开定位（GNOME 上 default action 由通知体触发）
                        n.action("default", action_label(lang, "default"));
                        match n.show() {
                            Ok(handle) => handle.wait_for_action(|action| {
                                dispatch_action(&app, &store, action, &item_id);
                            }),
                            Err(e) => logging::log(&format!("myday: 发送通知失败: {e}")),
                        }
                    });
            }
        }
        emit_reminder_fired(&self.app, reminder, item);
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let lang = Lang::from_store(&self.store);
        let s = strings(lang);
        let body = missed_body(lang, count, lines);
        if let Err(e) = notify_rust::Notification::new()
            .appname("MyDay")
            .summary(&s.missed_summary.replace("{n}", &count.to_string()))
            .body(&body)
            .show()
        {
            logging::log(&format!("myday: 发送错过摘要失败: {e}"));
        }
    }
}

/// 通知按钮 / 通知体点击的分发（SPRINT-SPEC §1.2）。
/// complete / snooze 直接走 Store（与 CLI `item complete|snooze` 同一函数），
/// default（打开）唤起主窗口并让前端定位到条目详情。
#[cfg(desktop)]
fn dispatch_action(app: &tauri::AppHandle, store: &Store, action: &str, item_id: &str) {
    match action {
        "complete" => {
            if store.complete_task(item_id).is_ok() {
                emit_changed(app);
            }
        }
        "snooze" => {
            let until = chrono::Utc::now() + chrono::Duration::minutes(10);
            if store.snooze(item_id, until).is_ok() {
                emit_changed(app);
            }
        }
        "default" | "open" => {
            crate::show_main(app);
            let _ =
                tauri::Emitter::emit(app, "reminder-fired", serde_json::json!({ "id": item_id }));
        }
        // "closed"（无操作关闭）等其余动作不处理
        _ => {}
    }
}

#[cfg(desktop)]
fn emit_changed(app: &tauri::AppHandle) {
    let _ = tauri::Emitter::emit(app, "data-changed", serde_json::json!({}));
}

// ----------------------------------------------------------------------
// 移动端：全部提醒移交系统 AlarmManager（本地插件 myday-alarm）
// ----------------------------------------------------------------------

/// 前端 → 插件的同步载荷：应调度集合 + 需取消的 keys。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg(mobile)]
pub struct AlarmSyncPayload {
    pub alarms: Vec<AlarmSpec>,
    pub cancel_keys: Vec<String>,
}

/// 移动端调度同步状态（managed；提醒循环写，命令读）。
#[cfg(mobile)]
pub struct MobileAlarmSync {
    /// 当前载荷（`alarms_sync_payload` 命令返回值；前端拉取后写入插件）
    payload: Mutex<AlarmSyncPayload>,
    /// 上次推送的应调度 keys（算取消差集）
    last_keys: Mutex<Vec<String>>,
    /// 前端确认已写入 AlarmManager 的 keys（判定「系统已接管」，跳过兜底通知）
    confirmed: Mutex<std::collections::HashSet<String>>,
    /// 进程内循环起点：兜底通知的等待窗从这里起算
    started: std::time::Instant,
}

/// 冷启动等待窗：首轮 tick 必然早于 webview 挂载同步（进程起来几百毫秒，
/// webview 要 1-3 秒），补发窗口内的过期时刻此时还不能断定「系统没接管」——
/// 等这个窗口结束仍无确认，才走应用内兜底（宁可晚一分钟，不抢系统闹钟的活）。
#[cfg(mobile)]
const CONFIRM_GRACE: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(mobile)]
impl MobileAlarmSync {
    pub fn new() -> Self {
        Self {
            payload: Mutex::new(AlarmSyncPayload::default()),
            last_keys: Mutex::new(Vec::new()),
            confirmed: Mutex::new(std::collections::HashSet::new()),
            started: std::time::Instant::now(),
        }
    }

    /// 前端确认：本次成功写入插件的 keys 全集（覆盖式）。
    pub fn confirm(&self, keys: Vec<String>) {
        *self.confirmed.lock().expect("confirmed poisoned") = keys.into_iter().collect();
    }

    /// 当前载荷快照（`alarms_sync_payload` 命令用）。
    pub fn payload_snapshot(&self) -> AlarmSyncPayload {
        self.payload.lock().expect("payload poisoned").clone()
    }
}

/// 系统通知（兜底 / 错过摘要）：tauri-plugin-notification（无动作按钮，点击即唤起 App）。
#[cfg(mobile)]
struct MobileNotifier {
    app: tauri::AppHandle,
    store: std::sync::Arc<Store>,
}

#[cfg(mobile)]
impl Notifier for MobileNotifier {
    fn notify(
        &self,
        reminder: &Reminder,
        item: &Item,
        at: chrono::DateTime<chrono::Utc>,
        _actions: &[&'static str],
    ) {
        let (summary, body) = render(Lang::from_store(&self.store), item, at);
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(summary)
            .body(body)
            .show()
        {
            logging::log(&format!("myday: 发送通知失败: {e}"));
        }
        emit_reminder_fired(&self.app, reminder, item);
    }

    fn notify_missed(&self, count: usize, lines: &[(String, String)]) {
        let lang = Lang::from_store(&self.store);
        let s = strings(lang);
        let body = missed_body(lang, count, lines);
        use tauri_plugin_notification::NotificationExt;
        if let Err(e) = self
            .app
            .notification()
            .builder()
            .title(s.missed_summary.replace("{n}", &count.to_string()))
            .body(body)
            .show()
        {
            logging::log(&format!("myday: 发送错过摘要失败: {e}"));
        }
    }
}

/// 移动端一轮：
/// 1. 计算应调度集合（未来 24h ∪ 过去 grace 内，未标记、非回收站、非已完成），
///    与上次有差 → 更新载荷并推 `alarms-sync`（前端写入插件后回报 keys）；
/// 2. 到期扫描：grace 外 → 标记 + 聚合摘要；grace 内且系统已接管（confirmed）
///    → 跳过（接收器已按时送达）；grace 内但未确认（webview 没起来 / 同步失败）
///    → 兜底用 tauri-plugin-notification 直发，宁重复不漏报。
#[cfg(mobile)]
fn tick_mobile(
    app: &tauri::AppHandle,
    store: &std::sync::Arc<Store>,
) -> Result<(), myday_core::MyDayError> {
    use tauri::Manager;

    let state = app.state::<MobileAlarmSync>();
    let now = chrono::Utc::now();
    let window = myday_core::reminder::catchup_window(store);

    // 1) 应调度集合 + 变更检测
    let should: Vec<AlarmSpec> = store
        .scheduled_occurrences(now, SCHEDULE_LOOKAHEAD, window)?
        .iter()
        .map(|occ| alarm_spec(store, occ))
        .collect();
    let should_keys: Vec<String> = should.iter().map(|a| a.key.clone()).collect();

    let mut changed = false;
    {
        let mut last = state.last_keys.lock().expect("last_keys poisoned");
        let cancels: Vec<String> = last
            .iter()
            .filter(|k| !should_keys.contains(k))
            .cloned()
            .collect();
        if *last != should_keys || !cancels.is_empty() {
            let mut payload = state.payload.lock().expect("payload poisoned");
            payload.alarms = should.clone();
            payload.cancel_keys = cancels;
            changed = true;
        }
        // 首轮（last 为空且 should 非空）也要推：条件同上已覆盖（keys 不等）
        *last = should_keys;
    }
    if changed {
        let _ = tauri::Emitter::emit(app, "alarms-sync", serde_json::json!({}));
    }

    // 2) 到期扫描（补发 / 跳过 / 错过聚合）
    let due = store.due_reminder_occurrences(now)?;
    let notifier = MobileNotifier {
        app: app.clone(),
        store: store.clone(),
    };
    let mut missed: Vec<(String, String)> = Vec::new();
    let confirmed = state.confirmed.lock().expect("confirmed poisoned");
    // 冷启动等待窗内：webview 大概率还没完成「同步 + 回报」，在应调度集合里的
    // 时刻先让系统闹钟接力（它们马上会随挂载同步交给 AlarmManager），不抢跑
    let awaiting_first_sync = state.started.elapsed() < CONFIRM_GRACE;
    for occ in due {
        let key = occ_key(occ.reminder.id, occ.at);
        if now - occ.at > window {
            // 窗口外：系统已送达的（闹钟按时响过 / 通知按时发过）只静默标记，
            // 不进「错过」摘要；系统没送达的（进程死了整个窗口）才值得补一条聚合
            store.mark_reminded(occ.reminder.id, occ.at)?;
            if !confirmed.contains(&key) && !awaiting_first_sync {
                missed.push((
                    display_title(&occ.item),
                    occ.at
                        .with_timezone(&chrono::Local)
                        .format("%m-%d %H:%M")
                        .to_string(),
                ));
            }
            continue;
        }
        if confirmed.contains(&key) {
            // 接收器已按时发通知/响铃；保持未标记直到滑出 grace（上面处理）
            continue;
        }
        if awaiting_first_sync {
            // 系统侧尚未回报，但也没到放弃等待的时候（挂载同步马上会把它交给系统）
            continue;
        }
        // 兜底：系统侧确认超时（webview 起不来 / 同步持续失败），直发一条保底通知
        notifier.notify(&occ.reminder, &occ.item, occ.at, &[]);
        store.mark_reminded(occ.reminder.id, occ.at)?;
    }
    drop(confirmed);
    if !missed.is_empty() {
        notifier.notify_missed(missed.len(), &missed);
    }
    Ok(())
}

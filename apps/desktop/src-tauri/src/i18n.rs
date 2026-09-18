//! Rust 侧极小 i18n：托盘菜单 / 系统通知等会从这里发出的文案，随界面语言
//! （settings `ui_lang`，前端设置页切换）渲染。前端词典在 `src/lib/i18n/`；
//! core 保持 locale 中立，通知只传结构化数据，文案由本层组装。

use myday_core::Store;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// 每次渲染前现读（改语言即时生效，无需重启提醒环 / 重建托盘之外的东西）。
    pub fn from_store(store: &Store) -> Self {
        match store.get_setting("ui_lang").ok().flatten().as_deref() {
            Some("en") => Self::En,
            _ => Self::Zh,
        }
    }
}

/// Rust 侧会发出的全部文案（静态表，数量少不值得引入格式化框架）。
pub struct Strings {
    // 托盘
    pub tray_open: &'static str,
    pub tray_quick_add: &'static str,
    pub tray_overlay_show: &'static str,
    pub tray_overlay_lock: &'static str,
    pub tray_quit: &'static str,
    // 窗口标题
    pub quickadd_title: &'static str,
    pub overlay_title: &'static str,
    // 通知动作（ID → 标签的渲染在 reminder_loop，按 ID 查这里）
    pub act_open: &'static str,
    pub act_complete: &'static str,
    pub act_snooze: &'static str,
    // 通知摘要 / 正文模板（format! 用）
    pub sum_task: &'static str,
    pub sum_event: &'static str,
    pub sum_log: &'static str,
    pub body_task_due: &'static str,
    pub body_task_plain: &'static str,
    pub body_event_start: &'static str,
    pub body_event_plain: &'static str,
    pub body_log: &'static str,
    // 错过聚合摘要
    pub missed_summary: &'static str,
    pub missed_line: &'static str,
    pub missed_more: &'static str,
}

pub const ZH: Strings = Strings {
    tray_open: "打开 MyDay",
    tray_quick_add: "快速添加…",
    tray_overlay_show: "显示悬浮窗",
    tray_overlay_lock: "悬浮窗锁定（点击穿透）",
    tray_quit: "退出",
    quickadd_title: "快速添加",
    overlay_title: "MyDay 今日",
    act_open: "打开",
    act_complete: "完成",
    act_snooze: "稍后提醒",
    sum_task: "待办：{title}",
    sum_event: "日程：{title}",
    sum_log: "记录：{title}",
    body_task_due: "截止 {time}",
    body_task_plain: "到点提醒",
    body_event_start: "开始 {time}",
    body_event_plain: "即将开始",
    body_log: "提醒时间到",
    missed_summary: "MyDay · 错过 {n} 条提醒",
    missed_line: "• {title} · 原定 {at}",
    missed_more: "…等 {n} 条",
};

pub const EN: Strings = Strings {
    tray_open: "Open MyDay",
    tray_quick_add: "Quick Add…",
    tray_overlay_show: "Show overlay",
    tray_overlay_lock: "Lock overlay (click-through)",
    tray_quit: "Quit",
    quickadd_title: "Quick Add",
    overlay_title: "MyDay Today",
    act_open: "Open",
    act_complete: "Done",
    act_snooze: "Snooze 10 min",
    sum_task: "Task: {title}",
    sum_event: "Event: {title}",
    sum_log: "Log: {title}",
    body_task_due: "Due {time}",
    body_task_plain: "Reminder due",
    body_event_start: "Starts {time}",
    body_event_plain: "Starting soon",
    body_log: "Reminder time",
    missed_summary: "MyDay · {n} missed reminders",
    missed_line: "• {title} · was due {at}",
    missed_more: "…and {n} more",
};

pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::Zh => &ZH,
        Lang::En => &EN,
    }
}

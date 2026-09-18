//! CLI 输出：人类可读（默认）与稳定 JSON 信封（`--json`）。

use myday_core::model::{display_title, Item, ItemStatus, ItemType};
use myday_core::Result;

/// 输出模式：`--json` 时输出稳定 JSON 信封。
#[derive(Debug, Clone, Copy)]
pub struct JsonMode(pub bool);

/// 人类可读单条格式：`✓ tsk_xxx  09-16 14:30  标题  字段=值  #标签`
/// 时间锚点随类型：日程看开始、待办看截止、记录看发生时间。
pub fn human_line(item: &Item) -> String {
    use chrono::TimeZone;
    let time = match item.item_type {
        ItemType::Log => item.occurred_at,
        ItemType::Task => item.due_at.or(item.start_at),
        ItemType::Event => item.start_at,
    }
    .map(|t| {
        chrono::Local
            .from_utc_datetime(&t.naive_utc())
            .format("%m-%d %H:%M")
            .to_string()
    })
    .unwrap_or_else(|| "----------".into());
    let fields = item
        .extra
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, v)| {
                    let v = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    format!("{k}={v}")
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let status = match (item.item_type, item.status) {
        (ItemType::Task, Some(ItemStatus::Done)) => "✓ ",
        _ => "",
    };
    let tags = item
        .tags
        .iter()
        .map(|t| format!("#{t}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "{}{:<10} {} {} {} {}",
        status,
        item.id,
        time,
        display_title(item),
        fields,
        tags
    )
}

/// 输出任意 JSON 值。人类模式下执行 `human` 闭包。
pub fn print_json_envelope(
    mode: JsonMode,
    value: &serde_json::Value,
    human: impl FnOnce(&serde_json::Value),
) -> Result<()> {
    if mode.0 {
        println!("{}", serde_json::json!({ "ok": true, "data": value }));
    } else {
        human(value);
    }
    Ok(())
}

/// 输出单条 Item。
pub fn print_item(mode: JsonMode, item: &Item) -> Result<()> {
    if mode.0 {
        println!(
            "{}",
            serde_json::json!({ "ok": true, "data": item })
        );
    } else {
        println!("{}", human_line(item));
    }
    Ok(())
}

/// 输出列表。空列表在 JSON 模式返回 `[]`，人类模式提示无结果。
pub fn print_list(mode: JsonMode, items: Vec<Item>) -> Result<()> {
    if mode.0 {
        println!("{}", serde_json::json!({ "ok": true, "data": items }));
    } else if items.is_empty() {
        println!("（无结果）");
    } else {
        for item in &items {
            println!("{}", human_line(item));
        }
        println!("共 {} 条", items.len());
    }
    Ok(())
}

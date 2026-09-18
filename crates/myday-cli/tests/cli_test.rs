//! CLI 集成测试：直接运行编译出的 `myday` 二进制，使用独立临时数据目录。
//! 覆盖统一 item 子命令、类型自动识别、校验退出码与删除语义。

use std::process::{Command, Output};
use tempfile::TempDir;

fn myday(temp: &TempDir) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_myday"));
    cmd.env("MYDAY_DATA_DIR", temp.path().join("data"))
        .env("MYDAY_SOCKET_PATH", temp.path().join("s.sock"));
    cmd
}

fn run(cmd: &mut Command) -> Output {
    cmd.output().expect("run myday")
}

fn add_json(temp: &TempDir, args: &[&str]) -> serde_json::Value {
    let mut all = vec!["item", "add"];
    all.extend_from_slice(args);
    all.push("--json");
    let out = run(myday(temp).args(&all));
    assert!(
        out.status.success(),
        "item add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("valid json envelope")
}

fn add_item_id(temp: &TempDir, args: &[&str]) -> String {
    add_json(temp, args)["data"]["id"]
        .as_str()
        .expect("id in envelope")
        .to_string()
}

#[test]
fn detect_type_and_create_all_three() {
    let t = TempDir::new().unwrap();

    // 截止 → task（截止语义优先于开始）
    let v = add_json(&t, &["--title", "准备评审", "--due", "2026-09-20"]);
    assert_eq!(v["data"]["type"], "task");
    assert_eq!(v["data"]["status"], "todo", "待办缺省 status = todo");

    // 开始 → event；结束缺省 +1h
    let v = add_json(&t, &["--title", "评审会", "--start", "2026-09-18T10:00"]);
    assert_eq!(v["data"]["type"], "event");
    assert!(v["data"]["end_at"].is_string());
    assert!(v["data"]["status"].is_null(), "event 无状态");

    // at → log
    let v = add_json(&t, &["--title", "喝了咖啡", "--at", "2026-09-16T08:00"]);
    assert_eq!(v["data"]["type"], "log");
    assert!(v["data"]["occurred_at"].is_string(), "log 带 occurred_at");

    // 裸文本 → task
    let v = add_json(&t, &["--title", "买牛奶"]);
    assert_eq!(v["data"]["type"], "task");

    // 显式类型优先
    let v = add_json(&t, &["--type", "task", "--title", "明说待办"]);
    assert_eq!(v["data"]["type"], "task");
}

#[test]
fn validation_rejections_exit_2() {
    let t = TempDir::new().unwrap();

    // 日程必须带开始时间
    let out = run(myday(&t).args(["item", "add", "--type", "event", "--title", "没开始"]));
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));

    // log 不允许未来发生时间
    let out = run(myday(&t).args([
        "item", "add", "--type", "log", "--title", "穿越", "--at", "2099-01-01T00:00",
    ]));
    assert_eq!(out.status.code(), Some(2));

    // log 需要 title 或 note
    let out = run(myday(&t).args(["item", "add", "--type", "log"]));
    assert_eq!(out.status.code(), Some(2));

    // 未知类型
    let out = run(myday(&t).args(["item", "add", "--type", "memo", "--title", "x"]));
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn idempotency_key_returns_existing() {
    let t = TempDir::new().unwrap();
    let a = add_item_id(&t, &["--title", "买牛奶", "--idempotency-key", "agent-1"]);
    let b = add_item_id(&t, &["--title", "买牛奶", "--idempotency-key", "agent-1"]);
    assert_eq!(a, b, "幂等键命中应返回同一条目");
}

#[test]
fn field_values_use_field_ids() {
    let t = TempDir::new().unwrap();

    // 建字段拿 id
    let out = run(
        myday(&t).args(["field", "add", "--name", "体重", "--kind", "number", "--json"]),
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let fid = v["data"]["id"].as_str().unwrap().to_string();

    // 写值
    let id = add_item_id(&t, &["--type", "log", "--title", "称重", &format!("--field={fid}=70.5")]);
    let out = run(myday(&t).args(["item", "get", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["extra"][&fid], 70.5);

    // 未定义字段被拒（extra 的键必须是字段 id）
    let out = run(
        myday(&t).args(["item", "add", "--type", "log", "--title", "x", "--field", "不存在=1"]),
    );
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn delete_succeeds_then_not_found() {
    let t = TempDir::new().unwrap();
    let id = add_item_id(&t, &["--type", "event", "--title", "客户沟通", "--start", "2026-09-16T10:00"]);

    // 正常删除 → 0，返回被删条目
    let out = run(myday(&t).args(["item", "delete", &id, "--json"]));
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["data"]["id"].as_str().unwrap(), id);

    // 重复删除 → NOT_FOUND(3)
    let out = run(myday(&t).args(["item", "delete", &id]));
    assert_eq!(out.status.code(), Some(3));

    // get 同样 NOT_FOUND(3)
    assert_eq!(run(myday(&t).args(["item", "get", &id])).status.code(), Some(3));
}

#[test]
fn complete_cycle_and_views() {
    let t = TempDir::new().unwrap();
    let id = add_item_id(&t, &["--title", "写周报", "--due", "2026-09-16"]);

    let out = run(myday(&t).args(["item", "complete", &id, "--json"]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["status"], "done");
    assert!(v["data"]["completed_at"].is_string());

    // done 视图可见
    let out = run(myday(&t).args(["item", "list", "--view", "done", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 1);

    // 重开 → todo，completed_at 清空
    let out = run(myday(&t).args(["item", "uncomplete", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["status"], "todo");
    assert!(v["data"]["completed_at"].is_null());

    // doing 已删除：--status doing 被拒绝
    let out = run(myday(&t).args(["item", "update", &id, "--status", "doing", "--json"]));
    assert!(!out.status.success(), "doing 状态已不存在");
}

#[test]
fn log_future_at_and_update_note_only() {
    let t = TempDir::new().unwrap();
    let id = add_item_id(&t, &["--type", "log", "--note", "只有备注"]);

    let out = run(myday(&t).args(["item", "get", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["data"]["title"].is_null(), "log 允许只有 note");
    assert_eq!(v["data"]["note"], "只有备注");

    // 更新发生时间到过去
    let out = run(myday(&t).args(["item", "update", &id, "--at", "2026-09-15T09:00", "--json"]));
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["data"]["occurred_at"].as_str().unwrap_or("").starts_with("2026-09-15"),
        "occurred_at = {}",
        v["data"]["occurred_at"]
    );
}

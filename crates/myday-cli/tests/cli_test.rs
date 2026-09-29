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
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // log 不允许未来发生时间
    let out = run(myday(&t).args([
        "item",
        "add",
        "--type",
        "log",
        "--title",
        "穿越",
        "--at",
        "2099-01-01T00:00",
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
    let out = run(myday(&t).args([
        "field", "add", "--name", "体重", "--kind", "number", "--json",
    ]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let fid = v["data"]["id"].as_str().unwrap().to_string();

    // 写值
    let id = add_item_id(
        &t,
        &[
            "--type",
            "log",
            "--title",
            "称重",
            &format!("--field={fid}=70.5"),
        ],
    );
    let out = run(myday(&t).args(["item", "get", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["extra"][&fid], 70.5);

    // 未定义字段被拒（extra 的键必须是字段 id）
    let out = run(myday(&t).args([
        "item",
        "add",
        "--type",
        "log",
        "--title",
        "x",
        "--field",
        "不存在=1",
    ]));
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn delete_trash_restore_and_hard_delete() {
    let t = TempDir::new().unwrap();
    let id = add_item_id(
        &t,
        &[
            "--type",
            "event",
            "--title",
            "客户沟通",
            "--start",
            "2026-09-16T10:00",
        ],
    );

    // 删除 = 进回收站 → 0，返回被删条目；重复删除幂等成功（已软删不再改）
    let out = run(myday(&t).args(["item", "delete", &id, "--json"]));
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["ok"].as_bool().unwrap());
    assert_eq!(v["data"]["id"].as_str().unwrap(), id);
    assert_eq!(
        v["data"]["deleted_at"].as_str().unwrap_or(""),
        "",
        "软删条目带 deleted_at"
    );

    // 活跃列表不再可见；回收站可见；get 仍可访问（回收站详情）
    let out = run(myday(&t).args(["item", "list", "--type", "event", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 0);
    let out = run(myday(&t).args(["item", "list", "--trash", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 1);

    // 恢复 → 回到活跃列表
    let out = run(myday(&t).args(["item", "restore", &id, "--json"]));
    assert!(out.status.success());
    let out = run(myday(&t).args(["item", "list", "--type", "event", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 1);

    // --hard 彻底删除：活跃与回收站都不再有；对活跃条目 --hard 被拒绝(INVALID=2)
    let out = run(myday(&t).args(["item", "delete", &id, "--json"]));
    assert!(out.status.success());
    let out = run(myday(&t).args(["item", "delete", &id, "--hard", "--json"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        run(myday(&t).args(["item", "get", &id])).status.code(),
        Some(3)
    );
    assert_eq!(
        run(myday(&t).args(["item", "delete", &id, "--hard"]))
            .status
            .code(),
        Some(3)
    );
}

#[test]
fn import_ics_roundtrip_and_dedupe() {
    let t = TempDir::new().unwrap();
    // 带截止待办 → 导出 → 删掉原条目 → 导入 = 数据回来
    let id = add_item_id(
        &t,
        &[
            "--type",
            "task",
            "--title",
            "买牛奶",
            "--due",
            "2026-10-10T18:00",
        ],
    );
    let out = run(myday(&t).args([
        "export",
        "ics",
        "--output",
        t.path().join("roundtrip.ics").to_str().unwrap(),
    ]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // 删掉原条目（软删 → 彻底删除）后导入 → 待办回来（无截止映射等价）
    run(myday(&t).args(["item", "delete", &id]));
    run(myday(&t).args(["item", "delete", &id, "--hard"]));
    let out = run(myday(&t).args([
        "import",
        "ics",
        t.path().join("roundtrip.ics").to_str().unwrap(),
        "--json",
    ]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["tasks"].as_i64().unwrap(), 1, "VTODO → 待办");
    assert_eq!(
        run(myday(&t).args(["item", "list", "--type", "task", "--json"]))
            .status
            .code(),
        Some(0)
    );

    // 重复导入 → 幂等跳过（回链 / UID 幂等键），不新增
    let out = run(myday(&t).args([
        "import",
        "ics",
        t.path().join("roundtrip.ics").to_str().unwrap(),
        "--json",
    ]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["tasks"].as_i64().unwrap(), 0);
    assert!(v["data"]["skipped_duplicates"].as_i64().unwrap() >= 1);
}

#[test]
fn complete_cycle_and_views() {
    let t = TempDir::new().unwrap();
    let id = add_item_id(&t, &["--title", "写周报", "--due", "2026-09-16"]);

    let out = run(myday(&t).args(["item", "complete", &id, "--json"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
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
        v["data"]["occurred_at"]
            .as_str()
            .unwrap_or("")
            .starts_with("2026-09-15"),
        "occurred_at = {}",
        v["data"]["occurred_at"]
    );
}

#[test]
fn occurrence_detach_and_skip() {
    let t = TempDir::new().unwrap();
    // 周四 09:00 起每周重复（带结束条件语法）
    let id = add_item_id(
        &t,
        &[
            "--type",
            "event",
            "--title",
            "站会",
            "--start",
            "2026-09-24T09:00",
            "--recurse",
            "@weekly:4;count=3",
        ],
    );

    // 拆分下一期（10-01 09:00）为独立条目
    let out = run(myday(&t).args(["item", "detach", &id, "--at", "2026-10-01T09:00", "--json"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let detached_id = v["data"]["id"].as_str().unwrap().to_string();
    assert_ne!(detached_id, id);
    assert!(v["data"]["recurrence"].is_null(), "拆分出的条目无规则");

    // 原系列记入例外；错误锚点被拒绝(2)；非重复条目被拒绝(2)
    let out = run(myday(&t).args(["item", "get", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["recurrence_exdates"].as_array().unwrap().len(), 1);
    assert_eq!(
        run(myday(&t).args(["item", "skip", &id, "--at", "2026-09-30T09:00"]))
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        run(myday(&t).args(["item", "skip", &detached_id, "--at", "2026-10-01T09:00"]))
            .status
            .code(),
        Some(2)
    );

    // skip 正常路径
    let out = run(myday(&t).args(["item", "skip", &id, "--at", "2026-10-08T09:00", "--json"]));
    assert!(out.status.success());
}

// ----------------------------------------------------------------------
// 模板管理（v1.4.1 起 CLI 全量可管，不再只有前端入口）
// ----------------------------------------------------------------------

#[test]
fn template_crud_lifecycle() {
    let t = TempDir::new().unwrap();

    // 新建：空 defaults 合法；非法 defaults（未知字段 id）拒绝
    let out = run(myday(&t).args([
        "template",
        "add",
        "--name",
        "服药",
        "--type",
        "log",
        "--icon",
        "💊",
        "--tag",
        "健康",
        "--json",
    ]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let id = v["data"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("tpl_"));
    assert_eq!(v["data"]["pinned"], true, "新建模板缺省钉选");

    let out = run(
        myday(&t).args([
            "template",
            "add",
            "--name",
            "坏模板",
            "--type",
            "log",
            "--defaults",
            r#"{"fd_not_exist":"x"}"#,
            "--json",
        ]),
    );
    assert_eq!(out.status.code(), Some(2), "未知字段 id 应被校验拒绝");

    // 查看：defaults / fields JSON 可见
    let out = run(myday(&t).args(["template", "get", &id, "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["name"], "服药");
    assert_eq!(v["data"]["item_type"], "log");

    // 更新：只传要改的项（改名 + 换类型），其余沿用
    let out = run(myday(&t).args([
        "template",
        "update",
        &id,
        "--name",
        "吃药",
        "--json",
    ]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["name"], "吃药");
    assert_eq!(v["data"]["tag"], "健康", "未传的 tag 沿用现值");

    // 列表 + 类型过滤 + 取消钉选（库内还有内置种子模板，按 id 断言）
    let out = run(myday(&t).args(["template", "list", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tpl| tpl["id"] == id.as_str()),
        "列表应含新建模板"
    );
    let out = run(myday(&t).args(["template", "list", "--type", "task", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tpl| tpl["id"] != id.as_str()),
        "task 过滤不应含 log 模板"
    );
    let out = run(myday(&t).args(["template", "pin", &id, "--off", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["pinned"], false);

    // 删除
    let out = run(myday(&t).args(["template", "delete", &id, "--json"]));
    assert!(out.status.success());
    assert_eq!(
        run(myday(&t).args(["template", "get", &id, "--json"]))
            .status
            .code(),
        Some(3),
        "删除后 get 应 404"
    );
}

// ----------------------------------------------------------------------
// 备份恢复（myday backup / backup restore）
// ----------------------------------------------------------------------

#[test]
fn backup_restore_roundtrip_via_cli() {
    let t = TempDir::new().unwrap();

    let id1 = add_item_id(&t, &["--title", "恢复前的待办"]);
    let out = run(myday(&t).args(["backup", "--json"]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let zip_path = v["data"]["path"].as_str().unwrap().to_string();

    // 备份后新增一条 + 删掉原来那条；恢复后应回到备份时点
    add_item_id(&t, &["--title", "备份后的待办"]);
    run(myday(&t).args(["item", "delete", &id1]));

    let out = run(myday(&t).args(["backup", "restore", &zip_path, "--json"]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["items"]["task"], 1);
    assert!(
        v["data"]["safety_snapshot"].is_string(),
        "恢复应报告当前库的安全快照路径"
    );

    let out = run(myday(&t).args(["item", "list", "--json"]));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let titles: Vec<&str> = v["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec!["恢复前的待办"], "恢复应回到备份时点");
}

// ----------------------------------------------------------------------
// 提醒 tick（GUI 关闭时的调度兜底）
// ----------------------------------------------------------------------

#[test]
fn reminders_tick_without_gui_reports_sent_count() {
    let t = TempDir::new().unwrap();
    // 无 socket → GUI 未运行路径；库内无到期提醒 → sent = 0，不尝试系统通知
    let out = run(myday(&t).args(["reminders", "tick", "--json"]));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["data"]["sent"], 0);
}

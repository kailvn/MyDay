//! 备份 / 恢复集成测试：restore 的往返一致性、非法包与降级拒绝。

use std::io::Write;

use myday_core::backup::{backup_zip, restore_zip};
use myday_core::model::NewItem;
use myday_core::store::Store;

fn open(temp: &tempfile::TempDir) -> Store {
    let root = temp.path().join("data");
    Store::open(&root.join("myday.db"), &root).unwrap()
}

fn add_task(store: &Store, title: &str) {
    store
        .add_item(NewItem {
            title: Some(title.into()),
            ..Default::default()
        })
        .unwrap();
}

fn active_task_count(store: &Store) -> i64 {
    let conn = store.raw_conn().unwrap();
    conn.query_row(
        "SELECT COUNT(*) FROM items WHERE deleted_at IS NULL AND type = 'task'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn restore_roundtrip_recovers_items_and_attachments() {
    let t = tempfile::TempDir::new().unwrap();
    let root = t.path().join("data");

    let store = open(&t);
    add_task(&store, "恢复前的一条");
    // 附件：库存相对路径 attachments/<dir>/<file>
    let att = root.join("attachments").join("itm_test");
    std::fs::create_dir_all(&att).unwrap();
    std::fs::write(att.join("a.png"), b"png-bytes").unwrap();

    let zip_path = backup_zip(&store).unwrap();

    // 备份后继续写入 + 弄乱附件，模拟「数据已被破坏」
    add_task(&store, "备份后多出来的一条");
    std::fs::write(att.join("a.png"), b"corrupted").unwrap();
    drop(store);

    let report = restore_zip(&zip_path, &root).unwrap();
    assert_eq!(report.backup_schema_version, myday_core::store::schema_version());
    assert_eq!(report.items.task, 1, "恢复后应只剩备份时的那一条");
    assert_eq!(report.attachments_restored, 1);
    assert!(report.safety_snapshot.is_some(), "当前库应有安全快照");
    assert!(
        report.previous_attachments.is_some(),
        "原附件目录应整体挪开保留"
    );

    let store = Store::open(&root.join("myday.db"), &root).unwrap();
    assert_eq!(active_task_count(&store), 1);
    let restored = std::fs::read(root.join("attachments").join("itm_test").join("a.png")).unwrap();
    assert_eq!(restored, b"png-bytes", "附件内容应随备份回滚");
    // 挪开的旧附件目录仍在（宁占盘不丢失）
    let aside = report.previous_attachments.unwrap();
    assert_eq!(std::fs::read(aside.join("itm_test").join("a.png")).unwrap(), b"corrupted");
}

#[test]
fn restore_rejects_non_myday_zip() {
    let t = tempfile::TempDir::new().unwrap();
    let root = t.path().join("data");
    std::fs::create_dir_all(&root).unwrap();
    let zip_path = t.path().join("random.zip");
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    writer.start_file("hello.txt", options).unwrap();
    writer.write_all(b"not myday").unwrap();
    writer.finish().unwrap();

    let err = restore_zip(&zip_path, &root).unwrap_err();
    assert!(err.to_string().contains("myday.db"), "应报缺 myday.db：{err}");
}

#[test]
fn restore_rejects_downgrade_and_corrupt_db() {
    let t = tempfile::TempDir::new().unwrap();
    let root = t.path().join("data");
    let store = open(&t);
    add_task(&store, "一条");
    drop(store);

    // 降级：库的 user_version 超过当前程序版本（改在快照副本上，不动活库）
    let future_db = t.path().join("future.db");
    let conn = rusqlite::Connection::open(&root.join("myday.db")).unwrap();
    conn.execute_batch(&format!(
        "VACUUM INTO '{}'",
        future_db.to_string_lossy().replace('\'', "''")
    ))
    .unwrap();
    drop(conn);
    let fconn = rusqlite::Connection::open(&future_db).unwrap();
    fconn
        .pragma_update(None, "user_version", myday_core::store::schema_version() + 1)
        .unwrap();
    drop(fconn);
    let zip_path = t.path().join("future.zip");
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    writer
        .start_file("myday.db", options)
        .unwrap();
    writer
        .write_all(&std::fs::read(&future_db).unwrap())
        .unwrap();
    writer.finish().unwrap();
    let err = restore_zip(&zip_path, &root).unwrap_err();
    assert!(err.to_string().contains("升级"), "应拒绝降级恢复：{err}");

    // 损坏库：尾部截断
    let corrupt = t.path().join("corrupt.zip");
    let file = std::fs::File::create(&corrupt).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    writer.start_file("myday.db", options).unwrap();
    let mut bytes = std::fs::read(&root.join("myday.db")).unwrap();
    let cut = bytes.len() / 2;
    bytes.truncate(cut);
    writer.write_all(&bytes).unwrap();
    writer.finish().unwrap();
    let err = restore_zip(&corrupt, &root).unwrap_err();
    assert!(
        err.to_string().contains("数据库"),
        "应拒绝损坏库（完整性校验或打开失败）：{err}"
    );
}

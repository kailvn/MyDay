//! 一键备份 / 恢复（需求 P1.2 / SPRINT2-SPEC §6）。
//!
//! `VACUUM INTO` 生成数据库一致快照（WAL 活跃时也安全），与 attachments/ 一并
//! 打包为 `backups/myday-backup-YYYYMMDD-HHMMSS.zip`；仅保留最近 7 份自动轮换。
//! 压缩用 STORED（附件多为 PNG 已压缩，db 体积小，换取零重依赖）。
//!
//! 恢复（[`restore_zip`]）：只接受 MyDay 备份包；换入前先校验
//! （integrity_check + schema 版本，拒绝降级 / 非法包），并给当前数据留
//! 双保险（db 快照 + 附件目录整体改名挪开），恢复后打开一次库把旧备份
//! 沿迁移链升到当前版本。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{MyDayError, Result};
use crate::store::Store;

const KEEP_BACKUPS: usize = 7;

/// 生成备份 zip，返回文件路径。
pub fn backup_zip(store: &Store) -> Result<PathBuf> {
    let dir = store.data_root().join("backups");
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let target = dir.join(format!("myday-backup-{stamp}.zip"));
    let snapshot = dir.join(format!(".snapshot-{stamp}.db"));

    // 一致快照（不在事务内执行；失败则中止备份）
    {
        let conn = store.raw_conn()?;
        let quoted = snapshot.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{quoted}'"))?;
    }

    let mut writer = zip::ZipWriter::new(std::fs::File::create(&target)?);
    let options: zip::write::SimpleFileOptions =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("myday.db", options)?;
    let db_bytes = std::fs::read(&snapshot)?;
    writer.write_all(&db_bytes)?;
    // 附件按相对路径原样入包
    let att_root = store.data_root().join("attachments");
    if att_root.is_dir() {
        let mut files = Vec::new();
        collect_files(&att_root, &att_root, &mut files)?;
        for (rel, abs) in files {
            writer.start_file(rel, options)?;
            let bytes = std::fs::read(abs)?;
            writer.write_all(&bytes)?;
        }
    }
    writer.finish()?;
    let _ = std::fs::remove_file(&snapshot);

    rotate(&dir)?;
    Ok(target)
}

/// 递归收集 (zip 内相对路径, 绝对路径)。
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root.parent().unwrap_or(root))
                .unwrap_or(&path);
            out.push((rel.to_string_lossy().replace('\\', "/"), path));
        }
    }
    Ok(())
}

/// 只保留最近 KEEP_BACKUPS 份 myday-backup-*.zip（文件名含时间戳，字典序即时间序）。
fn rotate(dir: &Path) -> Result<()> {
    let mut zips: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("myday-backup-") && n.ends_with(".zip"))
                .unwrap_or(false)
        })
        .collect();
    zips.sort();
    while zips.len() > KEEP_BACKUPS {
        let oldest = zips.remove(0);
        let _ = std::fs::remove_file(oldest);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 恢复
// ---------------------------------------------------------------------------

/// 恢复结果报告（CLI `--json` 直接输出此结构）。
#[derive(Debug, serde::Serialize)]
pub struct RestoreReport {
    /// 备份包内数据库的 schema 版本
    pub backup_schema_version: i64,
    /// 恢复并沿迁移链升级后的 schema 版本（= 当前程序版本）
    pub restored_schema_version: i64,
    /// 恢复后按类型计数（活跃条目，不含回收站）
    pub items: ItemCounts,
    /// 从备份包解出的附件文件数
    pub attachments_restored: usize,
    /// 恢复前当前库的安全快照（可能为 None：原先无库或快照失败）
    pub safety_snapshot: Option<PathBuf>,
    /// 原附件目录整体挪开后的位置（None = 原先没有附件目录）
    pub previous_attachments: Option<PathBuf>,
}

/// 按类型计数。
#[derive(Debug, serde::Serialize)]
pub struct ItemCounts {
    pub event: i64,
    pub task: i64,
    pub log: i64,
}

/// 从备份 zip 恢复数据目录（db + attachments）。
///
/// 前置条件由调用方保证：GUI 未运行（否则 WAL 被另一进程持有，换文件即损坏）。
/// 步骤：解出 db → 校验（integrity_check / items 表 / 拒绝降级）→ 当前库
/// `VACUUM INTO` 快照 → 换入新 db（清掉 -wal/-shm 残迹）→ 附件目录整体改名
/// 挪开（不删除，宁占盘不丢失）→ 解出附件 → 打开一次库完成迁移升级 → 报告。
pub fn restore_zip(zip_path: &Path, root: &Path) -> Result<RestoreReport> {
    std::fs::create_dir_all(root)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let backups_dir = root.join("backups");
    std::fs::create_dir_all(&backups_dir)?;

    // -- 解出 db 到临时文件 -------------------------------------------------
    let file = std::fs::File::open(zip_path)
        .map_err(|e| MyDayError::Invalid(format!("打开备份包失败: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| MyDayError::Invalid(format!("不是有效的 zip 包: {e}")))?;
    let staging = backups_dir.join(format!(".restore-{stamp}.db"));
    {
        let mut entry = archive
            .by_name("myday.db")
            .map_err(|_| MyDayError::Invalid("备份包内没有 myday.db，不是 MyDay 备份".into()))?;
        let mut out = std::fs::File::create(&staging)?;
        std::io::copy(&mut entry, &mut out)?;
    }

    // -- 校验：库完好、像 MyDay 的库、不是来自更新的程序版本 ----------------
    let backup_version = validate_staged_db(&staging)?;

    // -- 当前库安全快照（失败不阻断：损坏库正是要恢复的对象，旧文件随后
    //    也只是被换下而非删除，仍可手工找回） --------------------------------
    let live_db = root.join("myday.db");
    let mut safety_snapshot: Option<PathBuf> = None;
    if live_db.exists() {
        let snap = backups_dir.join(format!("pre-restore-{stamp}.db"));
        match snapshot_db_to(&live_db, &snap) {
            Ok(()) => safety_snapshot = Some(snap),
            Err(e) => eprintln!("myday: 当前库快照失败（{e}），跳过安全备份继续恢复"),
        }
    }

    // -- 换入 db：清掉 WAL 残迹再整体替换 ------------------------------------
    for sidecar in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(root.join(format!("myday.db{sidecar}")));
    }
    if live_db.exists() {
        std::fs::remove_file(&live_db)?;
    }
    std::fs::rename(&staging, &live_db)?;

    // -- 附件：旧目录整体挪开（保留，不删除），再解出包内附件 ----------------
    let att_dir = root.join("attachments");
    let mut previous_attachments: Option<PathBuf> = None;
    if att_dir.is_dir() {
        let aside = root.join(format!("attachments.pre-restore-{stamp}"));
        std::fs::rename(&att_dir, &aside)?;
        previous_attachments = Some(aside);
    }
    let mut attachments_restored = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name == "myday.db" || !name.starts_with("attachments/") {
            continue; // 非附件条目忽略（备份包约定只含 db + attachments/）
        }
        let rel = match Path::new(&name).strip_prefix("attachments/") {
            Ok(r) if is_safe_rel_path(r) => r.to_path_buf(),
            _ => {
                return Err(MyDayError::Invalid(format!(
                    "备份包含不安全的附件路径：{name}"
                )))
            }
        };
        let target = att_dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        attachments_restored += 1;
    }

    // -- 打开一次：沿迁移链升级旧备份 + 播种，同时取计数 ----------------------
    let store = Store::open(&live_db, root)?;
    let counts = {
        let conn = store.raw_conn()?;
        let mut m = ItemCounts { event: 0, task: 0, log: 0 };
        let mut stmt = conn.prepare(
            "SELECT type, COUNT(*) FROM items WHERE deleted_at IS NULL GROUP BY type",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (t, n) = row?;
            match t.as_str() {
                "event" => m.event = n,
                "task" => m.task = n,
                "log" => m.log = n,
                _ => {}
            }
        }
        m
    };
    let restored_version = crate::store::schema_version();

    Ok(RestoreReport {
        backup_schema_version: backup_version,
        restored_schema_version: restored_version,
        items: counts,
        attachments_restored,
        safety_snapshot,
        previous_attachments,
    })
}

/// 校验暂存库：integrity_check 通过、含 items 表、版本可被当前程序接手。
/// 返回备份包内库的 schema 版本。
fn validate_staged_db(staging: &Path) -> Result<i64> {
    let conn = rusqlite::Connection::open_with_flags(
        staging,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| MyDayError::Invalid(format!("备份包内的数据库打不开: {e}")))?;
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| MyDayError::Invalid(format!("备份包内数据库完整性校验失败: {e}")))?;
    if integrity != "ok" {
        return Err(MyDayError::Invalid(format!(
            "备份包内数据库完整性校验失败（{integrity}），拒绝恢复"
        )));
    }
    let items_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'items'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)?;
    if !items_exists {
        return Err(MyDayError::Invalid(
            "备份包内数据库没有 items 表，不是 MyDay 备份".into(),
        ));
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let current = crate::store::schema_version();
    if version > current {
        return Err(MyDayError::Invalid(format!(
            "备份来自 schema v{version}，高于当前程序的 v{current}；请先升级 MyDay 再恢复（降级打开无迁移链可走）"
        )));
    }
    if version < crate::store::migration_baseline() {
        return Err(MyDayError::Invalid(format!(
            "备份来自遗留结构（schema v{version}），只能由原版本程序打开升级后再恢复"
        )));
    }
    Ok(version)
}

/// 对任意库文件做 `VACUUM INTO` 一致快照（restore 的安全网）。
fn snapshot_db_to(live_db: &Path, snapshot: &Path) -> Result<()> {
    let conn = rusqlite::Connection::open(live_db)?;
    let quoted = snapshot.to_string_lossy().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{quoted}'"))?;
    Ok(())
}

/// 附件相对路径防 zip-slip：不允许 `..` 段与绝对路径。
fn is_safe_rel_path(rel: &Path) -> bool {
    !rel.as_os_str().is_empty()
        && !rel.is_absolute()
        && rel
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

//! 一键备份（需求 P1.2 / SPRINT2-SPEC §6）。
//!
//! `VACUUM INTO` 生成数据库一致快照（WAL 活跃时也安全），与 attachments/ 一并
//! 打包为 `backups/myday-backup-YYYYMMDD-HHMMSS.zip`；仅保留最近 7 份自动轮换。
//! 压缩用 STORED（附件多为 PNG 已压缩，db 体积小，换取零重依赖）。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::Result;
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
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
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

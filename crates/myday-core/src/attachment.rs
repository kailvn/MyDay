//! 附件目录管理（需求 §4）。
//!
//! 图片以文件形式保存在 `<数据目录>/attachments/<item_id>/<uuid>.<ext>`，
//! 数据库只存相对路径；不进数据库、不嵌入 ICS。
//! 删除条目时由 [`crate::store::Store::delete_item`] 级联清理。

use std::path::PathBuf;

use crate::error::{MyDayError, Result};
use crate::model::Attachment;
use crate::store::Store;

fn mime_of(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

impl Store {
    /// 保存附件字节流并登记。`ext` 不含点号，如 `png`。
    pub fn add_attachment_bytes(
        &self,
        item_id: &str,
        bytes: &[u8],
        ext: &str,
    ) -> Result<Attachment> {
        // 确认条目存在（外键也会兜底，但提前给出友好错误）
        self.get_item(item_id)?;
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(MyDayError::Invalid(format!("bad attachment extension: {ext}")));
        }
        let file_stem = format!(
            "att_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let rel_path = format!("attachments/{item_id}/{file_stem}.{ext}");
        let abs = self.data_root().join(&rel_path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs, bytes)?;
        let att = Attachment {
            id: 0,
            item_id: item_id.to_string(),
            rel_path: rel_path.clone(),
            mime: mime_of(&ext).to_string(),
            size: bytes.len() as u64,
            created_at: chrono::Utc::now(),
        };
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO attachments (item_id, rel_path, mime, size, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                item_id,
                rel_path,
                att.mime,
                att.size as i64,
                crate::store::dt(att.created_at),
            ],
        )?;
        let mut att = att;
        att.id = conn.last_insert_rowid();
        Ok(att)
    }

    /// 附件绝对路径（备份 / 预览用）。
    pub fn attachment_abs_path(&self, att: &Attachment) -> PathBuf {
        self.data_root().join(&att.rel_path)
    }

    /// 删除单条附件（数据库行 + 文件）。
    pub fn delete_attachment(&self, attachment_id: i64) -> Result<()> {
        let rel: Option<String> = {
            let conn = self.lock()?;
            conn.query_row(
                "SELECT rel_path FROM attachments WHERE id = ?1",
                rusqlite::params![attachment_id],
                |r| r.get(0),
            )
            .optional()?
        };
        let Some(rel) = rel else {
            return Err(MyDayError::NotFound(format!(
                "attachment {attachment_id} not found"
            )));
        };
        let conn = self.lock()?;
        conn.execute(
            "DELETE FROM attachments WHERE id = ?1",
            rusqlite::params![attachment_id],
        )?;
        drop(conn);
        let _ = std::fs::remove_file(self.data_root().join(rel));
        Ok(())
    }

    /// 附件迁移（类型转换用，SPRINT2-SPEC §7）：文件挪到新条目目录 + 行改指向。
    /// 先移文件后改行：中断最多留下指向缺失文件的行（读取侧已容忍）。
    pub fn move_item_attachments(&self, from_id: &str, to_id: &str) -> Result<()> {
        let rows: Vec<(i64, String)> = {
            let conn = self.lock()?;
            let mut stmt =
                conn.prepare("SELECT id, rel_path FROM attachments WHERE item_id = ?1")?;
            let rows = stmt
                .query_map(rusqlite::params![from_id], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let mut moves: Vec<(i64, String)> = Vec::new();
        for (att_id, rel) in rows {
            let Some(name) = rel.rsplit('/').next() else { continue };
            let new_rel = format!("attachments/{to_id}/{name}");
            let from_abs = self.data_root().join(&rel);
            let to_abs = self.data_root().join(&new_rel);
            if let Some(parent) = to_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if from_abs.exists() {
                std::fs::rename(&from_abs, &to_abs)?;
            }
            moves.push((att_id, new_rel));
        }
        if moves.is_empty() {
            return Ok(());
        }
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction()?;
        for (att_id, new_rel) in moves {
            tx.execute(
                "UPDATE attachments SET item_id = ?2, rel_path = ?3 WHERE id = ?1",
                rusqlite::params![att_id, to_id, new_rel],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 附件复制（日程生成记录用）：文件复制成新条目独立副本 + 新行，原条目不动。
    pub fn copy_item_attachments(&self, from_id: &str, to_id: &str) -> Result<()> {
        let rows: Vec<(String, String, i64)> = {
            let conn = self.lock()?;
            let mut stmt = conn
                .prepare("SELECT rel_path, mime, size FROM attachments WHERE item_id = ?1")?;
            let rows = stmt
                .query_map(rusqlite::params![from_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let now = crate::store::dt(chrono::Utc::now());
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction()?;
        for (rel, mime, size) in rows {
            let Some(ext) = rel.rsplit('.').next() else { continue };
            let stem = format!("att_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
            let new_rel = format!("attachments/{to_id}/{stem}.{ext}");
            let from_abs = self.data_root().join(&rel);
            let to_abs = self.data_root().join(&new_rel);
            if from_abs.exists() {
                if let Some(parent) = to_abs.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&from_abs, &to_abs)?;
            } else {
                continue; // 源文件已缺失：跳过（不登记幽灵行）
            }
            tx.execute(
                "INSERT INTO attachments (item_id, rel_path, mime, size, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![to_id, new_rel, mime, size, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

use rusqlite::OptionalExtension;

/// 清理条目附件目录（条目删除后若目录为空则移除）。
pub(crate) fn prune_item_dir(store: &Store, item_id: &str) {
    let dir = store.data_root().join("attachments").join(item_id);
    let _ = std::fs::remove_dir(&dir); // 仅空目录可删，失败忽略
}

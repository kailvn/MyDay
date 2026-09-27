//! 局域网同步（v1，无云服务器，非实时）：条目级「最后写入胜出」+ 冲突副本。
//!
//! 设计定稿（2026-09 讨论，硬约束：任何失败模式不得丢内容）：
//! - 协议两步、无游标无状态：①全量索引交换（服务端纯读）②按需推送完整条目；
//!   任意一步失败/中断，下次同步原样重跑即收敛（幂等）。
//! - 合并规则两端共用同一实现（本文件），桌面服务端与手机客户端无逻辑漂移。
//! - LWW 覆盖存活条目前，若**本地自上次同步后被改过**且内容与来者不同，先落
//!   「同步冲突副本」（完整克隆含提醒/标签，打 `同步冲突` 标签）再覆盖——
//!   败者的内容永不消失。
//! - 删除只以墓碑传播（`deleted_at`），被删条目进回收站走 30 天 GC；同步代码
//!   永不硬删。附件行在本设备替换时原位保留（文件本就各机一份，不随条目同步）。
//! - LWW 与 `sync.last_sync_at` 都依赖两端时钟大致准确（NTP 默认开启），文档注明。

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::MyDayError;
use crate::model::{new_id, FieldDef, Item, Template};
use crate::store::{dt, exdates_json, item_mapper, opt_dt, parse_dt, Store};

type Result<T> = std::result::Result<T, MyDayError>;

/// 同步服务固定端口（桌面监听 / 手机直连或网关探测）。
pub const SYNC_PORT: u16 = 26136;

/// settings 键：本机上次完成同步的时刻（LWW 冲突判定基准）。
const LAST_SYNC_KEY: &str = "sync.last_sync_at";

// ----------------------------------------------------------------------
// 协议类型（serde 即协议；桌面 axum 与手机 reqwest 共用）
// ----------------------------------------------------------------------

/// 索引条目：合并比较只看这三个字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncIndexEntry {
    pub id: String,
    pub updated_at: DateTime<Utc>,
    pub deleted: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncIndexReq {
    pub items: Vec<SyncIndexEntry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncIndexResp {
    /// 服务器比客户端新（或客户端没有）的完整条目，含墓碑。
    pub to_client: Vec<Item>,
    /// 客户端比服务器新的 id 清单，等 push 阶段送全量。
    pub want: Vec<String>,
    /// 配置随索引单向对齐（桌面权威 → 手机）：模板全量 + 活跃字段定义。
    /// None = 对端是旧版本、不下发配置（跳过对齐，保留本地种子）；
    /// Some(空) = 桌面真的清空了（手机跟着清）。手机端只消费不生产。
    #[serde(default)]
    pub templates: Option<Vec<Template>>,
    #[serde(default)]
    pub field_defs: Option<Vec<FieldDef>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncPushReq {
    pub items: Vec<Item>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SyncPushResp {
    pub applied: usize,
    pub conflicts: usize,
}

// ----------------------------------------------------------------------
// Store 扩展：索引 / 载荷 / 合并
// ----------------------------------------------------------------------

impl Store {
    /// 全量索引（含墓碑）。个人库量级（几千条 × 40B）可整包交换，无需游标。
    pub fn sync_list_index(&self) -> Result<Vec<SyncIndexEntry>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, updated_at, deleted_at FROM items")?;
        let rows = stmt.query_map([], |row| {
            let updated_at: Option<String> = row.get(1)?;
            let deleted_at: Option<String> = row.get(2)?;
            Ok(SyncIndexEntry {
                id: row.get(0)?,
                updated_at: updated_at
                    .as_deref()
                    .and_then(parse_dt)
                    .unwrap_or_else(Utc::now),
                deleted: deleted_at.is_some(),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(MyDayError::from)
    }

    /// 按_id_批量取完整条目（**含墓碑**——删除也要传播；附件行照读，仅在拥有它的
    /// 设备上有意义）。整批查询 + [`Store::hydrate_many`]，不逐条。
    pub fn sync_get_items(&self, ids: &[String]) -> Result<Vec<Item>> {
        let conn = self.lock()?;
        load_items_full(&conn, ids)
    }

    /// 会话收尾：记录本机「上次完成同步」时刻（秒精度，与条目 updated_at 同口径）。
    pub fn sync_mark_done(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![LAST_SYNC_KEY, dt(Utc::now())],
        )?;
        Ok(())
    }

    /// 阶段①：对比客户端索引，产出（发给客户端的差异，向客户端索要的 id 清单）。
    /// 纯读不写——任何时刻中断都无副作用。
    pub fn sync_handle_index(&self, req: &SyncIndexReq) -> Result<SyncIndexResp> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, updated_at FROM items")?;
        let local: std::collections::HashMap<String, DateTime<Utc>> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?
                        .as_deref()
                        .and_then(parse_dt)
                        .unwrap_or_else(Utc::now),
                ))
            })?
            .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()?;

        let mut resp = SyncIndexResp::default();
        let mut want_ids = Vec::new();
        let mut send_ids = Vec::new();
        for entry in &req.items {
            match local.get(&entry.id) {
                None => want_ids.push(entry.id.clone()),
                // 客户端比服务器新 → 服务器缺这个更新,等 push
                Some(t) if *t < entry.updated_at => want_ids.push(entry.id.clone()),
                _ => {}
            }
        }
        for (id, t) in &local {
            match req.items.iter().find(|e| &e.id == id) {
                None => send_ids.push(id.clone()),
                Some(e) if *t > e.updated_at => send_ids.push(id.clone()),
                _ => {}
            }
        }
        drop(stmt);
        resp.want = want_ids;
        // 待发条目一次整批取出（对比索引判定是纯集合运算，读行走批量）
        if !send_ids.is_empty() {
            resp.to_client = load_items_full(&conn, &send_ids)?;
        }
        // 配置随索引下发（桌面权威，手机端 sync_apply_config 对齐）
        resp.templates = Some(
            conn.prepare(
                "SELECT id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields \
                 FROM templates ORDER BY sort ASC, name ASC",
            )?
            .query_map([], crate::store::template_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        );
        resp.field_defs = Some(
            conn.prepare(
                "SELECT id, name, kind, options, scope, sort, builtin FROM field_defs \
                 WHERE deleted_at IS NULL ORDER BY sort ASC, name ASC",
            )?
            .query_map([], crate::store::field_def_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        );
        Ok(resp)
    }

    /// 配置对齐（模板 + 活跃字段定义；桌面权威 → 手机单向）：给到的整行 REPLACE，
    /// 本地不在给到集合里的行删除。幂等——重复应用无副作用。
    pub fn sync_apply_config(&self, templates: &[Template], field_defs: &[FieldDef]) -> Result<()> {
        let conn = self.lock()?;
        let tx = conn.unchecked_transaction()?;

        for t in templates {
            tx.execute(
                "INSERT OR REPLACE INTO templates (id, name, tag, defaults, note, sort, builtin, icon, item_type, pinned, fields) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    t.id,
                    t.name,
                    t.tag,
                    t.defaults.to_string(),
                    t.note,
                    t.sort,
                    t.builtin as i64,
                    t.icon,
                    t.item_type.as_str(),
                    t.pinned as i64,
                    t.fields.to_string(),
                ],
            )?;
        }
        replace_guard(&tx, "templates", &templates.iter().map(|t| t.id.clone()).collect::<Vec<_>>())?;

        for f in field_defs {
            tx.execute(
                "INSERT OR REPLACE INTO field_defs (id, name, kind, options, scope, sort, builtin) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    f.id,
                    f.name,
                    f.kind.as_str(),
                    f.options.to_string(),
                    f.scope.as_ref().map(|t| t.as_str()).unwrap_or("all"),
                    f.sort,
                    f.builtin as i64,
                ],
            )?;
        }
        replace_guard(&tx, "field_defs", &field_defs.iter().map(|f| f.id.clone()).collect::<Vec<_>>())?;

        tx.commit()?;
        Ok(())
    }

    /// 阶段②：应用对端推来的条目（服务端收 push、客户端收 to_client 共用）。
    /// 读取/回写 `sync.last_sync_at` 作为「本地自上次同步后是否改过」的判定基准。
    /// 每条目独立事务：任意中断留下的一半在下次同步自然收敛。
    pub fn sync_apply_incoming(&self, items: &[Item], record_sync: bool) -> Result<SyncPushResp> {
        let since: Option<DateTime<Utc>> = {
            let conn = self.lock()?;
            conn.query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![LAST_SYNC_KEY],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .as_deref()
            .and_then(parse_dt)
        };
        let resp = self.sync_apply_incoming_since(items, since)?;
        if record_sync {
            let conn = self.lock()?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![LAST_SYNC_KEY, dt(Utc::now())],
            )?;
        }
        Ok(resp)
    }

    /// 合并纯函数:`since` = 本机上次完成同步的时刻(秒精度)。暴露成参数
    /// 便于确定性单测;真实入口 [`Self::sync_apply_incoming`] 从 settings 读。
    /// 本地现状整批预读（读不逐条），写入仍每条目独立事务——
    /// 任意中断留下的一半在下次同步自然收敛。
    pub fn sync_apply_incoming_since(
        &self,
        items: &[Item],
        since: Option<DateTime<Utc>>,
    ) -> Result<SyncPushResp> {
        let since = since.unwrap_or(DateTime::<Utc>::MIN_UTC);

        let mut applied = 0usize;
        let mut conflicts = 0usize;
        let conn = self.lock()?;
        let ids: Vec<String> = items.iter().map(|i| i.id.clone()).collect();
        let mut locals: std::collections::HashMap<String, Item> = load_items_full(&conn, &ids)?
            .into_iter()
            .map(|it| (it.id.clone(), it))
            .collect();
        for incoming in items {
            let tx = conn.unchecked_transaction()?;
            match locals.get(&incoming.id) {
                None => {
                    sync_upsert(&tx, incoming)?;
                    applied += 1;
                }
                Some(local) => {
                    // 幂等跳过（含相等）：响应丢失后的重试安全网
                    if incoming.updated_at <= local.updated_at {
                        tx.commit()?;
                        continue;
                    }
                    // 冲突副本：本地存活、来者存活、内容不同、且本地在上次同步后改过
                    //（= 两端真的同时动过同一条）。对端单方编辑不产生副本。
                    if local.deleted_at.is_none()
                        && incoming.deleted_at.is_none()
                        && local.updated_at > since
                        && !same_content(local, incoming)
                    {
                        insert_conflict_copy(&tx, local)?;
                        conflicts += 1;
                    }
                    sync_upsert(&tx, incoming)?;
                    applied += 1;
                }
            }
            tx.commit()?;
            // 缓存同步到已写状态：同批重复 id 与「写后再读」口径一致
            locals.insert(incoming.id.clone(), incoming.clone());
        }

        Ok(SyncPushResp { applied, conflicts })
    }
}

/// 批量完整读取（持锁环境自由函数）：items 行分块整取 + [`Store::hydrate_many`]
/// 批量附属，共 4 次查询量级。对端索引口误 / 已被 GC 的 id 自然缺席。
fn load_items_full(conn: &Connection, ids: &[String]) -> Result<Vec<Item>> {    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = vec!["?"; chunk.len()].join(",");
        let mut stmt =
            conn.prepare(&format!("SELECT * FROM items WHERE id IN ({placeholders})"))?;
        let items = stmt
            .query_map(rusqlite::params_from_iter(chunk), item_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        out.extend(items);
    }
    Store::hydrate_many(conn, &mut out)?;
    Ok(out)
}

/// 配置对齐的删除护栏：给到的 id 集合之外全删（空集合 = 全删——桌面清空即手机清空）。
fn replace_guard(conn: &Connection, table: &str, keep: &[String]) -> Result<()> {
    if keep.is_empty() {
        conn.execute(&format!("DELETE FROM {table}"), [])?;
        return Ok(());
    }
    let placeholders = vec!["?"; keep.len()].join(",");
    conn.execute(
        &format!("DELETE FROM {table} WHERE id NOT IN ({placeholders})"),
        rusqlite::params_from_iter(keep),
    )?;
    Ok(())
}

/// 内容比较（排除 updated_at 与设备本地的附件行——附件不随条目同步）。
fn same_content(a: &Item, b: &Item) -> bool {
    a.id == b.id
        && a.item_type == b.item_type
        && a.title == b.title
        && a.note == b.note
        && a.start_at == b.start_at
        && a.end_at == b.end_at
        && a.all_day == b.all_day
        && a.due_at == b.due_at
        && a.due_all_day == b.due_all_day
        && a.occurred_at == b.occurred_at
        && a.status == b.status
        && a.completed_at == b.completed_at
        && a.recurrence == b.recurrence
        && a.recurrence_exdates == b.recurrence_exdates
        && a.reminders == b.reminders
        && a.tags == b.tags
        && a.extra == b.extra
}

/// 单条落库（插入或整行替换），单事务内调用。
/// - REPLACE 级联清掉旧提醒 / 标签关联 / 附件行，随后整组重建提醒与标签；
/// - 附件行是设备本地数据，**原位保留**（文件不随条目同步）；
/// - `idempotency_key` 若已被其他条目占用则置空——REPLACE 遇 UNIQUE 冲突会
///   连带删除撞键行，必须防住。
fn sync_upsert(conn: &Connection, it: &Item) -> Result<()> {
    let key_free: bool = match it.idempotency_key.as_deref() {
        Some(k) if !k.is_empty() => {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM items WHERE idempotency_key = ?1 AND id != ?2",
                params![k, it.id],
                |r| r.get(0),
            )?;
            n == 0
        }
        _ => false,
    };
    let key = if key_free { it.idempotency_key.clone() } else { None };

    // 附件行保真：REPLACE 级联删除前读出，插回原行（id 不变，文件路径照旧）
    let mut att_stmt = conn.prepare(
        "SELECT id, item_id, rel_path, mime, size, created_at FROM attachments WHERE item_id = ?1",
    )?;
    let atts = att_stmt
        .query_map(params![it.id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(att_stmt);

    conn.execute(
        "INSERT OR REPLACE INTO items (id, type, title, note, start_at, end_at, all_day,
                                due_at, due_all_day, occurred_at, status, completed_at,
                                recurrence, recurrence_exdates, template_id, idempotency_key,
                                deleted_at, extra, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            it.id,
            it.item_type.as_str(),
            it.title,
            it.note,
            opt_dt(it.start_at),
            opt_dt(it.end_at),
            it.all_day as i64,
            opt_dt(it.due_at),
            it.due_all_day as i64,
            opt_dt(it.occurred_at),
            it.status.map(|s| s.as_str()),
            opt_dt(it.completed_at),
            it.recurrence,
            exdates_json(&it.recurrence_exdates),
            it.template_id,
            key,
            opt_dt(it.deleted_at),
            serde_json::to_string(&it.extra).unwrap_or_else(|_| "{}".into()),
            dt(it.created_at),
            dt(it.updated_at),
        ],
    )?;

    conn.execute("DELETE FROM reminders WHERE item_id = ?1", params![it.id])?;
    for r in &it.reminders {
        conn.execute(
            "INSERT INTO reminders (item_id, spec, channel) VALUES (?1, ?2, ?3)",
            params![it.id, r.spec, r.channel],
        )?;
    }

    for raw in &it.tags {
        let tag = raw.trim().trim_start_matches('#');
        if tag.is_empty() {
            continue;
        }
        conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![tag])?;
        let tag_id: i64 =
            conn.query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| r.get(0))?;
        conn.execute(
            "INSERT OR IGNORE INTO item_tags (item_id, tag_id) VALUES (?1, ?2)",
            params![it.id, tag_id],
        )?;
    }

    for (id, item_id, rel_path, mime, size, created_at) in atts {
        conn.execute(
            "INSERT OR REPLACE INTO attachments (id, item_id, rel_path, mime, size, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, item_id, rel_path, mime, size, created_at],
        )?;
    }
    Ok(())
}

/// 冲突副本：原条目完整克隆（含提醒 / 标签 / 附件行引用），打 `同步冲突` 标签。
/// 就是普通条目——可见、可搜索、进回收站，无需任何特殊 UI。
fn insert_conflict_copy(conn: &Connection, local: &Item) -> Result<()> {
    let mut copy = local.clone();
    copy.id = new_id(local.item_type);
    copy.title = local
        .title
        .clone()
        .map(|t| format!("{t}（同步冲突副本）"))
        .or_else(|| Some("（同步冲突副本）".into()));
    copy.idempotency_key = None;
    copy.tags.push("同步冲突".into());
    copy.updated_at = Utc::now();
    sync_upsert(conn, &copy)
}

// ----------------------------------------------------------------------
// 测试：失败注入四场景（内容保全断言）
// ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::SubsecRound;
    use crate::model::{ItemType, NewItem};

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("myday.db"), dir.path()).unwrap();
        (s, dir)
    }

    fn now() -> DateTime<Utc> {
        Utc::now().trunc_subsecs(0)
    }

    fn add(s: &Store, title: &str, at: DateTime<Utc>) -> Item {
        s.add_item(NewItem {
            item_type: Some(ItemType::Log),
            title: Some(title.into()),
            occurred_at: Some(at),
            ..Default::default()
        })
        .unwrap()
    }

    fn edited(item: &Item, title: &str, at: DateTime<Utc>) -> Item {
        let mut it = item.clone();
        it.title = Some(title.into());
        it.updated_at = at;
        it
    }

    fn titles(s: &Store) -> Vec<String> {
        s.list_items(&Default::default())
            .unwrap()
            .into_iter()
            .map(|i| i.title.unwrap_or_default())
            .collect()
    }

    /// 把条目 updated_at 回拨 secs：测试里制造与「现在」的秒级间隔（秒精度下防同秒抖动）
    fn backdate(s: &Store, id: &str, secs: i64) {
        let it = s.get_item(id).unwrap();
        let conn = s.raw_conn().unwrap();
        conn.execute(
            "UPDATE items SET updated_at = ?1 WHERE id = ?2",
            params![dt(it.updated_at - chrono::Duration::seconds(secs)), id],
        )
        .unwrap();
    }

    /// 两端同时编辑同一条(基线后本地也改过 + 对端送来更新):
    /// LWW 取新者,本地败者必须以冲突副本保全
    #[test]
    fn conflict_copy_preserves_loser() {
        let (s, _dir) = store();
        let base = add(&s, "原始", now());
        // 基线显式设为建库之前:此后本地真实编辑必然晚于基线(秒精度下确定性成立)
        let since = Some(base.updated_at - chrono::Duration::seconds(5));

        // 本地真实编辑(update_item 会写当前时刻,必然晚于基线)
        s.update_item(
            &base.id,
            crate::model::ItemPatch {
                title: Some("本地改".into()),
                ..Default::default()
            },
        )
        .unwrap();

        // 对端基于旧版本编辑,时刻更新 → 真冲突
        let mut remote = base.clone();
        remote.title = Some("手机改的".into());
        remote.updated_at = now() + chrono::Duration::seconds(30);
        let resp = s.sync_apply_incoming_since(&[remote], since).unwrap();
        assert_eq!(resp.applied, 1);
        assert_eq!(resp.conflicts, 1);

        let all = titles(&s);
        assert!(all.iter().any(|t| t == "手机改的"), "新版本在场");
        assert!(
            all.iter().any(|t| t == "本地改（同步冲突副本）"),
            "本地败者以副本保全"
        );
        let items = s.list_items(&Default::default()).unwrap();
        assert!(
            items
                .iter()
                .any(|i| i.tags.contains(&"同步冲突".into())),
            "副本带同步冲突标签"
        );
    }

    /// 对端单方编辑（本地自上次同步未动）：干净应用，不产生副本噪音
    #[test]
    fn one_sided_edit_no_copy() {
        let (s, _dir) = store();
        let base = add(&s, "桌面建", now());
        s.sync_apply_incoming(std::slice::from_ref(&base), true).unwrap();

        // 本地未再改动（updated_at 仍等于基线时刻），对端送来更新
        let newer = edited(&base, "对端编辑", now() + chrono::Duration::seconds(5));
        let resp = s.sync_apply_incoming(&[newer], true).unwrap();
        assert_eq!(resp.applied, 1);
        assert_eq!(resp.conflicts, 0);
        assert_eq!(titles(&s).len(), 1);
    }

    /// 墓碑传播：删除以软删形式到达，进回收站可恢复，绝不硬删
    #[test]
    fn tombstone_goes_to_trash() {
        let (s, _dir) = store();
        let base = add(&s, "要被删的", now());
        s.sync_apply_incoming(std::slice::from_ref(&base), true).unwrap();

        let mut tomb = base.clone();
        tomb.updated_at = now() + chrono::Duration::seconds(5);
        tomb.deleted_at = Some(tomb.updated_at);
        let resp = s.sync_apply_incoming(&[tomb], true).unwrap();
        assert_eq!(resp.applied, 1);

        assert!(s.list_items(&Default::default()).unwrap().is_empty(), "活跃列表不含");
        let trash = s.list_trash().unwrap();
        assert_eq!(trash.len(), 1, "墓碑条目在回收站");
        let restored = s.restore_item(&base.id).unwrap();
        assert_eq!(restored.title.as_deref(), Some("要被删的"));
    }

    /// 本地比墓碑新的编辑存活（对端的删除被本地更新打败），且对端编辑不丢
    #[test]
    fn local_edit_beats_remote_tombstone() {
        let (s, _dir) = store();
        let base = add(&s, "同源", now());
        s.sync_apply_incoming(std::slice::from_ref(&base), true).unwrap();

        // 本地在基线后编辑（冲突路径）
        s.update_item(&base.id, crate::model::ItemPatch {
            title: Some("本地改".into()),
            ..Default::default()
        })
        .unwrap();

        // 对端(基于旧状态)删除,墓碑时刻 = 基线时刻,必然早于本地编辑 → 被拒
        let mut tomb = base.clone();
        tomb.deleted_at = Some(base.updated_at);
        let resp = s.sync_apply_incoming(&[tomb], true).unwrap();
        assert_eq!(resp.applied, 0, "墓碑更旧，被拒绝");
        let live = s.list_items(&Default::default()).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].title.as_deref(), Some("本地改"));
    }

    /// 中断重跑:同一批推送重复应用,幂等收敛,无重复无副本
    #[test]
    fn reapply_is_idempotent() {
        let (s, _dir) = store();
        let base = add(&s, "条目", now());
        s.sync_apply_incoming(std::slice::from_ref(&base), true).unwrap();

        let batch = vec![edited(&base, "改一次", now() + chrono::Duration::seconds(1))];
        let first = s.sync_apply_incoming(&batch[..1], false).unwrap();
        let second = s.sync_apply_incoming(&batch, false).unwrap();
        assert_eq!(first.applied, 1);
        assert_eq!(first.conflicts, 0, "本地自基线未动,无副本");
        assert_eq!(second.applied, 0, "重跑幂等跳过");
        assert_eq!(second.conflicts, 0);
        assert_eq!(titles(&s).len(), 1);
    }

    /// 删除/恢复必须推进 updated_at——LWW 只认时间戳,不推进的变化
    /// 在对端索引对比里是「无变化」,墓碑/复活永远传不过去(真机踩过的坑)。
    #[test]
    fn delete_and_restore_advance_updated_at() {
        let (s, _dir) = store();
        let base = add(&s, "删了又恢复", now());
        backdate(&s, &base.id, 10);
        let base = s.get_item(&base.id).unwrap();

        s.delete_item(&base.id).unwrap();
        let idx = s.sync_list_index().unwrap();
        let e = idx.iter().find(|e| e.id == base.id).unwrap();
        assert!(e.deleted, "删除后进回收站(墓碑)");
        assert!(e.updated_at > base.updated_at, "删除必须推进 updated_at");

        s.restore_item(&base.id).unwrap();
        let idx = s.sync_list_index().unwrap();
        let e = idx.iter().find(|e| e.id == base.id).unwrap();
        assert!(!e.deleted, "恢复后回到活跃列表");
        assert!(e.updated_at > base.updated_at, "恢复也必须推进 updated_at");
    }

    /// 端到端删除传播：本地删 + 对端持旧活版本 → 索引判定对端要墓碑 →
    /// 墓碑在对端落成回收站条目（不是悄悄消失）。
    #[test]
    fn delete_propagates_via_sync() {
        let (phone, _d1) = store();
        let (desktop, _d2) = store();
        let base = add(&phone, "同步后删除", now());
        backdate(&phone, &base.id, 10);
        let base = phone.get_item(&base.id).unwrap();

        // 桌面先有一份(首次同步)
        desktop
            .sync_apply_incoming(std::slice::from_ref(&base), true)
            .unwrap();

        // 手机删除 → 二次同步:桌面索引判定客户端新 → 拉墓碑 → 进回收站
        phone.delete_item(&base.id).unwrap();
        let index = phone.sync_list_index().unwrap();
        let resp = desktop.sync_handle_index(&SyncIndexReq { items: index }).unwrap();
        assert!(resp.want.contains(&base.id), "对端必须索要墓碑");
        let tomb: Vec<Item> = {
            let ids = resp.want.clone();
            phone.sync_get_items(&ids).unwrap()
        };
        let resp2 = desktop
            .sync_apply_incoming_since(&tomb, None)
            .unwrap();
        assert_eq!(resp2.applied, 1);
        let trash = desktop.list_trash().unwrap();
        assert!(trash.iter().any(|i| i.id == base.id), "桌面端进回收站");
    }

    /// 配置对齐：模板/字段定义整包 REPLACE + 缺失行删除（含建库即有的内置种子），
    /// 重复应用幂等；桌面清空 → 手机清空。
    #[test]
    fn config_alignment_mirrors_desktop() {
        let (phone, _d1) = store();
        assert!(
            !phone.list_templates().unwrap().is_empty(),
            "新库有内置种子模板,对齐后应被桌面集合替换"
        );
        let tpl = Template {
            id: "tpl_weight".into(),
            name: "体重".into(),
            tag: None,
            icon: Some("⚖".into()),
            item_type: ItemType::Log,
            defaults: serde_json::json!({"title": "体重"}),
            fields: serde_json::json!([{"id": "fd_w", "name": "体重", "kind": "number", "scope": "log"}]),
            note: None,
            sort: 0,
            pinned: true,
            builtin: true,
        };
        let fd = FieldDef {
            id: "fd_w".into(),
            name: "体重(kg)".into(),
            kind: crate::model::FieldKind::Number,
            options: serde_json::json!({"unit": "kg"}),
            scope: Some(ItemType::Log),
            sort: 0,
            builtin: true,
        };
        phone.sync_apply_config(std::slice::from_ref(&tpl), std::slice::from_ref(&fd)).unwrap();
        let tpls = phone.list_templates().unwrap();
        assert_eq!(tpls.len(), 1);
        assert_eq!(tpls[0].name, "体重");
        assert!(phone.list_field_defs(None).unwrap().iter().any(|f| f.id == "fd_w"));
        // 索引响应携带配置（Some = 对端是新版本；None 才是旧版本跳过对齐）
        let resp = phone.sync_handle_index(&SyncIndexReq::default()).unwrap();
        assert_eq!(resp.templates.as_ref().unwrap().len(), 1);
        assert_eq!(resp.field_defs.as_ref().unwrap().len(), 1);
        // 重复应用幂等
        phone.sync_apply_config(std::slice::from_ref(&tpl), std::slice::from_ref(&fd)).unwrap();
        assert_eq!(phone.list_templates().unwrap().len(), 1);
        // 桌面清空 → 手机清空
        phone.sync_apply_config(&[], &[]).unwrap();
        assert!(phone.list_templates().unwrap().is_empty());
        assert!(phone.list_field_defs(None).unwrap().is_empty());
    }

    /// 索引交换：服务器侧三向判定（没有→want；服务器新→to_client；客户端新→want）
    #[test]
    fn index_exchange_directions() {        let (s, _dir) = store();
        let a = add(&s, "服务器独有", now());
        let b = add(&s, "双方都有_服务器新", now());

        let resp = s
            .sync_handle_index(&SyncIndexReq {
                items: vec![
                    SyncIndexEntry { id: a.id.clone(), updated_at: a.updated_at, deleted: false },
                    SyncIndexEntry {
                        id: b.id.clone(),
                        updated_at: b.updated_at - chrono::Duration::seconds(1),
                        deleted: false,
                    },
                    SyncIndexEntry {
                        id: "log_remote1".into(),
                        updated_at: now(),
                        deleted: false,
                    },
                ],
            })
            .unwrap();
        assert_eq!(resp.want, vec!["log_remote1".to_string()]);
        let to_ids: Vec<&str> = resp.to_client.iter().map(|i| i.id.as_str()).collect();
        assert!(to_ids.contains(&b.id.as_str()), "服务器新 → to_client");
        assert!(!to_ids.contains(&a.id.as_str()), "两边一致 → 不发");
    }
}


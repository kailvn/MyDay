//! MyDay 核心库。
//!
//! GUI（Tauri）与 CLI 共享本 crate，保证数据模型、存储层、提醒逻辑唯一：
//! - [`model`]：三类统一条目（日程 / 待办 / 记录）及附属对象；类型创建即定，不可互转
//! - [`store`]：SQLite 存储（SCHEMA-REDESIGN 定稿 schema、WAL、事务）与全部业务读写
//! - [`view`]：视图模型与过滤器引擎（FILTER-SPEC v1：条件 AST、view_defs、
//!   预筛下推、挂件聚合管线、内置视图种子）
//! - [`overlay`]：今日悬浮窗求值（OVERLAY-SPEC v1：今日未完成的唯一语义实现）
//! - [`attachment`]：附件目录管理（数据库只存相对路径）
//! - [`recurrence`]：重复规则文法与窗口展开（SPRINT-SPEC §2）
//! - [`reminder`]：到期提醒查询与去重（reminder_log 复合键）
//! - [`ics`]：ICS 导出（VEVENT/VTODO + RRULE + VALARM，SPRINT2-SPEC §6）
//! - [`backup`]：一键备份 zip（VACUUM INTO 快照 + attachments，SPRINT2-SPEC §6）
//! - [`ipc`]：本地 IPC 协议（Unix domain socket + JSON Lines）
//! - [`paths`]：数据目录约定（支持 `MYDAY_DATA_DIR` 覆盖，便于测试与备份）

pub mod attachment;
pub mod backup;
pub mod error;
pub mod ics;
pub mod ipc;
pub mod model;
pub mod overlay;
pub mod paths;
pub mod recurrence;
pub mod reminder;
pub mod store;
pub mod tpltime;
pub mod view;

pub use error::{ErrorCode, MyDayError, Result};
pub use model::*;
pub use paths::{attachments_dir, data_dir, db_path, socket_endpoint, socket_path};
pub use store::Store;

//! 数据目录约定（需求 §4、§10、场景 H）。
//!
//! 默认数据目录：`~/.local/share/myday`
//! - `myday.db`：SQLite 数据库
//! - `attachments/<item_id>/<uuid>.png`：图片附件
//! - `backups/`：P1 备份输出
//!
//! 环境变量 `MYDAY_DATA_DIR` 可整体覆盖（测试与便携部署用），
//! socket 路径可用 `MYDAY_SOCKET_PATH` 覆盖，默认在 `$XDG_RUNTIME_DIR`。
//! 整个数据目录可整体复制备份迁移。

use std::path::PathBuf;

/// 数据目录（自动创建）。
pub fn data_dir() -> std::io::Result<PathBuf> {
    let dir = match std::env::var_os("MYDAY_DATA_DIR") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => dirs::data_dir()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "cannot resolve data dir")
            })?
            .join("myday"),
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// SQLite 数据库文件路径。
pub fn db_path() -> std::io::Result<PathBuf> {
    Ok(data_dir()?.join("myday.db"))
}

/// 附件根目录。
pub fn attachments_dir() -> std::io::Result<PathBuf> {
    Ok(data_dir()?.join("attachments"))
}

/// 本地 IPC Unix socket 路径。
#[cfg(windows)]
pub fn socket_path() -> std::io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("MYDAY_SOCKET_PATH") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    // Windows 无 XDG 约定；IPC 实际端点见 [`socket_endpoint`]，此路径仅为兼容保留。
    Ok(std::env::temp_dir().join("myday.sock"))
}

/// 本地 IPC Unix socket 路径。
#[cfg(not(windows))]
pub fn socket_path() -> std::io::Result<PathBuf> {
    if let Some(p) = std::env::var_os("MYDAY_SOCKET_PATH") {
        if !p.is_empty() {
            return Ok(PathBuf::from(p));
        }
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("TMPDIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    Ok(runtime.join("myday.sock"))
}

/// IPC 端点的可读描述（诊断输出用）：Windows 为 `127.0.0.1:port`。
#[cfg(windows)]
pub fn socket_endpoint() -> std::io::Result<String> {
    // std 的 Windows UDS 尚未稳定（rust-lang/rust#150487），IPC 走本地 TCP。
    // `MYDAY_IPC_PORT` 可覆盖；`MYDAY_SOCKET_PATH` 为纯数字时同样按端口处理（测试隔离用）。
    if let Some(p) = std::env::var_os("MYDAY_IPC_PORT") {
        if let Ok(port) = p.to_string_lossy().parse::<u16>() {
            return Ok(format!("127.0.0.1:{port}"));
        }
    }
    if let Some(p) = std::env::var_os("MYDAY_SOCKET_PATH") {
        if let Ok(port) = p.to_string_lossy().parse::<u16>() {
            return Ok(format!("127.0.0.1:{port}"));
        }
    }
    Ok("127.0.0.1:45987".into())
}

/// IPC 端点的可读描述（诊断输出用）：Unix 为 socket 文件路径。
#[cfg(not(windows))]
pub fn socket_endpoint() -> std::io::Result<String> {
    Ok(socket_path()?.to_string_lossy().into_owned())
}

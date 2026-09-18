//! 本地 IPC（需求 §2.4、§9.3）。
//!
//! 传输：Unix domain socket（默认 `$XDG_RUNTIME_DIR/myday.sock`）+ JSON Lines，
//! 每连接一请求一响应（写一行请求 → 读一行响应 → 关闭）。
//! Windows 上 std 的 AF_UNIX 尚未稳定，传输降级为 127.0.0.1 TCP，协议不变。
//!
//! 协同规则：
//! - GUI 启动时绑定 socket 并启动服务线程；
//! - CLI 优先通过 IPC 转发写操作，GUI 执行并刷新界面（单一写入口，视图实时一致）；
//! - GUI 未运行时 CLI 直接写库（[`crate::store::Store`]），
//!   GUI 下次启动自然加载；
//! - `myday quick-add`（无参数）通过 `ShowQuickAdd` 唤起快速窗口。

use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, MyDayError, Result};
use crate::model::{ItemPatch, ItemType, NewItem};

/// 传输层：Unix 为 UDS；Windows 上 std 的 AF_UNIX 尚未稳定（rust-lang/rust#150487），
/// 改用 127.0.0.1 TCP（端点由 [`crate::paths::socket_endpoint`] 决定）。
#[cfg(unix)]
pub type IpcListener = UnixListener;
#[cfg(unix)]
pub type IpcStream = UnixStream;
#[cfg(windows)]
pub type IpcListener = TcpListener;
#[cfg(windows)]
pub type IpcStream = TcpStream;

/// CLI → GUI 请求。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
pub enum IpcRequest {
    /// 探活，返回版本
    Ping,
    /// 唤起快速添加窗口（GNOME 自定义快捷键路径）
    ShowQuickAdd {
        item_type: Option<ItemType>,
        title: Option<String>,
    },
    /// 由 GUI 代为执行创建（GUI 运行时的写路径）
    AddItem { new: NewItem },
    UpdateItem { id: String, patch: ItemPatch },
    DeleteItem { id: String },
    CompleteTask { id: String },
    Snooze { id: String, until: chrono::DateTime<chrono::Utc> },
    /// 类型间转换（SPRINT2-SPEC §7）：to = event / log
    ConvertItem { id: String, to: String },
    /// 通知 GUI 数据已被外部（CLI 直写）修改，刷新视图
    Refresh,
    /// 打开并定位到条目（通知点击）
    Reveal { id: String },
}

/// 统一响应信封（与 CLI JSON 输出结构一致，需求 §2.3）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<IpcError>,
}

impl IpcResponse {
    pub fn ok(data: impl Into<serde_json::Value>) -> Self {
        IpcResponse {
            ok: true,
            data: Some(data.into()),
            error: None,
        }
    }

    pub fn err(code: ErrorCode, message: impl Into<String>) -> Self {
        IpcResponse {
            ok: false,
            data: None,
            error: Some(IpcError {
                code: code.as_str().to_string(),
                message: message.into(),
            }),
        }
    }

    pub fn from_error(e: &MyDayError) -> Self {
        Self::err(e.code(), e.to_string())
    }

    pub fn into_result<T: for<'de> Deserialize<'de>>(self) -> Result<T> {
        if !self.ok {
            let msg = self
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "ipc error".into());
            return Err(MyDayError::Internal(format!("ipc: {msg}")));
        }
        serde_json::from_value(self.data.unwrap_or(serde_json::Value::Null))
            .map_err(|e| MyDayError::Internal(format!("ipc response decode: {e}")))
    }
}

/// GUI 侧请求处理器。
pub trait IpcHandler: Send + Sync + 'static {
    fn handle(&self, req: IpcRequest) -> IpcResponse;
}

/// 绑定 IPC socket。若存在残留 socket 且无法连接则移除后重绑。
#[cfg(unix)]
pub fn bind(path: &Path) -> std::io::Result<UnixListener> {
    if path.exists() {
        let stale = UnixStream::connect(path)
            .map(|mut s| {
                let _ = s.write_all(b"{\"cmd\":\"ping\"}\n");
                false
            })
            .map_err(|_| true)
            .unwrap_or(true);
        if stale {
            let _ = std::fs::remove_file(path);
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another myday GUI already owns the socket",
            ));
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    UnixListener::bind(path)
}

/// Windows：绑定 127.0.0.1 TCP；端口被占用即视为已有 GUI 在运行。
#[cfg(windows)]
pub fn bind(_path: &Path) -> std::io::Result<TcpListener> {
    let addr = crate::paths::socket_endpoint()?;
    TcpListener::bind(&addr).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "another myday GUI already owns the IPC port",
            )
        } else {
            e
        }
    })
}

/// 阻塞服务循环（调用方负责放入独立线程）。连接逐个处理，个人应用流量足够。
pub fn serve(listener: IpcListener, handler: Arc<dyn IpcHandler>) -> ! {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_conn(stream, handler.as_ref());
            }
            Err(e) => {
                eprintln!("myday ipc accept error: {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn handle_conn(stream: IpcStream, handler: &dyn IpcHandler) -> Result<()> {
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let resp = match serde_json::from_str::<IpcRequest>(line.trim()) {
        Ok(req) => handler.handle(req),
        Err(e) => IpcResponse::err(ErrorCode::Invalid, format!("bad ipc request: {e}")),
    };
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    stream.write_all(out.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// 客户端：发送单次请求。GUI 未运行（连接失败）返回 `Err`。
#[cfg(unix)]
pub fn send(req: &IpcRequest) -> Result<IpcResponse> {
    let path = crate::paths::socket_path()?;
    send_to(&path, req)
}

/// 客户端：发送单次请求。GUI 未运行（连接失败）返回 `Err`。
#[cfg(windows)]
pub fn send(req: &IpcRequest) -> Result<IpcResponse> {
    let addr = crate::paths::socket_endpoint()?;
    let stream = TcpStream::connect(&addr)
        .map_err(|e| MyDayError::Internal(format!("GUI not running ({e})")))?;
    stream_call(stream, req)
}

/// 客户端：向指定 socket 发送单次请求（Unix 专用，Windows 走 [`send`]）。
#[cfg(unix)]
pub fn send_to(path: &Path, req: &IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(path)
        .map_err(|e| MyDayError::Internal(format!("GUI not running ({e})")))?;
    stream_call(stream, req)
}

fn stream_call(stream: IpcStream, req: &IpcRequest) -> Result<IpcResponse> {
    let mut stream = stream;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader.read_line(&mut resp_line)?;
    serde_json::from_str(&resp_line).map_err(|e| MyDayError::Internal(format!("ipc decode: {e}")))
}

/// GUI 是否正在运行（socket 可连通）。
#[cfg(unix)]
pub fn is_gui_running() -> bool {
    let Ok(path) = crate::paths::socket_path() else {
        return false;
    };
    UnixStream::connect(path).is_ok()
}

/// GUI 是否正在运行（IPC 端口可连通）。
#[cfg(windows)]
pub fn is_gui_running() -> bool {
    let Ok(addr) = crate::paths::socket_endpoint() else {
        return false;
    };
    TcpStream::connect(addr).is_ok()
}

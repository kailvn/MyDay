//! 桌面同步服务（局域网 HTTP，v1）：axum 双端点 + Bearer token 校验。
//!
//! 托盘常驻时监听 `0.0.0.0:SYNC_PORT`；手机端在前台主动发起两步同步
//! （`myday_core::sync` 协议）。合并规则全部在 core，服务端只做转发与鉴权，
//! 不存在独立逻辑。明文 HTTP + 随机 token，仅限个人局域网场景。

use std::sync::Arc;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use myday_core::sync::{SyncIndexReq, SyncIndexResp, SyncPushReq, SyncPushResp, SYNC_PORT};
use myday_core::Store;
use tauri::Manager;
use tokio::sync::watch;

pub struct SyncServerState {
    pub store: Arc<Store>,
    pub token: String,
    /// push 应用后向 GUI 广播 data-changed（否则桌面界面要重启才看到同步来的变化）
    pub app: tauri::AppHandle,
}

// ----------------------------------------------------------------------
// 端点
// ----------------------------------------------------------------------

fn check_auth(headers: &HeaderMap, token: &str) -> Result<(), (StatusCode, String)> {
    let ok = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {token}"))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "token 不匹配".into()))
    }
}

fn to_err<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn sync_index(
    State(st): State<Arc<SyncServerState>>,
    headers: HeaderMap,
    Json(req): Json<SyncIndexReq>,
) -> Result<Json<SyncIndexResp>, (StatusCode, String)> {
    check_auth(&headers, &st.token)?;
    let store = st.store.clone();
    let resp = tauri::async_runtime::spawn_blocking(move || store.sync_handle_index(&req))
        .await
        .map_err(to_err)?
        .map_err(to_err)?;
    Ok(Json(resp))
}

async fn sync_push(
    State(st): State<Arc<SyncServerState>>,
    headers: HeaderMap,
    Json(req): Json<SyncPushReq>,
) -> Result<Json<SyncPushResp>, (StatusCode, String)> {
    check_auth(&headers, &st.token)?;
    let store = st.store.clone();
    // record_sync = true：服务端会话收尾,更新「上次完成同步」基准
    let resp =
        tauri::async_runtime::spawn_blocking(move || store.sync_apply_incoming(&req.items, true))
            .await
            .map_err(to_err)?
            .map_err(to_err)?;
    if resp.applied > 0 {
        use tauri::Emitter;
        let _ = st.app.emit("data-changed", serde_json::json!({}));
    }
    Ok(Json(resp))
}

// ----------------------------------------------------------------------
// 生命周期
// ----------------------------------------------------------------------

async fn build_router(state: Arc<SyncServerState>) -> Router {
    Router::new()
        .route("/sync/index", post(sync_index))
        .route("/sync/push", post(sync_push))
        .with_state(state)
}

/// 启动监听。返回停机句柄；再次 start 前须先 stop。
pub async fn start(
    app: tauri::AppHandle,
    store: Arc<Store>,
    token: String,
) -> Result<watch::Sender<bool>, String> {
    let (tx, mut rx) = watch::channel(false);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", SYNC_PORT))
        .await
        .map_err(|e| format!("绑定端口 {SYNC_PORT} 失败: {e}"))?;
    let state = Arc::new(SyncServerState { store, token, app });
    let router = build_router(state).await;
    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = rx.changed().await;
            })
            .await;
    });
    Ok(tx)
}

/// 本机主路由的局域网 IP（UDP connect 不发包，仅让内核选路由）。
/// 离线 / 无路由时为 None,由前端提示手填。
pub fn primary_lan_ip() -> Option<String> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip().to_string())
}

pub fn new_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ----------------------------------------------------------------------
// Tauri 侧生命周期管理(Managed State)
// ----------------------------------------------------------------------

/// 停机句柄容器(managed)。None = 未运行。
pub struct SyncRunning(pub std::sync::Mutex<Option<watch::Sender<bool>>>);

/// 启动前先停旧实例(端口占用保护)。
pub(crate) async fn server_start(
    app: &tauri::AppHandle,
    store: Arc<Store>,
    token: String,
) -> Result<(), String> {
    server_stop(app);
    let tx = start(app.clone(), store, token).await?;
    if let Some(st) = app.try_state::<SyncRunning>() {
        if let Ok(mut g) = st.0.lock() {
            *g = Some(tx);
        }
    }
    Ok(())
}

pub(crate) fn server_stop(app: &tauri::AppHandle) {
    if let Some(st) = app.try_state::<SyncRunning>() {
        if let Ok(mut g) = st.0.lock() {
            if let Some(tx) = g.take() {
                let _ = tx.send(true);
            }
        }
    }
}

pub(crate) fn server_running(app: &tauri::AppHandle) -> bool {
    app.try_state::<SyncRunning>()
        .map(|s| s.0.lock().map(|g| g.is_some()).unwrap_or(false))
        .unwrap_or(false)
}

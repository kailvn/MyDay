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

/// 同步可用的候选地址：真实枚举全部网卡（默认路由法只能看到一张），
/// 覆盖 USB 网络共享（usb/rndis/ncm）、蓝牙网络共享（bt-pan）、Tailscale 等
/// 虚拟网卡的地址——二维码与手填不再绑死主路由。
pub fn candidate_lan_ips() -> Vec<String> {
    let raw = if_addrs::get_if_addrs()
        .map(|list| {
            list.into_iter()
                .map(|i| {
                    let ip = i.ip();
                    (i.name, ip)
                })
                .collect()
        })
        .unwrap_or_default();
    select_candidates(raw)
}

/// 从原始 (网卡名, IP) 列表筛出同步候选：剔除回环 / 链路本地（169.254，
/// DHCP 失败的空网卡）与常见虚拟网桥（docker 等，手机永远到不了），
/// 按「家用网段 → 办公网段 → CGNAT/Tailscale → 其他」排序（同类内按地址序）。
fn select_candidates(raw: Vec<(String, std::net::IpAddr)>) -> Vec<String> {
    let mut out: Vec<(u8, String)> = Vec::new();
    for (name, ip) in raw {
        let std::net::IpAddr::V4(v4) = ip else {
            continue;
        };
        if v4.is_loopback() || is_junk_iface(&name) {
            continue;
        }
        let o = v4.octets();
        if o[0] == 169 && o[1] == 254 {
            continue;
        }
        out.push((rank_ip(&o), v4.to_string()));
    }
    out.sort();
    out.dedup();
    out.into_iter().map(|(_, ip)| ip).collect()
}

/// 手机永远连不上的虚拟接口（网桥 / 容器对），按名前缀剔除；
/// tailscale / ZeroTier / usb / bt-pan / wg 等真实可达链路全部保留。
fn is_junk_iface(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    ["docker", "br-", "veth", "virbr", "vmnet", "lo"]
        .iter()
        .any(|p| n.starts_with(p))
}

/// 地址类别（越小越优先）：192.168 家用 > 10. 办公/大型局域网 >
/// 172.16-31 > 100.64-127（CGNAT 段，Tailscale 等虚拟网所在）> 其他。
fn rank_ip(o: &[u8; 4]) -> u8 {
    match (o[0], o[1]) {
        (192, 168) => 0,
        (10, _) => 1,
        (172, 16..=31) => 2,
        (100, 64..=127) => 3,
        _ => 4,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    fn v4(s: &str) -> IpAddr {
        s.parse().expect("valid ipv4")
    }

    #[test]
    fn candidates_filter_and_rank() {
        let out = select_candidates(vec![
            ("lo".into(), v4("127.0.0.1")),
            ("wlp3s0".into(), v4("192.168.1.23")), // 家用 Wi-Fi：最优先
            ("enp5s0".into(), v4("10.0.12.5")),    // 办公网
            ("docker0".into(), v4("172.17.0.1")),  // 容器网桥：按名剔除
            ("br-9a2b".into(), v4("172.18.0.1")),  // 同上
            ("virbr0".into(), v4("192.168.122.1")), // libvirt 网桥：剔除
            ("tailscale0".into(), v4("100.64.12.9")), // 虚拟网：保留，排 CGNAT 段
            ("usb0".into(), v4("192.168.42.87")),  // USB 网络共享：保留
            ("wlp3s0".into(), v4("169.254.3.4")),  // 链路本地：剔除
            ("eth0".into(), v4("10.0.12.5")),      // 同 IP 多网卡：去重
        ]);
        assert_eq!(
            out,
            vec!["192.168.1.23", "192.168.42.87", "10.0.12.5", "100.64.12.9"]
        );
    }

    #[test]
    fn rank_orders_network_classes() {
        let mut ips = [
            "100.64.0.1",
            "172.20.0.1",
            "8.8.8.8",
            "192.168.0.2",
            "10.1.2.3",
        ];
        ips.sort_by_key(|s| rank_ip(&s.parse::<std::net::Ipv4Addr>().expect("ipv4").octets()));
        assert_eq!(
            ips,
            [
                "192.168.0.2",
                "10.1.2.3",
                "172.20.0.1",
                "100.64.0.1",
                "8.8.8.8"
            ]
        );
    }

    #[test]
    fn junk_iface_names() {
        assert!(is_junk_iface("docker0"));
        assert!(is_junk_iface("br-1a2b3c"));
        assert!(is_junk_iface("vethabc123"));
        assert!(is_junk_iface("LO")); // 大小写不敏感
        assert!(!is_junk_iface("wlp3s0"));
        assert!(!is_junk_iface("bt-pan"));
        assert!(!is_junk_iface("tailscale0"));
        assert!(!is_junk_iface("usb0"));
    }
}

//! 手机端同步客户端（移动专属）：reqwest 原生栈直连桌面同步服务。
//!
//! 不走 WebView 的 `fetch`——Android 默认拦明文 HTTP,原生 socket 无此限制。
//! 服务器地址缺省时按 `/proc/net/route` 探测默认网关（USB 网络共享 / 热点
//! 拓扑下,桌面就在网关上）,探测失败由前端提示手填。

use std::sync::Arc;

use myday_core::sync::{SyncIndexReq, SyncIndexResp, SyncPushReq, SyncPushResp, SYNC_PORT};
use myday_core::Store;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub async fn sync_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    server: Option<String>,
    token: String,
) -> Result<serde_json::Value, String> {
    let store = state.store.clone();
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("未填写配对码".into());
    }

    let manual = server
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let candidates: Vec<String> = match manual {
        Some(s) => vec![normalize_base(&s)],
        None => probe_gateways()
            .into_iter()
            .map(|gw| format!("http://{gw}:{SYNC_PORT}"))
            .collect(),
    };
    if candidates.is_empty() {
        return Err("未探测到网关,请手填桌面地址".into());
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let mut last_err = "无可用地址".to_string();
    for base in candidates {
        match run_session(&http, &store, &base, &token).await {
            Ok(report) => {
                let mut v = serde_json::json!({ "base": base });
                if let serde_json::Value::Object(map) = report {
                    for (k, val) in map {
                        v[k] = val;
                    }
                }
                // 拉取应用到了本地库：广播刷新（否则日历要重进应用才更新）
                if v["pulled"].as_u64().unwrap_or(0) > 0 {
                    use tauri::Emitter;
                    let _ = app.emit("data-changed", serde_json::json!({}));
                }
                return Ok(v);
            }
            Err(e) => {
                last_err = format!("{base}: {e}");
                continue;
            }
        }
    }
    Err(last_err)
}

async fn run_session(
    http: &reqwest::Client,
    store: &Arc<Store>,
    base: &str,
    token: &str,
) -> Result<serde_json::Value, String> {
    let s = store.clone();
    let index = tauri::async_runtime::spawn_blocking(move || s.sync_list_index())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let url = format!("{base}/sync/index");
    let resp: SyncIndexResp = http
        .post(&url)
        .bearer_auth(token)
        .json(&SyncIndexReq { items: index })
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("服务端拒绝: {e}"))?
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))?;

    // 配置对齐先行（模板 / 字段定义，桌面权威）：手机端记录要用模板。
    // None = 对端旧版本不下发配置 → 跳过（保留本地种子）；Some = 精确对齐（可清空）
    if resp.templates.is_some() || resp.field_defs.is_some() {
        let s = store.clone();
        let (tpls, fds) = (resp.templates.clone().unwrap_or_default(), resp.field_defs.clone().unwrap_or_default());
        tauri::async_runtime::spawn_blocking(move || s.sync_apply_config(&tpls, &fds))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    }

    let s = store.clone();
    let to_client = resp.to_client.clone();
    let applied =
        tauri::async_runtime::spawn_blocking(move || s.sync_apply_incoming(&to_client, false))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

    let s = store.clone();
    let push_items = tauri::async_runtime::spawn_blocking(move || {
        s.sync_get_items(&resp.want)
            .map(|v| SyncPushReq { items: v })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let url = format!("{base}/sync/push");
    let pushed: SyncPushResp = http
        .post(&url)
        .bearer_auth(token)
        .json(&push_items)
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("服务端拒绝: {e}"))?
        .json()
        .await
        .map_err(|e| format!("响应解析失败: {e}"))?;

    let s = store.clone();
    tauri::async_runtime::spawn_blocking(move || s.sync_mark_done())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let last_sync = {
        let s = store.clone();
        tauri::async_runtime::spawn_blocking(move || s.get_setting("sync.last_sync_at"))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?
    };

    Ok(serde_json::json!({
        "pulled": applied.applied,
        "conflicts_in": applied.conflicts,
        "pushed": pushed.applied,
        "conflicts_server": pushed.conflicts,
        "last_sync": last_sync,
    }))
}

fn normalize_base(s: &str) -> String {
    let s = s.trim().trim_end_matches('/');
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    }
}

/// 从 /proc/net/route 提取默认路由网关（USB 网络共享 / 热点下即桌面所在）。
/// 小端十六进制 → 点分十进制;读不到返回空(前端提示手填)。
fn probe_gateways() -> Vec<String> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string("/proc/net/route") else {
        return out;
    };
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 || cols[1] != "00000000" {
            continue;
        }
        let hex = cols[2];
        if hex.len() != 8 {
            continue;
        }
        let bytes: Option<Vec<u8>> = (0..4)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
            .collect();
        if let Some(b) = bytes {
            // 小端:反转得到点分序
            out.push(format!("{}.{}.{}.{}", b[3], b[2], b[1], b[0]));
        }
    }
    out.sort();
    out.dedup();
    out
}

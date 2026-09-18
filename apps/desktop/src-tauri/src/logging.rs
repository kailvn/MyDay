//! 最小文件日志（1.0）：`<数据目录>/logs/myday.log`，超过 1MB 轮转为 `.old`。
//!
//! 桌面进程从桌面环境启动时 stderr 不可见，出问题无从排查 —— 这里给
//! 提醒环 / IPC / 启动失败 / panic 一个落点；「设置 → 数据与 IPC → 打开日志目录」
//! 可直接跳转。CLI 侧维持 stderr（那是它的接口），不经过这里。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_BYTES: u64 = 1_000_000;

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 在装配最早点调用（Store 打开成功后、任何可能出错的子系统之前）。
pub fn init(data_root: &Path) {
    let _ = LOG_DIR.set(data_root.join("logs"));
    install_panic_hook();
}

pub fn log_path() -> Option<PathBuf> {
    LOG_DIR.get().map(|d| d.join("myday.log"))
}

pub fn log_dir() -> Option<PathBuf> {
    LOG_DIR.get().cloned()
}

/// 追加一行 `时间戳 消息`；失败静默（日志不能反过来干扰主流程）。
pub fn log(msg: &str) {
    let Some(dir) = LOG_DIR.get() else { return };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("myday.log");
    // 轮转：超过上限改名让位（只留一份 .old，够定位「上次崩在哪」）
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let _ = std::fs::rename(&path, dir.join("myday.log.old"));
        }
    }
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let _ = writeln!(f, "{ts} {msg}");
    eprintln!("{msg}");
}

/// panic 同时落到日志与 stderr；只记信息不拦截（unwind 行为不变）。
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log(&format!("myday: PANIC {info}"));
        default(info);
    }));
}

//! `myday` CLI —— 数据通道，不是交互层。
//!
//! 定位：对 core 数据模型的完整、平直的读写入口（人/脚本/agent 通用）。
//! 不做任何"组装"：模板预填、快速创建、类型识别回显等交互逻辑全部在前端。
//!
//! - 人类可读输出为默认；`--json` 输出稳定结构 `{ok, data}` / `{ok, error{code,message}}`
//! - 退出码：0 成功，1 一般错误，2 参数错误（clap 默认），3 未找到，4 冲突
//! - 写操作优先转发给运行中的 GUI（IPC，由 GUI 执行并刷新视图），GUI 未运行时直写
//! - `extra` 的键 = 字段 id（`myday field list` 可查）；文件链接保留键为 `文件`

mod output;
mod util;

use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use myday_core::ipc::{self, IpcRequest, IpcResponse};
use myday_core::model::*;
use myday_core::store::{ListOrder, Store, TaskView};
use myday_core::{MyDayError, Result};

use output::{print_item, print_json_envelope, print_list, JsonMode};
use util::{parse_dt_arg, parse_due_arg, parse_recurrence_arg, parse_reminder_spec};

#[derive(Parser)]
#[command(
    name = "myday",
    version,
    about = "MyDay 数据 CLI（条目/字段完整读写 + 唤起 GUI 快速弹窗；支持 --json）"
)]
struct Cli {
    /// 所有输出改为稳定 JSON（agent / 脚本模式）
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 条目（event / task / log 统一模型）完整读写
    Item {
        #[command(subcommand)]
        cmd: ItemCmd,
    },
    /// 视图管理（FILTER-SPEC §12）：内置视图 seed 进库，可被定制 / 重置
    View {
        #[command(subcommand)]
        cmd: ViewCmd,
    },
    /// 字段定义管理（extra 的键 = 字段 id）
    Field {
        #[command(subcommand)]
        cmd: FieldCmd,
    },
    /// 唤起 GUI 快速添加弹窗（可带预填）；内容组装由前端完成
    QuickAdd {
        /// 预填类型：event / task / log；缺省由前端按内容识别
        #[arg(short, long = "type")]
        r#type: Option<String>,
        /// 预填标题
        #[arg(long)]
        title: Option<String>,
    },
    /// 搜索标题 / 备注 / 标签 / 字段名与字段值
    Search {
        query: String,
        /// 限定类型：event / task / log
        #[arg(long)]
        r#type: Option<String>,
    },
    /// IPC 探活：检查 GUI 是否运行
    Ping,
    /// 前置并聚焦主窗口
    Reveal,
    /// 统计汇总：热力图 / 打卡连续 / 数值趋势（SPRINT-SPEC §6）
    Stats {
        /// 统计窗口天数（7–1095，缺省 365；热力图与趋势随之变化）
        #[arg(long)]
        days: Option<i64>,
    },
    /// 提醒中心：最近已处理提醒（SPRINT2-SPEC §5）
    Reminders {
        /// 取最近 N 条（缺省 20）
        #[arg(long)]
        limit: Option<i64>,
    },
    /// 导出（SPRINT2-SPEC §6）
    Export {
        #[command(subcommand)]
        cmd: ExportCmd,
    },
    /// 一键备份 zip（db + attachments，保留最近 7 份；SPRINT2-SPEC §6）
    Backup,
}

#[derive(Subcommand)]
enum ExportCmd {
    /// 导出 ICS 日历文件（日程 VEVENT + 带截止待办 VTODO；重复规则与提醒一并映射）
    Ics {
        /// 输出路径；缺省写到数据目录 exports/
        #[arg(long)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum ItemCmd {
    /// 新建条目。类型缺省时按内容自动识别（截止→task、开始→event、记录字段→log、裸文本→task）
    Add {
        /// event / task / log
        #[arg(long = "type")]
        r#type: Option<String>,
        #[arg(short, long)]
        title: Option<String>,
        #[arg(long)]
        note: Option<String>,
        /// 开始时间（event 必填）：2026-09-16T14:30 / 2026-09-16 / RFC3339
        #[arg(long)]
        start: Option<String>,
        /// 结束时间，缺省为开始 + 1 小时（仅 event / task）
        #[arg(long)]
        end: Option<String>,
        /// 截止时间或日期（仅 task）；仅日期表示全天截止
        #[arg(long)]
        due: Option<String>,
        /// 发生时间（仅 log，缺省现在，不允许未来）
        #[arg(long)]
        at: Option<String>,
        /// 全天日程（仅 event）
        #[arg(long)]
        all_day: bool,
        /// 初始状态（仅 task）：todo / done，缺省 todo
        #[arg(long)]
        status: Option<String>,
        /// 重复规则（仅 event / task）：@daily / @weekly:1-7（1=周一）/ @monthly:1-31
        #[arg(long)]
        recurse: Option<String>,
        /// 按模板创建（defaults 双命名空间预填；模板管理在前端）
        #[arg(long)]
        template: Option<String>,
        /// 提醒：@token（@start-1h / @due-1d / @dailyT09:00）或时间；缺省按设置提前 N 分钟
        #[arg(long)]
        remind: Option<String>,
        /// 禁用提醒
        #[arg(long, conflicts_with = "remind")]
        no_remind: bool,
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// 字段值，可重复：--field <字段id>=<值>
        #[arg(long = "field")]
        fields: Vec<String>,
        /// 幂等键：重复创建返回已有条目
        #[arg(long)]
        idempotency_key: Option<String>,
        /// 预览将写入的数据，不落库
        #[arg(long)]
        dry_run: bool,
        /// 从 stdin 读取 JSON 作为基础输入（命令行参数优先）
        #[arg(long)]
        stdin: bool,
    },
    /// 列表：--view 走待办标准视图，否则按过滤条件平铺
    List {
        /// today / upcoming / all / done（待办四视图）
        #[arg(long)]
        view: Option<String>,
        /// event / task / log
        #[arg(long = "type")]
        r#type: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        /// todo / done
        #[arg(long)]
        status: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// 单日，如 2026-09-16
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        asc: bool,
        #[arg(long)]
        limit: Option<i64>,
    },
    /// 查看详情
    Get { id: String },
    /// 部分更新（未指定的字段不动；类型不可变）
    Update {
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        note: Option<String>,
        #[arg(long)]
        start: Option<String>,
        /// 清空开始时间（event 不允许）
        #[arg(long)]
        clear_start: bool,
        #[arg(long)]
        end: Option<String>,
        #[arg(long)]
        clear_end: bool,
        #[arg(long)]
        due: Option<String>,
        #[arg(long)]
        clear_due: bool,
        /// 发生时间（仅 log，不允许未来）
        #[arg(long)]
        at: Option<String>,
        /// todo / done（仅 task）
        #[arg(long)]
        status: Option<String>,
        /// 重复规则（仅 event / task）：@daily / @weekly:1-7 / @monthly:1-31；`none` 清除
        #[arg(long)]
        recurse: Option<String>,
        #[arg(long = "tag")]
        tags: Option<Vec<String>>,
        /// 字段值，可重复：--field <字段id>=<值>（与条目现有 extra 合并后整体替换）
        #[arg(long = "field")]
        fields: Vec<String>,
        /// 整体替换 extra（优先级低于 --field 合并结果）
        #[arg(long)]
        extra_json: Option<String>,
        #[arg(long)]
        remind: Option<String>,
        /// 删除全部提醒
        #[arg(long, conflicts_with = "remind")]
        no_remind: bool,
    },
    /// 删除（级联清理提醒 / 标签 / 附件）
    Delete { id: String },
    /// 待办标记完成（自动写一条完成记录，可在设置关闭）
    Complete { id: String },
    /// 待办取消完成（回到 todo，completed_at 置空）
    Uncomplete { id: String },
    /// 稍后提醒：该条目全部提醒改为 until；旧触发日志保留
    Snooze {
        id: String,
        #[arg(long)]
        until: String,
    },
    /// 类型转换（SPRINT2-SPEC §7）：待办转日程（替换）/ 日程生成记录（保留原日程）
    Convert {
        id: String,
        /// 转换目标：event / log
        #[arg(long = "to")]
        to: String,
    },
    /// 按视图查询（FILTER-SPEC §12）：与 GUI 同 id 同结果（同一时刻 + 同一本地时区）
    Query {
        /// 视图 id（myday view list 可查；内置 view_builtin_*）
        #[arg(long)]
        view: String,
        /// 运行时关键词：编译为 OR 子树并入视图过滤（标题 / 备注 / 标签 / 字段值）
        #[arg(long)]
        keyword: Option<String>,
        /// 覆盖视图数据集的 limit
        #[arg(long)]
        limit: Option<i64>,
    },
}

#[derive(Subcommand)]
enum ViewCmd {
    /// 列出全部视图（内置标「默认」，带 customized 标志——仅对内置视图有意义）
    List {
        /// 只看某面板：logs / tasks / search / stats
        #[arg(long)]
        panel: Option<String>,
    },
    /// 查看视图配置：缺省输出生效配置（config_user ?? config，标注 customized）；
    /// --seed 看 seed 原文
    Show {
        id: String,
        #[arg(long)]
        seed: bool,
    },
}

#[derive(Subcommand)]
enum FieldCmd {
    /// 列出活跃字段定义（软删的不显示）
    List {
        /// 只看某类型的字段：event / task / log；缺省全部
        #[arg(long)]
        scope: Option<String>,
    },
    /// 新增字段（返回分配的字段 id）
    Add {
        #[arg(short, long)]
        name: String,
        /// text / number / select / multiselect / bool / date / url
        #[arg(long, default_value = "text")]
        kind: String,
        /// number 字段的单位
        #[arg(long)]
        unit: Option<String>,
        /// select / multiselect 的选项，可重复
        #[arg(long = "choice")]
        choices: Vec<String>,
        /// 限定类型：event / task / log；缺省全局
        #[arg(long)]
        scope: Option<String>,
    },
    /// 修改字段（改名零成本：值按 id 存，无需迁移）
    Update {
        id: String,
        #[arg(long)]
        name: Option<String>,
        /// number 字段的单位（传空串清除）
        #[arg(long)]
        unit: Option<String>,
        /// select / multiselect 的选项，可重复（覆盖原有）
        #[arg(long = "choice")]
        choices: Vec<String>,
    },
    /// 删除字段（软删：同名可重建，条目上的历史值保留但不再展示）
    Delete { id: String },
}

fn main() {
    let cli = Cli::parse();
    let mode = JsonMode(cli.json);
    let result = dispatch(&cli.command, mode);
    match result {
        Ok(()) => {}
        Err(e) => {
            let code = e.code();
            if mode.0 {
                let env = IpcResponse::err(code, e.to_string());
                let _ = serde_json::to_writer(std::io::stdout(), &env);
                println!();
            } else {
                eprintln!("myday: {e}");
            }
            std::process::exit(code.exit_code());
        }
    }
}

fn dispatch(cmd: &Commands, mode: JsonMode) -> Result<()> {
    match cmd {
        Commands::Item { cmd } => item_cmd(cmd, mode),
        Commands::View { cmd } => view_cmd(cmd, mode),
        Commands::Field { cmd } => field_cmd(cmd, mode),
        Commands::QuickAdd { r#type, title } => quick_add(r#type.as_deref(), title.as_deref(), mode),
        Commands::Search { query, r#type } => search(query, r#type.as_deref(), mode),
        Commands::Ping => ping(mode),
        Commands::Reveal => {
            let resp = ipc::send(&IpcRequest::Reveal { id: String::new() })?;
            if mode.0 {
                println!("{}", serde_json::to_string(&resp).unwrap_or_default());
            } else if resp.ok {
                println!("已前置主窗口");
            } else {
                println!("GUI 未响应 reveal");
            }
            Ok(())
        }
        Commands::Stats { days } => stats_cmd(*days, mode),
        Commands::Reminders { limit } => reminders_cmd(*limit, mode),
        Commands::Export { cmd } => export_cmd(cmd, mode),
        Commands::Backup => backup_cmd(mode),
    }
}

/// 统计汇总（SPRINT-SPEC §6 / SPRINT2-SPEC §8）：--json 输出完整结构；人类可读输出摘要。
fn stats_cmd(days: Option<i64>, mode: JsonMode) -> Result<()> {
    let days = days.unwrap_or(365);
    let s = open_store()?.stats_summary(days)?;
    if mode.0 {
        println!("{}", serde_json::to_string(&s)?);
        return Ok(());
    }
    let last7: i64 = s.heatmap.iter().rev().take(7).map(|d| d.count).sum();
    println!(
        "近 7 天记录 {} 条 · 近 {} 天 {} 条",
        last7,
        s.heatmap.len(),
        s.heatmap.iter().map(|d| d.count).sum::<i64>()
    );
    if s.streaks.is_empty() {
        println!("打卡：暂无 pinned 记录模板");
    } else {
        for t in &s.streaks {
            println!(
                "打卡 {}{}：连续 {} 天 · 最长 {} 天 · 近 {} 天 {} 次",
                t.name,
                t.icon.as_deref().map(|i| format!(" {i}")).unwrap_or_default(),
                t.current,
                t.longest,
                s.heatmap.len(),
                t.recent
            );
        }
    }
    if s.series.is_empty() {
        println!("趋势：暂无含 ≥2 个数值的 number 字段");
    } else {
        for x in &s.series {
            let last = x.points.last().map(|(_, v)| v.to_string()).unwrap_or_default();
            let min = x.points.iter().map(|(_, v)| v).fold(f64::INFINITY, |a, &b| a.min(b));
            let max = x.points.iter().map(|(_, v)| v).fold(f64::NEG_INFINITY, |a, &b| a.max(b));
            println!(
                "趋势 {}{}：{} 点 · 最新 {}（min {min} / max {max}）",
                x.name,
                x.unit.as_deref().map(|u| format!("({u})")).unwrap_or_default(),
                x.points.len(),
                last
            );
        }
    }
    Ok(())
}

// ----------------------------------------------------------------------
// 写操作：GUI 运行时走 IPC（由 GUI 执行并刷新视图），否则直写
// ----------------------------------------------------------------------

fn run_mutation(
    mode: JsonMode,
    req: IpcRequest,
    direct: impl FnOnce(&Store) -> Result<serde_json::Value>,
) -> Result<()> {
    let value = if ipc::is_gui_running() {
        let resp = ipc::send(&req)?;
        if !resp.ok {
            let (code, message) = resp
                .error
                .as_ref()
                .map(|e| (e.code.clone(), e.message.clone()))
                .unwrap_or(("INTERNAL".into(), "ipc error".into()));
            return Err(map_ipc_error(code, message));
        }
        resp.data.unwrap_or(serde_json::Value::Null)
    } else {
        let store = open_store()?;
        direct(&store)?
    };
    let item: Item = serde_json::from_value(value)
        .map_err(|e| MyDayError::Internal(format!("内部响应解码失败: {e}")))?;
    print_item(mode, &item)
}

fn map_ipc_error(code: String, message: String) -> MyDayError {
    match code.as_str() {
        "INVALID" => MyDayError::Invalid(message),
        "NOT_FOUND" => MyDayError::NotFound(message),
        "CONFLICT" => MyDayError::Conflict(message),
        "UNSUPPORTED" => MyDayError::Unsupported(message),
        _ => MyDayError::Internal(message),
    }
}

fn open_store() -> Result<Store> {
    Store::open_default()
}

// ----------------------------------------------------------------------
// Item 命令实现
// ----------------------------------------------------------------------

fn item_cmd(cmd: &ItemCmd, mode: JsonMode) -> Result<()> {
    match cmd {
        ItemCmd::Add {
            r#type,
            title,
            note,
            start,
            end,
            due,
            at,
            all_day,
            status,
            recurse,
            template,
            remind,
            no_remind,
            tags,
            fields,
            idempotency_key,
            dry_run,
            stdin,
        } => {
            let mut new = base_new_item(*stdin)?;
            new.item_type = match r#type.as_deref() {
                None => None,
                Some(t) => Some(ItemType::parse(t).ok_or_else(|| {
                    MyDayError::Invalid(format!("未知类型: {t}（可用 event / task / log）"))
                })?),
            };
            // --at 是明确的记录语义：未显式指定类型时视为 log
            if new.item_type.is_none() && at.is_some() {
                new.item_type = Some(ItemType::Log);
            }
            apply_common(&mut new, title, note, tags, idempotency_key.clone());
            new.template_id = template.clone().or(new.template_id);
            new.start_at = opt_time(start.as_deref(), new.start_at)?;
            new.end_at = opt_time(end.as_deref(), new.end_at)?;
            new.occurred_at = opt_time(at.as_deref(), new.occurred_at)?;
            if let Some(d) = due {
                let (t, all) = parse_due_arg(d)?;
                new.due_at = Some(t);
                new.due_all_day = all;
            }
            new.all_day = *all_day || new.all_day;
            if let Some(s) = recurse.as_deref() {
                new.recurrence = Some(parse_recurrence_arg(s)?);
            }
            if let Some(s) = status.as_deref() {
                new.status = Some(ItemStatus::parse(s).ok_or_else(|| {
                    MyDayError::Invalid(format!(
                        "未知状态: {s}（可用 todo / done）"
                    ))
                })?);
            }
            new.reminders = match remind.as_deref() {
                Some(r) => vec![NewReminder {
                    spec: parse_reminder_spec(r)?,
                    channel: "notify".into(),
                }],
                None if *no_remind => Vec::new(),
                None => Vec::new(), // 未表态 → core 按设置补默认提醒
            };
            merge_fields(&mut new.extra, fields)?;
            validate_new(&new)?;
            if *dry_run {
                return print_json_envelope(mode, &serde_json::to_value(&new)?, |v| {
                    println!("[dry-run] 将创建: {}", v["title"].as_str().unwrap_or(""));
                });
            }
            run_mutation(mode, IpcRequest::AddItem { new: new.clone() }, |store| {
                let item = store.add_item(new.clone())?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::List {
            view,
            r#type,
            from,
            to,
            status,
            tags,
            date,
            asc,
            limit,
        } => {
            let store = open_store()?;
            if let Some(v) = view.as_deref() {
                let tv = parse_view(v)?;
                let items = store.tasks_view(tv, None)?;
                return print_list(mode, items);
            }
            let (f, t) = util::resolve_range(date.as_deref(), from.as_deref(), to.as_deref())?;
            let parsed_status = match status.as_deref() {
                None => None,
                Some(s) => Some(ItemStatus::parse(s).ok_or_else(|| {
                    MyDayError::Invalid(format!(
                        "未知状态: {s}（可用 todo / done）"
                    ))
                })?),
            };
            let items = store.list_items(&ListFilter {
                item_type: parse_type(r#type.as_deref())?,
                from: f,
                to: t,
                status: parsed_status,
                tag: tags.first().cloned(),
                limit: *limit,
                order: if *asc { ListOrder::Asc } else { ListOrder::Desc },
                ..Default::default()
            })?;
            print_list(mode, items)
        }
        ItemCmd::Get { id } => print_item(mode, &open_store()?.get_item(id)?),
        ItemCmd::Update {
            id,
            title,
            note,
            start,
            clear_start,
            end,
            clear_end,
            due,
            clear_due,
            at,
            status,
            recurse,
            tags,
            fields,
            extra_json,
            remind,
            no_remind,
        } => {
            let store = open_store()?;
            // --field 与现有 extra 合并后整体替换（ItemPatch.extra 是整对象语义）
            let mut extra = match extra_json {
                Some(s) => serde_json::from_str(s)
                    .map_err(|e| MyDayError::Invalid(format!("--extra-json 无效: {e}")))?,
                None => store.get_item(id)?.extra,
            };
            merge_fields(&mut extra, fields)?;
            let (due_at, due_all_day) = match due.as_deref() {
                Some(d) => {
                    let (t, all) = parse_due_arg(d)?;
                    (Some(t), Some(all))
                }
                None => (None, None),
            };
            let patch = ItemPatch {
                title: title.clone(),
                note: note.clone(),
                start_at: start.as_deref().map(parse_dt_arg).transpose()?,
                end_at: end.as_deref().map(parse_dt_arg).transpose()?,
                clear_start_at: *clear_start,
                clear_end_at: *clear_end,
                due_at,
                due_all_day,
                clear_due_at: *clear_due,
                occurred_at: at.as_deref().map(parse_dt_arg).transpose()?,
                status: match status.as_deref() {
                    None => None,
                    Some(s) => Some(ItemStatus::parse(s).ok_or_else(|| {
                        MyDayError::Invalid(format!(
                            "未知状态: {s}（可用 todo / done）"
                        ))
                    })?),
                },
                recurrence: recurse
                    .as_deref()
                    .filter(|s| !s.eq_ignore_ascii_case("none"))
                    .map(parse_recurrence_arg)
                    .transpose()?,
                clear_recurrence: recurse.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("none")),
                tags: tags.clone(),
                extra: if fields.is_empty() && extra_json.is_none() {
                    None
                } else {
                    Some(extra)
                },
                reminders: remind
                    .as_deref()
                    .map(parse_reminder_spec)
                    .transpose()?
                    .map(|spec| {
                        vec![NewReminder { spec, channel: "notify".into() }]
                    }),
                clear_reminders: *no_remind,
                ..Default::default()
            };
            run_mutation(mode, IpcRequest::UpdateItem { id: id.clone(), patch: patch.clone() }, |store| {
                let item = store.update_item(id, patch)?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Delete { id } => {
            run_mutation(mode, IpcRequest::DeleteItem { id: id.clone() }, |store| {
                let item = store.delete_item(id)?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Complete { id } => {
            run_mutation(mode, IpcRequest::CompleteTask { id: id.clone() }, |store| {
                let item = store.complete_task(id)?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Uncomplete { id } => {
            run_mutation(mode, IpcRequest::UpdateItem {
                id: id.clone(),
                patch: ItemPatch { status: Some(ItemStatus::Todo), ..Default::default() },
            }, |store| {
                let item = store.uncomplete_task(id)?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Snooze { id, until } => {
            let until: DateTime<Utc> = parse_dt_arg(until)?;
            run_mutation(mode, IpcRequest::Snooze { id: id.clone(), until }, |store| {
                let item = store.snooze(id, until)?;
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Convert { id, to } => {
            let to = to.to_ascii_lowercase();
            if !matches!(to.as_str(), "event" | "log") {
                return Err(MyDayError::Invalid(
                    "--to 只支持 event / log（待办转日程 / 日程生成记录）".into(),
                ));
            }
            run_mutation(mode, IpcRequest::ConvertItem { id: id.clone(), to: to.clone() }, |store| {
                let item = if to == "event" {
                    store.convert_task_to_event(id)?
                } else {
                    store.event_to_log(id)?
                };
                Ok(serde_json::to_value(&item)?)
            })
        }
        ItemCmd::Query { view, keyword, limit } => {
            let store = open_store()?;
            let result = store.query_view(view, keyword.as_deref(), *limit)?;
            if mode.0 {
                println!(
                    "{}",
                    serde_json::json!({ "ok": true, "data": result })
                );
                return Ok(());
            }
            println!(
                "视图 {}（{} · {}）· 命中 {} 条 · 求值 {} ({})",
                result.name,
                result.view_id,
                result.panel.as_str(),
                result.total,
                result.evaluated_at,
                result.tz
            );
            if result.customized {
                println!("（已定制；myday view show {} --seed 可看 seed 原文）", result.view_id);
            }
            match (result.widgets, result.groups, result.items) {
                (Some(widgets), _, _) => {
                    for w in widgets {
                        println!(
                            "· {} [{:?}] {}",
                            w.title,
                            w.render,
                            w.error.as_deref().unwrap_or("")
                        );
                        if let Some(s) = &w.derived.streak {
                            println!(
                                "  连续 {} · 最长 {} · 近期 {}",
                                s.current, s.longest, s.recent
                            );
                        }
                    }
                }
                (_, Some(groups), _) => {
                    for g in groups {
                        println!("— {}", g.label);
                        for item in &g.items {
                            println!("  {}", output::human_line(item));
                        }
                    }
                }
                (_, _, Some(items)) => print_list(mode, items)?,
                _ => {}
            }
            Ok(())
        }
    }
}

// ----------------------------------------------------------------------
// View 命令实现（FILTER-SPEC §12）
// ----------------------------------------------------------------------

fn view_cmd(cmd: &ViewCmd, mode: JsonMode) -> Result<()> {
    match cmd {
        ViewCmd::List { panel } => {
            let panel = match panel.as_deref() {
                None => None,
                Some(p) => Some(myday_core::view::Panel::parse(p).ok_or_else(|| {
                    MyDayError::Invalid(format!("未知面板: {p}（可用 logs / tasks / search / stats）"))
                })?),
            };
            let views = open_store()?.list_views(panel)?;
            if mode.0 {
                let data: Vec<serde_json::Value> = views
                    .iter()
                    .map(|v| {
                        let mut row = serde_json::json!({
                            "id": v.id,
                            "name": v.name,
                            "panel": v.panel.as_str(),
                            "builtin": v.builtin,
                        });
                        // customized 仅对内置视图有意义（§12）
                        if v.builtin {
                            row["customized"] = serde_json::json!(v.customized());
                        }
                        row
                    })
                    .collect();
                println!("{}", serde_json::json!({ "ok": true, "data": data }));
                return Ok(());
            }
            for v in views {
                println!(
                    "{:<32} {:<14} {:<7} {}{}",
                    v.id,
                    v.name,
                    v.panel.as_str(),
                    if v.builtin { "内置" } else { "自定义" },
                    if v.customized() { " · 已定制" } else { "" },
                );
            }
            Ok(())
        }
        ViewCmd::Show { id, seed } => {
            let v = open_store()?.get_view(id)?;
            let config = if *seed { &v.config } else { v.effective_config() };
            if mode.0 {
                let data = serde_json::json!({
                    "id": v.id,
                    "name": v.name,
                    "panel": v.panel.as_str(),
                    "builtin": v.builtin,
                    "customized": v.customized(),
                    "showing_seed": *seed,
                    "config": config,
                });
                println!("{}", serde_json::json!({ "ok": true, "data": data }));
                return Ok(());
            }
            println!(
                "{} · {} [{}]{}{}",
                v.id,
                v.name,
                v.panel.as_str(),
                if v.builtin { " · 内置" } else { " · 自定义" },
                if v.customized() { " · 已定制" } else { "" },
            );
            println!(
                "{}",
                serde_json::to_string_pretty(config).unwrap_or_else(|_| "配置解析失败".into())
            );
            Ok(())
        }
    }
}

// ----------------------------------------------------------------------
// Field 命令实现
// ----------------------------------------------------------------------

fn field_cmd(cmd: &FieldCmd, mode: JsonMode) -> Result<()> {
    match cmd {
        FieldCmd::List { scope } => {
            let scope = parse_scope(scope.as_deref())?;
            let defs = open_store()?.list_field_defs(scope)?;
            if mode.0 {
                println!("{}", serde_json::json!({ "ok": true, "data": defs }));
            } else {
                for f in defs {
                    println!(
                        "{:<14} {:<10} {:<12} {} {}{}",
                        f.id,
                        f.name,
                        f.kind.as_str(),
                        f.options,
                        f.scope.map(|s| format!("[{}]", s.as_str())).unwrap_or_else(|| "[全局]".into()),
                        if f.builtin { " 内置" } else { "" }
                    );
                }
            }
            Ok(())
        }
        FieldCmd::Add { name, kind, unit, choices, scope } => {
            let kind = FieldKind::parse(kind)
                .ok_or_else(|| MyDayError::Invalid(format!(
                    "未知字段类型: {kind}（可用 text/number/select/multiselect/bool/date/url）"
                )))?;
            let mut options = serde_json::Map::new();
            if let Some(u) = unit.as_deref() {
                options.insert("unit".into(), serde_json::json!(u));
            }
            if !choices.is_empty() {
                options.insert("choices".into(), serde_json::json!(choices));
            }
            let def = open_store()?.add_field_def(
                name,
                kind,
                &serde_json::Value::Object(options),
                parse_scope(scope.as_deref())?,
            )?;
            print_field_def(mode, &def)
        }
        FieldCmd::Update { id, name, unit, choices } => {
            let mut options: Option<serde_json::Value> = None;
            if unit.is_some() || !choices.is_empty() {
                let existing = open_store()?.get_field_def(id)?;
                let mut opts = existing.options.as_object().cloned().unwrap_or_default();
                if let Some(u) = unit.as_deref() {
                    if u.is_empty() {
                        opts.remove("unit");
                    } else {
                        opts.insert("unit".into(), serde_json::json!(u));
                    }
                }
                if !choices.is_empty() {
                    opts.insert("choices".into(), serde_json::json!(choices));
                }
                options = Some(serde_json::Value::Object(opts));
            }
            let def = open_store()?.update_field_def(id, name.as_deref(), options.as_ref(), None)?;
            print_field_def(mode, &def)
        }
        FieldCmd::Delete { id } => {
            let def = open_store()?.delete_field_def(id)?;
            print_field_def(mode, &def)
        }
    }
}

fn print_field_def(mode: JsonMode, def: &FieldDef) -> Result<()> {
    if mode.0 {
        println!("{}", serde_json::json!({ "ok": true, "data": def }));
    } else {
        println!("{} {} ({}) {}", def.id, def.name, def.kind.as_str(), def.options);
    }
    Ok(())
}

// ----------------------------------------------------------------------
// quick-add / search / ping / reminders / export / backup
// ----------------------------------------------------------------------

/// 仅唤起 GUI 弹窗并预填；内容组装、类型识别回显与改口全部在前端。
fn quick_add(ty: Option<&str>, title: Option<&str>, mode: JsonMode) -> Result<()> {
    let item_type = parse_type(ty)?;
    let resp = ipc::send(&IpcRequest::ShowQuickAdd {
        item_type,
        title: title.map(str::to_string),
    })
    .map_err(|_| {
        MyDayError::Internal(
            "GUI 未运行，无法弹出快速窗口；请先启动 MyDay，或用 myday item add 直接创建".into(),
        )
    })?;
    if !resp.ok {
        let (code, message) = resp
            .error
            .map(|e| (e.code, e.message))
            .unwrap_or(("INTERNAL".into(), "ipc error".into()));
        return Err(map_ipc_error(code, message));
    }
    print_json_envelope(mode, &serde_json::json!({ "shown": true }), |_| {
        println!("已唤起快速添加窗口");
    })
}

fn search(query: &str, type_filter: Option<&str>, mode: JsonMode) -> Result<()> {
    let ty = parse_type(type_filter)?;
    let hits = open_store()?.search(query, ty)?;
    print_list(mode, hits.into_iter().map(|h| h.item).collect())
}

fn ping(mode: JsonMode) -> Result<()> {
    match ipc::send(&IpcRequest::Ping) {
        Ok(resp) if resp.ok => {
            print_json_envelope(mode, &resp.data.unwrap_or(serde_json::json!("pong")), |v| {
                println!("GUI 运行中 {}", v);
            })?;
            Ok(())
        }
        Ok(resp) => {
            let msg = resp.error.map(|e| e.message).unwrap_or_default();
            Err(MyDayError::Internal(format!("GUI 响应异常: {msg}")))
        }
        Err(_) => {
            print_json_envelope(mode, &serde_json::json!({"running": false}), |_| {
                println!("GUI 未运行");
            })?;
            Ok(())
        }
    }
}

/// 提醒中心历史（SPRINT2-SPEC §5）。
fn reminders_cmd(limit: Option<i64>, mode: JsonMode) -> Result<()> {
    let hist = open_store()?.reminder_history(limit.unwrap_or(20).clamp(1, 200))?;
    if mode.0 {
        println!("{}", serde_json::json!({ "ok": true, "data": hist }));
        return Ok(());
    }
    if hist.is_empty() {
        println!("暂无提醒历史");
        return Ok(());
    }
    for h in &hist {
        let local = h.remind_at.with_timezone(&chrono::Local);
        println!(
            "{}  [{}] {}{}",
            local.format("%m-%d %H:%M"),
            h.item.item_type,
            display_title(&h.item),
            h.item
                .status
                .map(|s| if s == ItemStatus::Done { " · 已完成" } else { "" })
                .unwrap_or(""),
        );
    }
    Ok(())
}

/// ICS 导出（SPRINT2-SPEC §6）。
fn export_cmd(cmd: &ExportCmd, mode: JsonMode) -> Result<()> {
    match cmd {
        ExportCmd::Ics { output } => {
            let store = open_store()?;
            let ics = myday_core::ics::export_ics(&store)?;
            let path = match output.as_deref() {
                Some(p) => std::path::PathBuf::from(p),
                None => {
                    let dir = store.data_root().join("exports");
                    std::fs::create_dir_all(&dir)
                        .map_err(|e| MyDayError::Internal(format!("创建导出目录失败: {e}")))?;
                    dir.join(format!(
                        "myday-{}.ics",
                        chrono::Local::now().format("%Y%m%d-%H%M%S")
                    ))
                }
            };
            std::fs::write(&path, &ics)
                .map_err(|e| MyDayError::Internal(format!("写入 ICS 失败: {e}")))?;
            print_json_envelope(
                mode,
                &serde_json::json!({ "path": path.to_string_lossy(), "bytes": ics.len() }),
                |_| println!("已导出 {}", path.display()),
            )
        }
    }
}

/// 一键备份 zip（SPRINT2-SPEC §6）。
fn backup_cmd(mode: JsonMode) -> Result<()> {
    let store = open_store()?;
    let path = myday_core::backup::backup_zip(&store)?;
    print_json_envelope(
        mode,
        &serde_json::json!({ "path": path.to_string_lossy() }),
        |_| println!("已备份 {}", path.display()),
    )
}

// ----------------------------------------------------------------------
// 共享工具
// ----------------------------------------------------------------------

fn parse_type(s: Option<&str>) -> Result<Option<ItemType>> {
    match s {
        None => Ok(None),
        Some(t) => ItemType::parse(t)
            .map(Some)
            .ok_or_else(|| MyDayError::Invalid(format!("未知类型: {t}（可用 event/task/log）"))),
    }
}

fn parse_scope(s: Option<&str>) -> Result<Option<ItemType>> {
    parse_type(s)
}

fn opt_time(arg: Option<&str>, current: Option<DateTime<Utc>>) -> Result<Option<DateTime<Utc>>> {
    match arg {
        Some(s) => Ok(Some(parse_dt_arg(s)?)),
        None => Ok(current),
    }
}

fn base_new_item(use_stdin: bool) -> Result<NewItem> {
    if use_stdin {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        let new: NewItem = serde_json::from_str(buf.trim())
            .map_err(|e| MyDayError::Invalid(format!("stdin JSON 无效: {e}")))?;
        Ok(new)
    } else {
        Ok(NewItem::default())
    }
}

fn apply_common(
    new: &mut NewItem,
    title: &Option<String>,
    note: &Option<String>,
    tags: &[String],
    idempotency_key: Option<String>,
) {
    if let Some(t) = title {
        new.title = Some(t.clone());
    }
    if note.is_some() {
        new.note = note.clone();
    }
    if !tags.is_empty() {
        new.tags = tags.to_vec();
    }
    if idempotency_key.is_some() {
        new.idempotency_key = idempotency_key;
    }
}

/// 解析 `--field 字段id=值`：extra 的键就是字段 id，不做名字解析。
/// 值按 bool → number → string 推断类型。
fn merge_fields(extra: &mut serde_json::Value, fields: &[String]) -> Result<()> {
    if fields.is_empty() {
        return Ok(());
    }
    if extra.is_null() {
        *extra = serde_json::json!({});
    }
    let map = extra
        .as_object_mut()
        .ok_or_else(|| MyDayError::Invalid("extra 必须是 JSON 对象".into()))?;
    for f in fields {
        let Some((k, v)) = f.split_once('=') else {
            return Err(MyDayError::Invalid(format!(
                "字段格式应为 字段id=值（收到 \"{f}\"；字段 id 用 myday field list 查询）"
            )));
        };
        let k = k.trim();
        if k.is_empty() {
            return Err(MyDayError::Invalid("字段 id 不能为空".into()));
        }
        let v = v.trim();
        let parsed = if v == "true" || v == "false" {
            serde_json::json!(v == "true")
        } else if let Ok(n) = v.parse::<f64>() {
            serde_json::json!(n)
        } else {
            serde_json::json!(v)
        };
        map.insert(k.to_string(), parsed);
    }
    Ok(())
}

fn validate_new(new: &NewItem) -> Result<()> {
    // 带模板时跳过预校验：core 会在合并 defaults（列 + 字段 id）后统一校验，
    // CLI 在合并前看到的 title/start 可能还缺着
    if new.template_id.is_some() {
        return Ok(());
    }
    let title = new.title.as_deref().map(str::trim).unwrap_or("");
    match new.item_type {
        Some(ItemType::Log) => {
            if title.is_empty()
                && new.note.as_deref().map(str::trim).filter(|n| !n.is_empty()).is_none()
            {
                return Err(MyDayError::Invalid("记录需要 --title 或 --note 至少一项".into()));
            }
        }
        _ => {
            if title.is_empty() {
                return Err(MyDayError::Invalid("标题不能为空（--title 必填）".into()));
            }
            if new.item_type == Some(ItemType::Event) && new.start_at.is_none() {
                return Err(MyDayError::Invalid("日程必须带开始时间（--start）".into()));
            }
        }
    }
    Ok(())
}

fn parse_view(v: &str) -> Result<TaskView> {
    match v {
        "today" => Ok(TaskView::Today),
        "upcoming" => Ok(TaskView::Upcoming),
        "all" => Ok(TaskView::All),
        "done" => Ok(TaskView::Done),
        other => Err(MyDayError::Invalid(format!(
            "未知视图: {other}（可用 today / upcoming / all / done）"
        ))),
    }
}

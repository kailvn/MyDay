# 更新日志

格式参照 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)；
版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。
功能详情见 [docs/FEATURE-INVENTORY.md](docs/FEATURE-INVENTORY.md)。

## [1.0.0] - 2026-09-18

首个正式版本。个人日程 / 待办 / 记录三合一，离线优先（Linux deb + Windows NSIS）。

### 新增
- 三类统一对象（日程 / 待办 / 记录）+ Notion 式字段系统 + 模板（时间占位 token）
- 日历月 / 周 / 日三视图：拖拽改期 / 调时长 / 拖选创建、重复规则（@daily / @weekly / @monthly「修改全部」）、法定节假日休 / 班角标（JSON 可导入）、冲突检测
- 待办四视图 + 批量操作 + 重复推进 / 回拨；记录时间线 + 统计挂件页（热力 / 连续 / 趋势，容器可编排）
- 提醒存意图（改期自动跟随）、去重、补发窗口 + 错过聚合、通知按钮直达（完成 / 稍后 / 打开）、应用内提醒中心
- 今日悬浮窗：置顶 / 锁定穿透 / 透明度 / 位置与尺寸持久化
- 视图模型（FILTER-SPEC）：过滤器 / 排序 / 视图复制重置，GUI 与 CLI 同引擎
- 今日悬浮窗之外的全套录入加速：全局快捷键快速弹窗、类型自动识别、五行时间选择器、受控 NL 时间 chip、Ctrl+K 命令面板、粘贴截图
- 完整 CLI + 本地 IPC：`--json` 稳定信封、幂等键、`--dry-run`、`--stdin`，agent 可编程
- ICS 导出（RRULE + VALARM）+ 一键备份 zip（保留 7 份）

### 1.0 发布专项
- **数据承诺**：v4 起库结构只走附加式迁移链（`MIGRATIONS`），缺迁移步骤 = 拒绝打开，
  不再以任何形式重建丢数据；更旧的遗留库仍备份后重建（仅 1.0 前）
- **界面双语**（中 / 英）：设置页可切换，托盘与系统通知同步跟随；`?lang=` 可强制
- **帮助页**：重要行为说明集中一处，界面内提示全面瘦身
- 开机自启（Linux XDG autostart / Windows Run 键，设置页开关）
- 文件日志 + panic 落盘（`<数据目录>/logs/`，设置页可打开日志目录）
- CI（GitHub Actions：cargo 测试 + 前端 golden + Playwright e2e + i18n key 覆盖检查）
- AppStream metainfo、MIT LICENSE

### 数据格式
- Schema v5（items 统一三类 + view_defs）。v4 库打开时自动补迁移；
  v4 以前的旧结构库先备份到 `backups/` 再重建（历史设计，1.0 前遗留）

### 已知限制
- 重复规则暂不支持单次例外 / until（「修改全部」语义）
- 内置种子数据（内置模板 / 优先级字段的选项值）暂为中文
- 未排期待办池、ICS 订阅、云同步、macOS：见 README 路线图

[1.0.0]: https://github.com/kailvn/MyDay/releases/tag/v1.0.0

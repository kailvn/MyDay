# MyDay 过滤器与视图模型（FILTER-SPEC v1）

> 状态：已实施（P1–P4，2026-09-17；统计页容器模型 v2 同日落地，v2.2 兼容清场）。
> P5 评估项单独立项。
> 本文自包含；对《SPRINT-SPEC》§6 / 《SPRINT2-SPEC》§8（统计）与需求 §7.4 / §11
> 做模型化重构：面板展示什么，由「视图 = 数据集 + 布局 / 挂件 = 数据集 + 聚合 + 渲染器」
> 声明，不再散落硬编码启发式。
> 参照 Anytype 视图模型（视图工具条 / 过滤 / 排序 / 布局）与本项目已有的
> Notion 式字段系统（《SCHEMA-REDESIGN》）。

## 0. 设计原则

1. **底层一条管线**：所有面板的渲染 = 视图求值结果。内置视图是 seed 进表的
   真实行，不是代码里的特判分支——用户在默认视图上直接改，改完立即生效。
2. **默认 = 功能等价**：零自定义时，各面板起到同样的功能——信息量、能力、
   路径长度不缩水，不新增必经操作步骤、不引入需要解释的新概念。观感允许为
   承载过滤器工具条 / 挂件编辑做合理调整；有意的行为差异在实施时逐条列明
   （已知项：趋势图 x 轴改真实时间刻度，见 §8）。捕获路径（快速添加、打字
   回车）仍要求零变化。「需要解释 / 多做事」的判定包含**跨面板学习成本**：
   同一操作（筛选、排序、换视图、编辑挂件）在所有面板是相同或同构的交互
   ——学一次即全站通用，不算新增学习成本；面板间交互不一致 = 需要重新
   学习 = 违反本原则。
3. **快（沿承 INTERACTION §0）**：捕获路径（快速添加窗、打字回车、Ctrl+N）零新增
   成本、零新增界面。过滤编辑是浏览路径上的纯可选交互。
4. **图表 = 数据集 × 聚合（+ 派生指标）× 渲染器**：三段解耦，渲染器只吃统一
   结果表——同数据即时换图型（折线↔柱状↔热力），打卡卡与打卡热力图同源
   （§8）。新增图型不加查询语义。
5. **确定性**：过滤语义全部可解释（空值、软删、重复展开、本地日），宁可查不到
   不静默猜测。

## 1. 借鉴 Anytype：采纳与裁剪

| Anytype 概念 | 决策 | 说明 |
|---|---|---|
| 视图工具条：视图切换 + Filter / Sort 入口 + 激活条件 chips | **采纳** | §9 |
| 运算符随属性类型走（文本含/数字比较/选项属于…） | **采纳** | §4 矩阵 |
| 动态相对日期（Today / Tomorrow / N days ago，查询时求值） | **采纳** | §4.2，视图因此「保鲜」 |
| 多级排序 | **采纳** | §5 |
| 视图复制 / 重置 | **采纳** | §7 |
| 条件 OR 分组 | **采纳，两层封顶** | §3（个人查询 95% 不需要更深） |
| 布局系统（Grid / Board / Gallery / Calendar） | **暂缓** | v1 布局随面板固定，config 留枚举位 |
| 看板（按 select 分组） | **暂缓** | 依赖字段分组，列 P5 评估 |
| 公式 / 关联属性 / 深嵌套过滤 | **不做** | 个人工具无此需求 |

Anytype 内容多会卡的教训不在「有过滤器模型」，在其渲染与对象图架构；对应的三条
实现约束见 §10，本项目按约束走不构成性能风险。

## 2. 概念模型（四层）

```
字段引用 ──► 条件（字段 + 运算符 + 值）──► 条件 AST（AND/OR 组）
                                              │
                       数据集 = 类型 + AST + 排序 + 分组 + limit
                                              │
              ┌───────────────────────────────┴────────────┐
        视图（面板主体）                              挂件（统计页卡片）
        = 数据集 + 布局                       = 数据集 + 聚合 + 派生指标 + 渲染器
```

挂件的 dataset 是**简化数据集**（item_type + filter——无 sort / group / limit：
范围用 window、形态用 agg 表达，§8）。

## 3. 条件 AST

JSON 形态（存于 `view_defs.config`）：

```json
{ "op": "and", "children": [
    { "field": "col:status",  "cmp": "eq", "value": "todo" },
    { "op": "or", "children": [
        { "field": "col:due_at", "cmp": "before", "value": { "rel": "tomorrow" } },
        { "field": "col:anchor", "cmp": "empty" }
    ] }
] }
```

- 节点二态：**组**（`op` = `and` | `or`，`children` 非空）或**条件**（`field`/`cmp`/`value`）。
- 根必须是组；嵌套深度 ≤ 2（组内可含子组，子组内只能含条件）。空 `children` = 恒真。
- **重复条目，两种求值模式**；母条目模式下时间列条件一律作用**存储值**，
  正确性靠两条 store 层不变量：
  - **母条目模式**（缺省）：不展开，列表 / 搜索对母条目求值。
    - **task + recurrence**：行内 `due_at` 恒为当前 / 下一期——完成即推进
      （`complete_task` 同步推进 due 与 start）、取消完成即回拨。对存储 due
      过滤 = 对当前期过滤，**待办四 tab 用母条目模式，不走实例模式**。
      这是**行为不变量，不是 DB 约束**，保证机制 = 唯一写路径 + 入口校验：
      推进 / 回拨逻辑只存在于 `complete_task` / 取消完成；P1 在 store 入口
      （add / update）补统一校验——task + recurrence ⇒ due_at 必填（对齐
      `complete_task` 既有报错）。用户手动或批量改重复待办的
      due = 编辑**当前期**（系列锚点随之平移，「修改全部」语义；README P2
      已知单次例外未做）——不变量不破。三条推论 / 语义钉死：
      - **回拨 = 恢复最近一次推进前的存储值**（记账键 `extra.recurred_done_at`，
        语义由 `uncomplete_task` 唯一定义）：只回退一期；连续多期完成后取消
        也只回一期，更早历史不留存、不补；记账键消费即清。现状存在经
        `update_item` 直改 status 的取消旁路（不触发回拨），P1 统一写路径时
        对齐到 `uncomplete_task`。
      - **重复任务不进收件箱**（有意）：due 必填 ⇒ anchor 恒非空，「收件箱」
        分支（anchor empty）仅覆盖非重复任务。
      - 直写 SQLite 绕过 store 属契约外场景（README 的可编程契约 = CLI / IPC，
        均过 store）。
    - **event + recurrence**：存储 start / end = 规则锚点期，无推进机制。列表 /
      搜索对其过滤作用锚点期（与现状一致）：**列表管「这条规则」，日历管
      「这次发生」**——后者必须走实例模式。
  - **实例模式**：调用方显式传窗口 `window: {from, to}`，event 按 recurrence 在
    窗口内展开为虚拟实例；时间列条件作用于实例时刻，`col:recurrence` 条件作用于
    母条目。**实例模式窗口必填，缺失 = 参数错误**（确定性：宁可报错不猜测，
    不允许无界展开）。今天页 / 日历属实例模式（v1 不接视图模型，P5 评估）。
    虚拟实例的身份（实例 id、完成状态、编辑回写 = 母条目还是单次例外）
    **留 P5 今天页接入时定义**，实施不得自行发明。
- **本地日**：一切「某天」语义用本地时区（对齐 `tasks_view` 的既有做法），
  相对值在**查询时**求值——视图因此不需要每天重写。

## 4. 字段引用与运算符矩阵

### 4.1 字段引用

`field` 取两类值：

| 前缀 | 含义 | 成员 |
|---|---|---|
| `col:` | 内置列 | `type` `status` `title` `note` `tags` `template_id` `recurrence` `start_at` `end_at` `due_at` `occurred_at` `created_at` `updated_at` `completed_at` `anchor` |
| 无前缀 | field_defs.id | 如 `fd_weight_kg`；软删字段不出现 |

- `col:anchor` = 锚点时间（合成列）：task = `COALESCE(due_at, start_at)`
  （**截止优先，开始只是缺省回退**），event = `start_at`，log = `occurred_at`。
  anchor 是**普通字段引用：可作条件、可排序、可分组**；引擎对它没有任何隐藏
  行为——「归属今天」等语义全部来自显式条件（§9 种子即如此）。
- **为什么不用 `created_at` / `updated_at` 这类不为空的键当锚点**：业务时间
  （due / start / occurred）回答「这件事何时到期 / 发生」，系统时间回答「这条数据
  何时被写入」。anchor 的用途——有效时间比较、排序、按天分组——全是业务
  语义：用 `updated_at` 排「即将到期」会退化成「最近编辑」（改个备注就跳到
  最前）。系统时间各司其职：两列都在本清单可排序可过滤，搜索种子默认序即
  `updated_at desc`。anchor 可空正是信息：NULL = 无任何时间承诺（收件箱，见
  下条）；永不为空的键会抹掉这个状态。
- 注：`ListFilter.from/to` 的窗口锚点对 task 用 `start_at`（《model.rs》注释原文），
  本 anchor 对 task 取「due 优先」是**有意差异**——日历类窗口关心开始，到期
  语义关心截止；实现时不要互相对齐。
- **无时间待办 = 收件箱语义**：due 与 start 均空的待办，现状即出现在「今天」、
  排序最后（`tasks_view` Today 的 SQL 行为）。视图模型把它从隐藏行为变成种子
  「今天」视图里显式的 `anchor empty` 条件分支——可见、可编辑、可删除（删除该
  分支 = 未安排的待办不再进今天）；anchor 为 NULL 恒排该级末尾（§5）。
- 保留键（`文件`）、`all_day`、`due_all_day`、附件不暴露为条件（列入未来：
  「有/无附件」）。
- **软删字段**：新建条件选不到（选择器不出现）；**存量条件照常按 extra 历史值
  求值**，编辑器中该行标黄「字段已删除」，去留由用户决定——引擎不改写配置
  （完整失效规则见 §7）。
- v1 不暴露：`col:id`、`idempotency_key`。

### 4.2 日期值（点值 / 范围值两型，与运算符的配对封闭）

**点值**（某一天）：

```json
{ "day": "2026-09-17" }
{ "rel": "today" | "tomorrow" | "yesterday" }
```

**范围值**（一段区间）：

```json
{ "rel": "this_week" | "this_month" | "last_days:N" }
```

- 展开规则（本地时区、查询时求值）：`this_week` = 周一 00:00 ~ 周日 24:00；
  `this_month` = 月初 ~ 月末；`last_days:N` = [今天−N+1 日始, 今天末]（含今天，
  滚动窗口）；**N ≥ 1，否则参数错误**。
- **配对封闭**：`on` / `before` / `after` 只接点值（on = 全天；before = < 日始；
  after = > 日末；「今天或之前」写 `before tomorrow`）；`between` 只接两个点值
  （from 日始 ~ to 日末）；**`within` 只接范围值**（命中展开区间即真）。点值配
  `within`、范围值配其余日期运算符 = 参数错误——UI 控件随运算符切换值控件，
  构造不出非法组合，求值器兜底报错。范围值即 UI 的「本周 / 本月 / 近 N 天」
  预设 chips。
- 「9/1 到本周末」这类混合端点**不放宽 `between`**：用条件组合表达——
  `after 9/1` 且 `within this_week`。保持值型配对封闭，复杂区间一律组合。
- 日期条件（`empty` / `not_empty` 除外）对 NULL 恒假，`within` 亦然。

### 4.3 运算符矩阵

| 字段类型 | `cmp` |
|---|---|
| text / url | `eq` `neq` `contains` `not_contains` `empty` `not_empty` |
| number | `eq` `neq` `gt` `gte` `lt` `lte` `between` `empty` `not_empty` |
| select | `eq` `neq` `empty` `not_empty` |
| multiselect / `col:tags` | `any`（含任一）`all`（含全部）`none`（不含）`empty` `not_empty` |
| bool | `is_true` `is_false` `empty` |
| 日期列（含 `col:anchor`） | `on` `before` `after`（点值）`between`（双点值）`within`（范围值，§4.2）`empty` `not_empty` |
| `col:type` | `eq` |
| `col:status` | `eq` |
| `col:template_id` | `eq` `neq` `empty` `not_empty` |
| `col:recurrence` | `empty`（= 不重复）`not_empty`（= 重复） |

- **空值语义**：extra 缺键 / `null` / 空串 / 空数组 一律算 `empty`；bool 未填
  （null）≠ false。
- `col:status` 值域**封闭于 todo / done**（DB CHECK 兜底，无第三态）：
  「未完成」= `eq todo` 即可，无需 `neq` / `in`；未来引入第三态再扩运算符。
- 文本比较大小写不敏感（对齐现有 search）；number 值可比较要求真的可转数字。

## 5. 排序 / 分组 / 布局

- **排序**：`sort: [{ "field", "dir": "asc"|"desc" }]`，按数组序多级；NULL / 缺值
  **恒排该级末尾**（与方向无关，可预期）；文本按字典序（大小写不敏感）。
- **分组**：`group`：`null` 或 `{ "bucket": "day", "field": "col:occurred_at" }`。
  v1 仅支持时间桶分组（记录时间线用）；组序 = 组键倒序，组内按 `sort`。
  按字段分组（看板）列 P5。
  分组遇 NULL（该列无值）单独成桶：键 null、展示「未安排 / 空」、恒排组序
  末尾（log 的 occurred_at 有 DB CHECK 非空兜底，规则仍防御性适用）。
- **布局**：`layout: "timeline" | "list"`（枚举预留）；v1 随面板固定：记录页 =
  `timeline`，待办 / 搜索 = `list`，统计页 = 挂件流。
- **可见字段**：`visible: [field…]` 预留（替换现在硬编码的 `fd_priority` 徽标
  判断，`TasksView.svelte:209`）；v1 全部视图 = auto（渲染现状徽标），编辑器 P5。

## 6. 数据集

```json
{ "item_type": "log" | "task" | "event" | "all",
  "filter": <AST>,
  "sort": [ … ],
  "group": …,
  "limit": 200 }
```

与 `ListFilter` 的关系：`ListFilter` 保留为**预筛层**（类型 + 索引时间窗 + 状态，
走现有组合索引），AST 在预筛结果上求值。两者不合并、不互斥。

## 7. 存储：`view_defs` 表

```sql
CREATE TABLE view_defs (
  id          TEXT PRIMARY KEY,   -- view_<8hex>；内置 view_builtin_*
  name        TEXT NOT NULL,
  panel       TEXT NOT NULL,      -- logs / tasks / search / stats
  config      TEXT NOT NULL,      -- 用户视图 = 生效配置；内置视图 = seed
  config_user TEXT,               -- 内置视图的用户定制（NULL = 未定制）
  builtin     INTEGER NOT NULL DEFAULT 0,
  sort        INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL, updated_at TEXT NOT NULL
);
```

- **生效配置 = `config_user ?? config`**。内置视图的编辑写 `config_user`；
  「重置」= 清空 `config_user`。
- **升级 seed 只覆盖 `config` 列，`config_user` 永不触碰**——用户定制不因
  升级丢失（修正 v1 的「seed 覆盖会丢定制」）。
- 动态内置挂件的隐藏状态（如 `hidden_templates`）**写 `config_user`**，不写
  seed 列（否则升级即丢）。恢复不必整行重置：「＋ 打卡卡」选择器中已隐藏的
  卡显示为「已隐藏」，点击即恢复（从 `config_user` 移除该项）。把某张卡
  「定制 / 转显式」时同步从 hidden_templates 移除对应项——**显式卡与隐藏项
  不共存**（防双重状态）。
- 实现从简：无历史迁移负担（首建即种），seed 于建库 / 升级时 upsert 覆盖
  `config` 列即可；**不为 `config_user` 写合并 / 迁移防御逻辑**（完整副本
  语义，§11 提示长期定制另存为用户视图）。
- **用户视图**：物理删除（无历史价值）；「另存为」从任意视图复制出用户视图。
- **内置视图**：不可删。
- CRUD 命令广播 `data-changed`，前端各面板按 panel 订阅刷新。

**被引用对象失效的统一规则**（求值永远按存储值，配置永不被引擎静默改写，
失效只做可见提示）：

| 被引对象 | 求值 | 编辑器提示 |
|---|---|---|
| 字段软删 | 条件照常按 extra 历史值匹配 | 标黄「字段已删除」 |
| 字段彻底清理（extra 值被 purge） | 恒不匹配 | 标红「字段已清除」 |
| 模板删除 | `col:template_id` 按存储 id 匹配，历史条目照常命中 | 标黄「模板已删除」 |
| 标签已不存在 | 按名字匹配，自然不命中 | 标黄「标签不存在」 |
| 动态内置挂件的来源模板 | pinned 取消后该卡自然消失（动态规则） | 无需提示 |

## 8. 挂件：聚合 → 派生指标 → 渲染器

挂件是三段**纯函数管线**，层层解耦——「同数据换渲染器」（折线↔柱状↔热力）
是一等能力，不改动数据配置：

```json
{ "kind": "widget",
  "dataset": { "item_type": "log", "filter": … },     // 挂件无 limit：范围用 window 表达
  "window": { "days": 365 } | { "all": true },         // 缺省 365；滚动窗口（以今天为终点的近 N 天，同 last_days:N），在挂件编辑里设置（页面无全局范围档）
  "agg": {
    "group":  { "by": "time",  "bucket": "day" | "week" | "month",
                "time_field": "col:occurred_at" }
            | { "by": "field", "field": "fd_mood" | "col:template_id" | "col:tags" }
            | null,
    "metric": { "fn": "count" | "sum" | "avg" | "min" | "max" | "values",
                "field": "fd_weight_kg" }               // fn ≠ count 时必填
  },
  "derived": { "kind": "streak" | null, "goal": { "daily": 1 } | { "weekly": 3 } },
  "render": "bar" | "line" | "pie" | "heatmap" | "card",
  "options": { "presence": false, "unit": null, "top_n": 8 } }   // unit 缺省取 metric.field 字段定义的 options.unit，此处仅覆盖
```

**聚合槽位规则**（group 与 metric 分槽，字段角色不再一词多义）：

- `group.by = "time"`：时间桶；`time_field` **显式指定**（记录 = `col:occurred_at`）。
- `group.by = "field"`：字段分组；允许 select / multiselect / `col:template_id` /
  `col:tags`；multiselect / tags = **每个命中值各计一次**（一条可进多个桶）。
- `metric.fn = "values"`（点列）：仅数据源 log；数值 = `metric.field`，时刻 =
  `occurred_at`，且 `group` 必须为 null——**有意限制**：点列是单数据集的原始
  点；「晨重 / 晚重分两条线」用**多线 series 组**表达（每线自带
  dataset.filter，P3 交付）；`group × values`（单数据集内分组出多列点列）会
  引入新的结果表形状，留 P5 评估。
- **挂件 dataset 禁用 limit**：行截断会静默掏空桶；范围只由 `window` 表达。
- **streak 特例**：current / longest 在**全历史**上判定（对齐现状
  `stats_summary`），`window` 只作用于 recent 计数；单习惯全历史为千行级，可接受。

**第一段：聚合 → 统一结果表**（渲染器的唯一输入，形状只有两种）：

- 分桶表 `buckets: [{ key, value }]`——key = 本地日 / ISO 周 / 月，或字段值；
  时间桶升序（line / heatmap 用），字段分组按 value 降序（bar / pie 默认）。
- 点列 `points: [{ t, v }]`——仅 `metric.fn = "values"` 时，(发生时刻, 值) 原始点。

**第二段：派生指标**（可选，纯函数 over 结果表，只产标量/标注、不改表）：
`streak`（`daily:N` = 连续达标日；`weekly:N` = 连续达标周，按本地周一起；今天
已达标含今天，否则存活到昨天 / 上周；weekly 显示「本周还差 X 次」）、
`total` `min` `max` `last`。

**第三段：渲染器**（结果表 + options → 图）。新增图型 = 新增渲染器，查询语义
零新增；渲染器声明兼容约束，不兼容时 UI 置灰并给一键兼容建议：

| 渲染器 | 输入 | 约束 |
|---|---|---|
| `bar` | buckets | 任意时间桶 / 字段分组 |
| `line` | points 或有序时间桶 | values 点列直接画；点列切 bar 时建议改 avg/sum + 月桶 |
| `pie` | 字段分组 buckets | top N + 「其他」；时间桶不适用 |
| `heatmap` | day 桶 buckets | `options.presence` = 只记有无（打卡热力）；缺省密度模式（现状） |
| `card` | buckets + 派生 | 指标卡：streak / total / min / max / last |

**同一数据的形态互换**（抽象是否成立的试金石）：

- 打卡：dataset(跑步) + day×count + streak + `card` ⇄ `heatmap`(presence) = 只记
  有无 ⇄ `bar` = 每日次数。
- 体重：values(fd_weight_kg) + `line` ⇄ 月桶 avg + `bar`。
- 频率：字段×count + `bar` ⇄ `pie`（需求 §7.4）。

**统计页内置挂件（动态，行为 = 现状 `stats_summary`）**：

- `view_builtin_stats_heatmap`：动态 heatmap（全部 log，密度模式）。
- `view_builtin_stats_streaks`：动态 streak 组——对每个 pinned log 模板出一张
  card（filter = `col:template_id eq <tpl>`，day×count + streak derived）；卡上
  × 移除 = 写入该行 `config_user.hidden_templates`（恢复：「＋ 打卡卡」选择器
  「已隐藏」点击恢复，整行重置亦可，§7）。**此后新 pinned 模板不再自动出现
  打卡卡**——要打卡，显式添加或把某张卡「定制」成任意条件来源；也可就地把
  某张卡切换成 heatmap(presence) / bar 形态。
- `view_builtin_stats_series`：动态 series 组——对每个活跃 number 字段出一条
  可选线（现状 chips 行为）；编辑可定稿为显式线列表（可绑定来源条件、可多线
  叠加）。

动态内置挂件编辑保存时转为显式配置（`dynamic` 标记去除）。`stats_summary`
在 P3 完成迁移后退役，`myday stats --json` 改由挂件求值实现（输出信封不变）。

## 9. 内置视图种子（零自定义 = 现状）

| id | panel | 配置要点 | 对齐现状 |
|---|---|---|---|
| `view_builtin_logs_timeline` | logs | source log；无过滤；sort occurred desc；group day:occurred_at；limit 200 | `LogsView` 时间线 |
| `view_builtin_tasks_today` | tasks | status is todo **且**（**anchor before 明天** 或 anchor 空）；sort anchor asc（空最后）；limit 500 | `tasks_view Today` 逐分支等价：有截止只看截止；无截止看开始；全无 = 收件箱 |
| `view_builtin_tasks_upcoming` | tasks | status is todo **且** anchor after 今天；sort anchor asc；limit 100 | `Upcoming` |
| `view_builtin_tasks_all` | tasks | status is todo；sort anchor asc（空最后）；limit 500 | `All` |
| `view_builtin_tasks_done` | tasks | status is done；sort completed_at desc；limit 200 | `Done` |
| `view_builtin_search_all` | search | source all；无过滤；sort updated desc；limit 50 | 搜索页 |

- 搜索页：关键词是**运行时输入**，编译为 OR 子树（标题 / 备注 / 标签 / 字段值
  contains）并入 AST 合取（§10）——全引擎只有一条求值路径；v1 不保存关键词。
- 待办页四 tab 即上表四行的展示；用户新建待办视图（如「高优先级未完成」）并列
  出现在 tab 行。
- 「今天」种子条件 = `anchor before 明天 或 anchor 空`，与 `tasks_view` Today 的
  SQL 逐分支等价：有截止只看截止（截止未来不进今天，哪怕开始在过去——截止
  优先）；无截止看开始；全无 = 收件箱（§4.1）。删除 `anchor empty` 分支即可
  排除未安排待办。Upcoming 的 `anchor after 今天` 同理逐分支等价。
  待办 tab 走母条目模式（§3 不变量：重复待办行内 due 恒为当前期）；「一致」
  限定为**母条目集合一致**——重复待办在待办 tab 与今天页都是单行当前期
  （今天页不展开重复待办）；今天页的重复日程实例展开不涉及待办页。实例级
  身份语义见 §3（P5 定义）。

## 10. 求值引擎与性能策略

**实现**：myday-core（GUI / CLI 唯一语义源）。个人数据量（重度用户年增千余条）
下毫秒级。

**求值顺序（定死，实现不得重排出别的语义）**：

1. **编译**：keyword 编译为 OR 子树（标题 / 备注 / 标签 / 字段值 contains）并入
   视图 filter AST；相对日期与 window 按查询时锚点（本地日）展开。
2. **预筛下推**（规则定死，防 OR 漏数据）：
   - 单条件可下推 ⇔ 其从根到叶的祖先组**全为 and**（此时单独构成超集）。
   - **OR 组整组下推** ⇔ 组内全部子条件可下推：预筛取各子条件超集的**并集**
     （SQL 层 OR 连接）；and 链上多个可下推条件自然合取（交集）。
     `(due before 明天) OR (status eq done)` 若按交集下推会漏掉 done 分支——
     并集是硬规则。
   - 其他情况（OR 含不可下推子条件、**跨列 OR**——ListFilter 各列窗之间是
     AND，表达不了跨列 OR）→ **放弃下推该子树**：少下推 = 候选更多，仍安全。
   - 多时间列条件分别填 `ListFilter` 对应列窗（start / end / due / occurred
     各自独立），不跨列合并、不只用第一列。
   - 下推值一律**松化为超集**（窗口放宽、枚举放宽），宁可多取——预筛只产
     候选，AST 终审裁决。**合成列不下推**（anchor 是 COALESCE，无索引可走）；
     增长路径可选项：按 COALESCE 分支拆解为 OR 下推。
3. **hydrate 一次**；**预筛层绝不施加视图的 limit**（limit 属排序后语义）。
4. **AST 内存求值 = 最终真相**（预筛是超集，不损正确性）。
5. **排序 → limit / offset 永远最后**。挂件另走：**dataset.filter（AST）求值 →
   window 过滤 → 聚合 → 结果表 → 派生指标 → 渲染**（§8）。filter 与 window 为
   AND，先后不影响结果；预筛下推取两者超集——window 直接构成时间窗下推，
   filter 中的时间条件可进一步收窄。
6. **结果信封带求值上下文**：`evaluated_at`（RFC3339）+ `tz`（本地时区）。
   前端三处触发检查：窗口聚焦 / `visibilitychange` / 60 秒定时器——信封的
   本地日或时区与当前不符（跨午夜、系统时区变更）即整体重查；CLI 与 GUI 的
   同结果契约以「同一时刻 + 同一本地时区」为前提（§12）。

**共享扫描是优化，不是语义**：面板一次加载，取**全部可见视图 / 挂件预筛条件的
并集**（对每个 AST 仍是超集）做一次查询 + hydrate，各视图在同一份内存数据上
分别求值（`stats_summary` 已是此模式）。构造不出公共超集时（类型 / 窗口差异
过大）退化为逐视图预筛，每视图一条 SQL 仍是常数级——**正确性优先于共享**。
统计页的预筛窗口取**各挂件窗口的并集（最大窗口）**：存在 streak 卡（全历史）
时整页取 all，window 过滤在内存按挂件进行——N 张 streak 卡在同一份全量内存
集上按各自 filter 求值，**同一数据源不重复扫描**（现状 `stats_summary` 每模板
一次全表扫的低效随之自然消除）。承认耦合：一张 streak 卡决定整页查询量是
**有意的折中**（个人量级无碍）；增长路径：若单页 all 成为瓶颈，streak 卡改为
独立全历史查询（脱离页级共享扫描），页级预筛回窗口——「同源不重复扫」约束
的是数据源去重，不约束窗口分层。

**两条实现约定**（零成本，评审时检查）：

1. **builder 预览防抖 + limit**：编辑器内预览查询带 limit、输入防抖，不逐键全量。
2. **求值不进前端响应式链**：core 一次算完返回结果数组，Svelte 只渲染结果；
   行渲染不得反向触发查询。

**性能策略：先不建基准测试**。遵循以上约定即可；将来实测出现卡顿，再按增长
路径逐级优化——模型不动，AST 放 core 的意义正是编译器可逐节点选择下推：

热字段条件下推 `json_each EXISTS`（search 已有此模式）→ 热字段 SQLite
generated column + 索引 → 关键词 FTS5 → 列表虚拟滚动。

## 11. UI 规范（参照 Anytype 工具条）

- **一套交互，处处相同**：工具条布局、筛选编辑器、排序编辑器、条件 chips、
  视图切换、挂件编辑的规则行，在记录 / 待办 / 搜索 / 统计是**同一共享组件的
  不同实例**（各面板只传参：视图列表、数据类型），不是四套相似实现——
  一致性靠组件复用保证，不靠实现约定。
- **工具条**（记录 / 待办 / 搜索页顶部一行）：左 = 视图切换（下拉，内置标
  「默认」）+ 激活条件 chips（点 chip 本体 = 打开筛选编辑器就地编辑；逐个 ×）；
  右 = 「筛选」「排序」按钮 + 「另存为」。
- **筛选编辑器**（Anytype 式轻量面板：锚定工具条右下的 popover，列表保持
  可见；**改动即时生效**——防抖落库并刷新列表，头部实时显示「匹配 N 条」，
  无「应用」步骤；Esc / 点外部 / 「完成」= 保存后关闭）：「＋ 添加条件」
  **先选字段**（分组 + 搜索的 FieldMenu，选字段即建行）→ 行 = 字段下拉 →
  运算符下拉（按 §4.3 矩阵出集）→ 值控件（文本框 / 数字 / 选项多选 /
  标签 chip 输入 + 已有标签建议 / 日期含相对 token 菜单，「具体日期」默认
  今天）。顶部「满足全部 / 任一」分段切换；「添加条件组」= 子组行（两层
  封顶，组内独立的添加入口）。底部：清空 / 重置（内置视图）/ 完成。
- **排序编辑器**：多级行列表（字段 + 方向 + 增删），与筛选编辑器同款
  popover + 即时生效；「＋ 添加排序级」同样先选字段。
- **挂件卡片**：⋯ 菜单 = 编辑来源 / 指标 / 图型 / 目标（streak 的 daily/weekly）、
  移除（动态内置 = 记 hidden）、重置；图型切换 = 换渲染器，数据集不动。
- 编辑器底部**常驻一行小字**提示「长期定制请另存为用户视图」（非每次弹窗；
  `config_user` 是完整副本，升级 seed 新增默认字段不合并，§7）。
- **不出现过滤 UI 的地方**：快速添加窗、ItemPanel、今天页顶部、日历。今天页 /
  日历 v1 不接视图模型（时间窗语义已自洽，P5 再评估）。
- 键盘：编辑器 Esc 关闭；chips 聚焦 Delete 移除。

## 12. CLI / IPC

- Tauri 命令：`view_list` `view_save` `view_delete` `view_reset` `query_view`
  （统一走本地 IPC，GUI 未运行直写库——对齐现有 CLI 写路径）。
- CLI：
  ```bash
  myday view list --json                   # 每行带 customized 标志（仅对内置视图有意义：config_user 非空；用户视图不输出该字段）
  myday view show <id> [--seed] --json   # 缺省输出生效配置（config_user ?? config，标注 customized）；--seed 看 seed 原文
  myday item query --view <id> [--keyword 关键词] [--limit n] --json
  ```
- **`myday stats --json` 固定按内置 seed 配置求值**（忽略 `config_user` 与隐藏
  状态）——机器消费的稳定契约，不随 UI 定制漂移；要看用户生效结果用
  `item query --view`。`stats_summary` 退役后信封保持兼容。「固定」指 seed
  **配置结构**；动态规则（streak 随 pinned 模板、series 随活跃 number 字段）
  按当前数据展开——输出内容随数据变，配置结构不随 UI 定制变。
- 同结果契约：**同一时刻 + 同一本地时区**下，`item query` 与 GUI 同 id 同结果
  （相对日期 / 今天锚点随时刻走；信封带 `evaluated_at` / `tz`，§10）。
- `view_save` 传内置 id = 写 `config_user`（与 GUI 编辑一致，可脚本化，不拒绝）；
  传用户 id = 覆盖其 `config`；恢复 seed 用 `view_reset`。

## 13. 分期实施

| 期 | 内容 | 完成判据 |
|---|---|---|
| P1 core | `view_defs` 表 + 种子 + 求值引擎 + CLI + Rust 测试（运算符矩阵 / 种子语义对齐 `tasks_view`） | **GUI 代码不动**，种子语义测试对齐 |
| P2 搜索页 | 工具条 + builder + 另存视图 | 需求 §11「标签筛选、日期范围」落地 |
| P3 记录页 + 统计 | 记录页工具条；三内置挂件迁移；`stats_summary` 退役；series x 轴时间刻度 | 统计功能等价（挂件化），观感可为工具条 / 卡片编辑合理调整 |
| P4 待办页 + 图表 | 四 tab → 内置视图；bar / pie 挂件 + **渲染器切换**（折线↔柱状↔热力，验证 §8 抽象）+ 频率统计预设 | 需求 §7.4 频率统计补齐；同挂件换图型不改数据配置 |
| P5 评估项 | visible fields 编辑、今天 / 日历接入、字段分组（看板）、深嵌套 | 单独立项 |

**验收标准**：

1. 零自定义时全部面板**功能等价**：信息与能力不缩水、无新增必经操作、无新增
   需解释的概念；观感允许合理调整，有意行为差异逐条列明（已知：series x 轴
   刻度）。「需解释」的判定含跨面板学习成本：筛选 / 排序 / 换视图在各面板
   交互相同（共享组件实现，§11），学一次全站通用。
2. 30 秒内自建「跑步连续」打卡卡且不依赖模板（新建挂件 → 条件来源 → 保存）。
3. 趋势不再混线（series 可绑定模板 / 标签条件；多来源各自成线）。
4. `myday item query --view` 与 GUI 同 id 同结果。
5. 图表抽象成立：同一挂件折线↔柱状↔热力（presence）切换不改数据配置（§8 示例）。
6. E2E：自建视图 → 筛选 → 保存 → 重开仍在；移除打卡卡不再出现；重置内置恢复。
7. 回归：快速添加 / Ctrl+N / 模板一键记录路径零变化。
8. 性能：遵循 §10 求值顺序与实现约定即可，不设前置基准；出现实测卡顿再按增长路径优化。

## 14. 修订历史

- v1（2026-09-17）：初稿定稿。模型：条件 AST（两层）+ 数据集 + 聚合 + 渲染器；
  存储 `view_defs`；内置视图 seed 进表；统计挂件动态内置可定制；性能三约束与预算。
- v1.1（2026-09-17）：① anchor 收窄为纯排序 / 分组键，「无时间待办 = 收件箱」
  从隐藏 SQL 行为显式化为可编辑条件（§4.1 / §9）；② 性能改为「先不建基准测试，
  实测卡顿再按增长路径优化」（§10）；③ 挂件改为三段管线：聚合 → 统一结果表 →
  派生指标 → 渲染器，渲染器即时互换（折线↔柱状↔热力 presence），入验收 §13-5。
- v1.2（2026-09-17）：§4.1 补 anchor 选型理由：task 截止优先而非 start 语义；
  业务时间与系统时间分工（为何不用 created_at / updated_at 当锚点，anchor 可空
  即收件箱信息）；标注与 ListFilter 窗口锚点（task = start_at）的差异为有意。
- v1.3（2026-09-17）：评审修正八项——①求值顺序定死（keyword 编译入 AST →
  预筛松化为超集 → AST 终审 → 排序 → limit 最后，预筛层不施加 limit，§10）；
  ②重复条目分母条目 / 实例两模式，实例模式窗口必填、禁止无界展开（§3）；
  ③anchor 统一为普通字段引用（可条件 / 排序 / 分组），消解与 §9 的矛盾
  （§4.1 / §4.3）；④挂件去 limit 改 window，streak 的 current/longest 全历史
  特例（§8）；⑤agg 拆 group / metric 双槽位，values 限 log、字段分组写法明确、
  multiselect 每值一桶（§8）；⑥内置视图 config + config_user 双列，升级只覆盖
  seed 列，定制不丢，重置 = 清空 config_user（§7）；⑦被引对象失效统一规则：
  按存储值求值 + 标黄 / 标红，引擎不改写配置（§7）；⑧共享扫描定义为预筛并集
  超集的优化，构造不出公共超集时逐视图预筛，正确性优先（§10）。
- v1.4（2026-09-17）：求值契约补遗（P1 前定死）——①母条目模式时间条件作用
  存储值，补两条 store 层不变量（重复待办 due 恒当前期 → 待办 tab 不走实例
  模式；重复日程存储即锚点期，「这次发生」必须实例模式，§3）；②「今天」种子
  修正为 `anchor before 明天 或 anchor 空`（原条件漏掉「无截止 + 开始在今天 /
  过去」分支，与 tasks_view Today 逐分支等价，§9）；③日期值分点值 / 范围值
  两型，配对封闭：on / before / after 接点值，between 接双点值，新增 within
  接范围值，非法组合 = 参数错误（§4.2 / §4.3）；④结果信封带 evaluated_at +
  tz，跨午夜 / 时区变更失效重查；CLI-GUI 同结果前提 = 同时刻同时区
  （§10 / §12）；⑤隐藏状态归 `config_user`，新增「恢复动态」入口
  （§7 / §8）；⑥`myday stats --json` 固定按 seed 配置求值，不随 UI 定制漂移；
  `view show` 缺省生效配置、`--seed` 看 seed（§12）；⑦分组空值单独成桶恒
  末尾（§5）；合成列（anchor）不下推（§10）；⑧编辑内置视图提示「长期定制
  另存为」（§11）。
- v1.5（2026-09-17）：P1 前最后一批契约补清——①母条目不变量写明保证机制：
  唯一写路径（complete_task / 取消完成）+ P1 在 store 入口补
  「task+recurrence ⇒ due 必填」统一校验，非 DB 兜底；改重复待办 due = 改
  当前期，不变量不破（§3）；②挂件管线定序 filter → window → 聚合（AND 可
  交换），预筛取两者超集（§10）；③统计页共享扫描取最大窗口，streak 全历史
  在同一份内存集上按各自 filter 求值，同源不重复扫（§10）；④values 禁 group
  标注为有意限制，多线走 series 组，group×values 留 P5（§8）；⑤跨午夜检测
  触发机制：聚焦 / visibilitychange / 60 秒定时器（§10）；⑥小项：混合端点
  不放宽 between、用条件组合表达；日期条件对 NULL 恒假；view list 带
  customized 标志；stats「固定 seed」= 配置结构固定、动态规则按当前数据
  展开；unit 缺省取字段定义、options 仅覆盖；编辑提示改编辑器底部常驻小字
  （§4.2 / §8 / §11 / §12）。
- v1.6（2026-09-17）：实施语义补钉——①重复任务不进收件箱（有意，due 必填
  ⇒ anchor 恒非空）；②回拨语义钉死 = 恢复最近一次推进前存储值（记账键
  `recurred_done_at`，单期、消费即清），update_item 直改 status 的取消旁路
  列为已知差异、P1 统一到 uncomplete_task（§3）；③status 值域封闭 todo/done，
  「未完成」= eq todo，无第三态（§4.3）；④待办 tab 与今天页的「一致」限定为
  母条目集合一致，实例级留 P5（§9）；⑤预筛下推规则定死：AND 链合取、OR 组
  整组下推取分支并集、跨列 OR / 含不可下推子项则放弃该子树，多时间列各填
  各的列窗（§10）；⑥承认 streak 整页 all 耦合并留窗口分层增长路径（§10）；
  ⑦view_save 传内置 id = 写 config_user（不拒绝）；customized 仅对内置视图
  输出（§12）；⑧转显式时同步清理 hidden_templates，显式与隐藏不共存；
  config_user 不写合并/迁移防御逻辑（§7）；⑨小项：window/last_days 为滚动
  窗口、N ≥ 1；虚拟实例身份留 P5、不得自行发明；§2 图注标明挂件 dataset 为
  简化数据集（§2 / §3 / §4.2）。
- v1.7（2026-09-17）：验收标准放宽——「GUI 行为零变化 / 观感一致」改为
  **功能等价**：信息与能力不缩水、无新增必经操作、无新增需解释的概念；
  观感可为工具条 / 挂件编辑合理调整，有意行为差异逐条列明。捕获路径
  （快速添加 / 打字回车）仍要求零变化（§0 / §13）。
- v1.8（2026-09-17）：功能等价的判定补入**跨面板学习成本**：筛选 / 排序 /
  换视图 / 挂件编辑在全部面板为同一套交互（同一共享组件的不同实例，面板只
  传参，一致性靠复用不靠约定），学一次全站通用（§0 / §11 / §13）。
- v1.9（2026-09-17）：实施完成（P1 core / P2 搜索页 / P3 记录页 + 统计 /
  P4 待办页 + 渲染器切换）。实施对齐本文语义；落地差异与补充：
  ① schema v4 → v5 为**附加式迁移**（补 `view_defs` 表 + 种子，不重建不动数据；
  更旧结构仍走备份重建）；② 多值字段（tags / multiselect）在矩阵外补
  `contains` / `not_contains`（子串命中任一成员）——关键词编译在标签上的
  「字段值 contains」需要它与 search 的 LIKE 口径一致；`none` 对空值恒真
  （空集合不含任何指定值）；③ 编辑内置视图 = 写 `config_user` 完整副本；
  统计行 `config_user` 合法形态 = `{"explicit":[…]}` / `{"hidden_templates":[…]}`
  / `{"hidden":true}`（显式与隐藏不共存，转显式即整行替换）；④ CLI `stats`
  的 `--days` 覆盖 seed 挂件的 window（「固定 seed」指配置结构，§12）；
  ⑤ series x 轴时间刻度落地（§13 P3 有意差异）；⑥ 前端跨午夜 / 时区检测
  为全局 60s 定时 + focus + visibilitychange，命中才整体重查（§10.6）；
  P5 项（visible 编辑、今天 / 日历接入、看板分组、group×values）未做。
- v2.0（2026-09-17）：统计页改**容器模型**——§2 概念图在挂件外新增一层：
  `统计页 = 容器列表；容器 = 布局（horizontal | vertical）+ 挂件列表`。
  动机：原三个内置行（热力图 / 打卡连续 / 数值趋势）是三套特判组件，没有
  通用性；预置页面必须能被用户沿同一条路径（＋容器 → ＋挂件）手动搭出来，
  而不是代码里的例外。要点：
  ① stats 视图行配置 = `{kind:"container", layout, dynamic?, window?, goal?,
  widgets:[…]}`；旧版 `{kind:"widget"}` 兼容解析为纵向容器 × 1 挂件，
  旧版 `config_user.explicit` 键等价于 `widgets`。
  ② 三个预置容器只是预置组合：热力图 = 纵向 × 1 显式挂件；打卡连续 = 横向 ×
  动态规则（每个 pinned log 模板一张卡）；数值趋势 = 纵向 × 动态规则（每个
  活跃 number 字段一条线，各自成挂件，不再整行共享一条选中线）。
  ③ ＋挂件是**相对于容器**的：显式挂件追加进 `widgets`；动态卡编辑 = 原 key
  记入 `hidden_templates` + 编辑结果落为显式（同容器不出现双状态）；
  `config_user` 部分合并语义（layout / window / goal / widgets / 
  hidden_templates / hidden 键级覆盖），未知键拒绝。
  ④ 水平布局每行并排上限 4 张（宽度限制，超出换行），容器可随时横↔纵切换
  （config_user.layout / 用户容器直接改配置）。
  ⑤ series 趋势从「chips 选一条」改为每字段一条独立线挂件（垂直堆叠）——
  有意行为差异：与用户手搭的线挂件完全同构。
  ⑥ 挂件渲染统一走通用 WidgetView 组件（card/bar/line/pie/heatmap），
  无面板特判；菜单开合状态提升到页面层，数据刷新不丢。
  CLI / IPC 契约不变：`stats_summary` 信封仍按 seed 容器求值，输出不变。
- v2.1（2026-09-17）：统计页再收敛——**「内置 / 隐藏 / 动态」概念全部退场**。
  ① 预置容器 = 开库时替用户建好的**普通容器**（builtin=0，一次性播种，
  settings.stats_presets_seeded 标记；此后删除即删除，不再自动复活），与
  自建容器完全同权：可编辑、可物理删除；§7 的「内置不可删」不再适用于统计页。
  ② 动态规则（streaks/series 自动展开）删除——预设卡 / 线在播种与恢复时按
  **当前数据**物化为显式挂件（v1 的「此后新 pinned 模板自动出现打卡卡」行为
  取消：想要新模板的卡，自己 ＋挂件 选模板条件）；`hidden_templates` 随之
  删除，× 移除即从容器 widgets 里真删。
  ③ 「恢复默认统计页」= **完全重置**：清除当前全部统计容器与挂件（含用户
  新增 / 修改，不影响任何条目数据），按当前数据铺回三预设。不常用，UI 藏在
  页面底部 + 确认弹窗。不再有 v2 的「补缺不重复铺」逻辑。
  ④ 统计行不再有 config_user——和其他视图一样整配置替换；builtin /
  config_user 语义仅保留在记录 / 待办 / 搜索的面板视图（视图切换 + 默认
  标记领域）。
  ⑤ `myday stats --json` 契约更干净：按**代码里的预设定义**求值，根本不读
  view_defs 的统计行——页面怎么改 / 删都不影响机器消费的信封。
  ⑥ 挂件编辑 = 手机小组件式：容器头「编辑挂件」进入编辑态——卡片整卡可点
  （打开编辑器）、右上角 × 移除、末尾 ＋ 空位新增，「完成」退出；
  v1.9 的动态卡「转显式 / 记隐藏」双轨语义随之删除。
- v2.2（2026-09-17）：**兼容代码清场**（正式版前不做旧数据迁移）+ 统计页
  交互收尾。
  ① 旧形态兼容全删：`{kind:"widget"}` 统计行兼容解析、v1/v2 统计行迁移
  （builtin→普通行合并 config_user、hidden 行物理删除）及
  `merge_legacy_container` 整体移除——`parse_container` 只认
  `{kind:"container", layout, widgets}`，其他一律参数错误。播种只在库中
  尚无任何 stats 行时铺一次（settings 标记防重），无迁移分支。
  ② 统计页**全局范围档 chips（近 30/90/180/365 天）删除**：窗口完全由各
  挂件自身 window 决定（缺省 365 不变，§8）；`query_stats_page` 去掉
  days 参数，前端 / IPC / mock 同步。CLI `stats --days` 契约不变
  （stats_summary 仍按天数覆盖预设挂件窗口，§12）。
  ③ 水平容器挂件尺寸限定：min 220 / 最佳 250 / **max 340** px，flex 换行；
  容器宽度封顶 ≈ 4 张——「每行最多 n 张」由最大宽度推出，卡片不再随窗口
  无限拉伸。
  ④ 容器 ⋯ 菜单新增「重命名」：标题原地变输入框，Enter / 失焦保存，
  Esc 取消；走 `view_save(name)`，仅普通容器（统计行都是）。
  ⑤ 「恢复默认统计页」从页底小字上移到**页头操作行**（＋容器右侧，ghost
  按钮），确认弹窗保留——页底藏太深用户找不到。
  ⑥ 修 mock 缺陷：e2e-viewmock 返回对象里 `query_stats_page` /
  `stats_restore_defaults` 键重复，后一份（v2 的「补缺」语义）覆盖了前面
  的完全重置实现——删除重复键，E2E 与真实核心语义一致。

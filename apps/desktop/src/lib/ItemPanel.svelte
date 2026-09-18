<script lang="ts">
  /**
   * 统一条目面板：创建 / 编辑 / 模板 三态同一组件、同一套字段渲染。
   *
   * windowMode = 快速添加独立窗口（保存 / 取消后隐藏窗口）；
   * 主窗口在 Shell 中以弹层复用；设置页模板编辑同样复用（模板模式）。
   *
   * - 激活模型（定稿）：字段两态——chip（未激活，全部字段可见）/ 展开控件（激活）。
   *   点 chip 即激活；激活类型独特字段立即推断类型并隐藏冲突字段
   *   （截止→待办、开始/结束→日程、记录字段→记录、裸面板→待办）；
   *   × 取消激活后类型回退、隐藏字段回来。类型创建即定，保存前可改口一次；
   *   改口与已激活值冲突 → 保存报错，不静默清除。
   * - 激活瞬间注入默认值，优先级：模板 token（经锚点日解析）> 日历锚点日
   *   （开始=当天9:00、结束=当天18:00、截止=当天结束、发生=当天/现在）> 类型回退。
   * - 模板模式：填一遍面板存为模板。时间列存 @token（占位池，tpltime.ts 文法），
   *   字段值 / 标题 / 备注 / 状态为具体默认；「模板存意图，条目存事实」。
   * - 提醒 1:N（相对锚点快捷 + 自定义）；event 全天 / task 全天截止开关。
   * - 时间可清空（task）；文件拖入只记路径；Ctrl+V 粘贴截图。
   */
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    api,
    displayTitle,
    fmtDateTime,
    FILE_LINKS_KEY,
    fileLinksFromExtra,
    fromLocalInput,
    nowLocalInput,
    quickAddIdempotencyKey,
    toLocalInput,
    toDateInput,
    typeLabel,
    type FieldDef,
    type Item,
    type ItemPatch,
    type ItemStatus,
    type ItemType,
    type NewItem,
    type Template,
  } from "./api";
  import { allFieldDefs, displayFieldMap, fieldDefMap, fieldDefsFor } from "./fields.svelte";
  import {
    isPastNowToken,
    poolFor,
    resolveToken,
    resolveTimeValue,
    validateForColumn,
    type ResolveCtx,
  } from "./tpltime";
  import { parseRecurrence, recurrenceLabel, recurrenceToString } from "./recurrence";
  import { parseTimeHint, type TimeHint } from "./timewords";
  import { toast } from "./toast.svelte";
  import TimePopover, { timePopoverOpen } from "./TimePopover.svelte";
  import { closePanel, panelRequest } from "./panel.svelte";
  import { deletions } from "./deletion.svelte";
  import DeleteButton from "./DeleteButton.svelte";
  import FileLinkChips from "./FileLinkChips.svelte";
  import { t, q, type MessageKey } from "./i18n";

  let { windowMode = false }: { windowMode?: boolean } = $props();

  // windowMode 挂载后不变（由所在窗口决定），取初值即可
  // svelte-ignore state_referenced_locally
  const win = windowMode ? getCurrentWindow() : null;

  // 父组件在 panelRequest() 非空时才挂载本组件
  const req = panelRequest()!;
  const isEdit = req.mode === "edit";
  const isTpl = req.mode === "template";
  const src: Item | null = isEdit ? req.item : null;
  const editingTpl = isTpl ? req.template : null;
  /** 创建态锚点日（日历格子），激活时间字段时的默认值来源 */
  // svelte-ignore state_referenced_locally
  const anchorDay: string | null = req.mode === "create" ? (req.anchorDay ?? null) : null;

  // ---- 时间字段元数据 -----------------------------------------------------
  const TIME_FIELD_META = {
    start: { label: "panel.time.start", column: "start_at" },
    end: { label: "panel.time.end", column: "end_at" },
    due: { label: "panel.time.due", column: "due_at" },
    occurred: { label: "panel.time.occurred", column: "occurred_at" },
  } as const;
  type TimeFieldId = keyof typeof TIME_FIELD_META;
  const TIME_IDS = Object.keys(TIME_FIELD_META) as TimeFieldId[];

  function columnToTimeField(column: string): TimeFieldId {
    return (TIME_IDS.find((id) => TIME_FIELD_META[id].column === column) ?? "start") as TimeFieldId;
  }

  // ---- 一次性初值（编辑期间不跟随后台数据变化） --------------------------
  /** 保存前的「改口」："" = 按激活推断；编辑态恒为 ""（类型不可变） */
  let typeMan = $state<"" | ItemType>("");
  let tplSetType = $state(false); // 模板设置的类型：取消应用模板时回退
  let title = $state("");
  let note = $state("");
  // 时间值：创建/编辑态 = datetime-local 串；模板态 = 占位 token（"" = 未设默认）
  let startLocal = $state("");
  let endLocal = $state("");
  let dueLocal = $state("");
  let occurredLocal = $state("");
  let allDay = $state(false); // 全天日程：当天 00:00–23:59
  let allDayStart = $state(""); // 全天日期 YYYY-MM-DD
  let status = $state<ItemStatus | null>(null);
  /** 重复规则（SPRINT-SPEC §2）："" = 不重复；仅 event / task */
  let recurrenceSpec = $state("");
  let tags = $state<string[]>([]);
  let tagInput = $state("");
  let pastedImages = $state<{ blob: Blob; ext: string; url: string }[]>([]);
  /** 粘贴截图的大图预览（双击缩略图打开；object URL 仅本组件内使用） */
  let previewUrl = $state("");
  // 文件链接：只记路径不复制（extra[FILE_LINKS_KEY]）
  let fileLinks = $state<string[]>([]);
  let linkInput = $state("");
  let dragHover = $state(false);
  let saving = $state(false);
  let error = $state("");

  // ---- 激活状态：时间字段 + 自定义字段（键 = 字段 id） -------------------
  let activeTimes = $state<TimeFieldId[]>([]);
  const initialExtra: Record<string, unknown> = {};
  let extraVals = $state<Record<string, unknown>>({});
  let picked = $state<string[]>([]);
  let pickerSel = $state("");

  // ---- 模板模式元信息（模板态头部表单；编辑已有模板时从 defaults 回填） ----
  let tplName = $state("");
  let tplIcon = $state("");
  let tplTag = $state("");
  let tplNote = $state("");
  let tplPinned = $state(true);
  // 新建模板缺省待办（最常用）；记录类模板请显式切换
  let tplItemType = $state<ItemType>(isTpl ? (req.template?.item_type ?? "task") : "task");

  if (isEdit && src) {
    title = src.title ?? "";
    note = src.note ?? "";
    startLocal = toLocalInput(src.start_at);
    endLocal = toLocalInput(src.end_at);
    dueLocal = toLocalInput(src.due_at);
    occurredLocal = toLocalInput(src.occurred_at);
    allDay = src.all_day;
    allDayStart = src.start_at ? toDateInput(new Date(src.start_at)) : "";
    status = src.status ?? null;
    recurrenceSpec = src.recurrence ?? "";
    tags = [...src.tags];
    fileLinks = fileLinksFromExtra(src.extra);
    Object.assign(initialExtra, { ...(src.extra ?? {}) });
    // 已有字段值回填为「已激活」：extra 是整对象替换，不激活保存即丢值（编辑即所见）
    const activeDefs = fieldDefMap();
    for (const k of Object.keys(initialExtra)) {
      if (activeDefs.has(k) && !picked.includes(k)) picked.push(k);
    }
    // 有值的时间字段默认展开；空的可选字段保持 chip
    // （一次性取初值：面板按 req 挂载，编辑期间不跟随后台变化）
    // svelte-ignore state_referenced_locally
    if (startLocal) activeTimes.push("start");
    // svelte-ignore state_referenced_locally
    if (endLocal) activeTimes.push("end");
    // svelte-ignore state_referenced_locally
    if (dueLocal) activeTimes.push("due");
    // svelte-ignore state_referenced_locally
    if (occurredLocal) activeTimes.push("occurred");
  } else if (isTpl && editingTpl) {
    const tpl = editingTpl;
    tplName = tpl.name;
    tplIcon = tpl.icon ?? "";
    tplTag = tpl.tag ?? "";
    tplNote = tpl.note ?? "";
    tplPinned = tpl.pinned;
    tplItemType = tpl.item_type;
    title = typeof tpl.defaults.title === "string" ? tpl.defaults.title : "";
    note = typeof tpl.defaults.note === "string" ? tpl.defaults.note : "";
    allDay = tpl.defaults.all_day === true;
    for (const id of TIME_IDS) {
      const v = tpl.defaults[TIME_FIELD_META[id].column];
      // svelte-ignore state_referenced_locally
      if (typeof v === "string" && v) {
        activeTimes.push(id);
        setTimeValue(id, v); // 模板态存 token 原文
      }
    }
    Object.assign(initialExtra, { ...t.defaults });
    // svelte-ignore state_referenced_locally
    for (const k of Object.keys(initialExtra)) {
      if (!byColumnKey(k)) picked.push(k);
    }
    // 随模板启用但未设默认值的字段：回填为「已激活未填值」（编辑时可补值，也可仅占位）
    // svelte-ignore state_referenced_locally
    for (const f of (t.fields ?? []) as { id?: string }[]) {
      if (f.id && fieldDefMap().has(f.id) && !picked.includes(f.id)) picked.push(f.id);
    }
  } else if (req.mode === "create") {
    title = req.title ?? "";
    tags = [...(req.presetTags ?? [])];
    Object.assign(initialExtra, { ...(req.presetDefaults ?? {}) });
    // svelte-ignore state_referenced_locally
    for (const k of Object.keys(initialExtra)) picked.push(k);
    // svelte-ignore state_referenced_locally
    extraVals = { ...initialExtra };
    // svelte-ignore state_referenced_locally
    if (req.presetStart) {
      activeTimes.push("start");
      startLocal = toLocalInput(req.presetStart);
      // 拖选创建时段：结束同时预填（SPRINT2-SPEC §2）
      if (req.presetEnd) {
        activeTimes.push("end");
        endLocal = toLocalInput(req.presetEnd);
      }
    }
    // svelte-ignore state_referenced_locally
    if (req.presetDue) {
      activeTimes.push("due");
      dueLocal = toLocalInput(req.presetDue);
    }
  }
  // svelte-ignore state_referenced_locally
  extraVals = { ...extraVals, ...initialExtra };

  /** defaults 的列键（非字段 id） */
  function byColumnKey(k: string): boolean {
    return TIME_IDS.some((id) => TIME_FIELD_META[id].column === k) ||
      ["title", "note", "all_day", "due_all_day"].includes(k);
  }

  // ---- 模板（创建模式）状态：先声明（类型判定要读 activeTpl） -------------
  let templates = $state<Template[]>([]);
  let activeTpl = $state<Template | null>(null);
  /** 经 openCreate 预填的模板（如记录页无默认值模板 → 打开面板补值） */
  let presetTplId = $state<string | null>(
    req.mode === "create" ? (req.presetTemplateId ?? null) : null,
  );

  // ---- 类型：激活即推断（创建）；编辑锁定；模板模式跟随 tplItemType -------
  let inferred = $derived.by<ItemType>(() => {
    if (activeTimes.includes("due")) return "task";
    if (activeTimes.includes("start") || activeTimes.includes("end")) return "event";
    if (activeTimes.includes("occurred")) return "log";
    const byId = fieldDefMap();
    if (picked.some((id) => byId.get(id)?.scope === "log")) return "log";
    return "task";
  });
  let effType = $derived(
    isEdit
      ? req.item.type
      : isTpl
        ? tplItemType
        : (typeMan || (req.mode === "create" ? (req.item_type ?? inferred) : inferred)),
  );

  /** 类型未确认（裸面板）：无激活、无显式类型、无模板。此时全部字段可见，
   *  否则没有入口能点到其他类型的字段。 */
  let typeUndetermined = $derived(
    !isEdit &&
      !isTpl &&
      !typeMan &&
      activeTimes.length === 0 &&
      picked.length === 0 &&
      !(req.mode === "create" && req.item_type) &&
      !activeTpl,
  );

  // 字段列表跟随当前类型：类型未确认时全量可见（激活 log 字段是记录的入口）；
  // 确认后只留该类型字段，类型切走时收起不再适用的已激活字段（创建态连值清除）
  let defs = $derived(typeUndetermined ? allFieldDefs() : fieldDefsFor(effType));
  $effect(() => {
    const ok = new Set(defs.map((d) => d.id));
    if (picked.some((id) => !ok.has(id))) picked = picked.filter((id) => ok.has(id));
    if (isEdit || isTpl) return; // 编辑 / 模板态不隐性清值（丢失数据）
    const visible = TIME_IDS.filter((id) => timeVisible(id));
    if (activeTimes.some((id) => !visible.includes(id))) {
      for (const id of activeTimes) {
        if (!visible.includes(id)) setTimeValue(id, "");
      }
      activeTimes = activeTimes.filter((id) => visible.includes(id));
    }
  });

  /** 冲突隐藏：类型确认后只留该类型的字段。
   *  event 留开始/结束；截止即确认待办 → 只留截止（开始/结束/发生全隐藏）；
   *  log 只留发生时间。编辑态有值的字段始终可见（避免隐性清数据）。 */
  function timeVisible(id: TimeFieldId): boolean {
    if (isEdit && !!timeValue(id)) return true;
    if (typeUndetermined) return true;
    if (effType === "event") return id === "start" || id === "end";
    if (effType === "task") return id === "due";
    return id === "occurred";
  }

  function timeValue(id: TimeFieldId): string {
    return id === "start" ? startLocal
      : id === "end" ? endLocal
      : id === "due" ? dueLocal
      : occurredLocal;
  }
  function setTimeValue(id: TimeFieldId, v: string) {
    if (id === "start") startLocal = v;
    else if (id === "end") endLocal = v;
    else if (id === "due") dueLocal = v;
    else occurredLocal = v;
  }

  // ---- 激活 / 取消激活：激活瞬间注入默认值 --------------------------------
  function activateTime(id: TimeFieldId) {
    if (activeTimes.includes(id)) return;
    activeTimes = [...activeTimes, id];
    // 模板态值由用户从占位池选；编辑态初值已载入
    if (!isEdit && !isTpl && !timeValue(id)) setTimeValue(id, defaultForTime(id));
  }
  function deactivateTime(id: TimeFieldId) {
    if (isEdit && effType === "event" && (id === "start" || id === "end")) return; // 必填不可撤
    activeTimes = activeTimes.filter((x) => x !== id);
    setTimeValue(id, "");
    if (id === "start" || id === "end") allDay = false;
  }

  /** 激活瞬间的时间默认值：模板 token > 日历锚点日 > 类型回退 */
  function defaultForTime(id: TimeFieldId): string {
    const tpl = activeTpl ??
      (presetTplId ? templates.find((x) => x.id === presetTplId) ?? null : null);
    const tplV = tpl?.defaults?.[TIME_FIELD_META[id].column];
    if (typeof tplV === "string" && tplV) {
      const r = resolveTimeValue(tplV, resolveCtx(id === "end"));
      if (r) return toLocalInput(r);
    }
    if (anchorDay) {
      const [y, m, d] = anchorDay.split("-").map(Number);
      if (id === "start") return toLocalInput(new Date(y, m - 1, d, 9, 0).toISOString());
      if (id === "end") return toLocalInput(new Date(y, m - 1, d, 18, 0).toISOString());
      if (id === "due") return toLocalInput(new Date(y, m - 1, d, 23, 59).toISOString());
      // 记录不能发生在未来：过去日取当日中午，今天起取现在
      if (anchorDay <= toDateInput(new Date())) {
        return toLocalInput(new Date(y, m - 1, d, 12, 0).toISOString());
      }
      return nowLocalInput();
    }
    if (id === "start" && effType === "event") return nextHour();
    if (id === "occurred") return nowLocalInput();
    return "";
  }

  // ---- 时间组：开始+结束只属于日程且成对存在，归并为一颗 chip -------------
  const TIME_GROUP_META = {
    time: { label: "panel.group.time", members: ["start", "end"] as TimeFieldId[] },
    due: { label: "panel.time.due", members: ["due"] as TimeFieldId[] },
    occurred: { label: "panel.time.occurred", members: ["occurred"] as TimeFieldId[] },
  } as const;
  type TimeGroupId = keyof typeof TIME_GROUP_META;
  const TIME_GROUP_IDS = Object.keys(TIME_GROUP_META) as TimeGroupId[];
  const groupOf = (id: TimeFieldId): TimeGroupId =>
    id === "start" || id === "end" ? "time" : (id as TimeGroupId);
  function groupVisible(g: TimeGroupId): boolean {
    return TIME_GROUP_META[g].members.some(timeVisible);
  }
  function groupActive(g: TimeGroupId): boolean {
    return TIME_GROUP_META[g].members.some((m) => activeTimes.includes(m));
  }
  function activateGroup(g: TimeGroupId) {
    TIME_GROUP_META[g].members.forEach(activateTime);
  }
  function deactivateGroup(g: TimeGroupId) {
    TIME_GROUP_META[g].members.forEach(deactivateTime);
  }

  // ---- 时间快捷标签：同一人话标签按所在行映射到不同 token，点击即算 -------
  // 开始行基准 = 当前时刻 / 锚点日；结束行基准 = 开始时间；时段标签同时填首尾。
  // 全天是时段标签而非开关（选中后开始/结束变日期输入，存 all_day + 00:00–23:59）。
  const START_LABELS: { label: MessageKey; token: string }[] = [
    { label: "panel.start.nextHour", token: "@next_hour" },
    { label: "panel.start.plus1h", token: "@now+1h" },
    { label: "panel.start.tomorrow9", token: "@d+1T09:00" },
  ];
  const END_LABELS: { label: MessageKey; token: string }[] = [
    { label: "panel.end.plus30m", token: "@start+30m" },
    { label: "panel.end.plus1h", token: "@start+1h" },
    { label: "panel.end.plus2h", token: "@start+2h" },
  ];
  const RANGE_LABELS: { label: MessageKey; start: string; end: string; allDay?: boolean }[] = [
    { label: "panel.range.allDay", start: "@d0", end: "@d0Tend", allDay: true },
    { label: "panel.range.work", start: "@d0T09:00", end: "@d0T18:00" },
    { label: "panel.range.lunch", start: "@d0T12:00", end: "@d0T14:00" },
  ];

  /** token 解析上下文：开始行/时段用当前与锚点日；结束行再带上已填的开始 */
  function resolveCtx(withStart = false): ResolveCtx {
    return {
      now: new Date(),
      anchorDay,
      start: withStart && startLocal ? new Date(fromLocalInput(startLocal)) : null,
    };
  }
  function fillStart(token: string) {
    const iso = resolveToken(token, resolveCtx());
    if (iso) {
      allDay = false;
      setTimeValue("start", toLocalInput(iso));
    }
  }
  function fillEnd(token: string) {
    const iso = resolveToken(token, resolveCtx(true));
    if (iso) setTimeValue("end", toLocalInput(iso));
  }
  function fillRange(r: { start: string; end: string; allDay?: boolean }) {
    const s = resolveToken(r.start, resolveCtx());
    const e = resolveToken(r.end, resolveCtx());
    if (!s || !e) return;
    setTimeValue("start", toLocalInput(s));
    setTimeValue("end", toLocalInput(e));
    allDayStart = toDateInput(new Date(s));
    allDay = !!r.allDay;
  }

  // ---- 重复规则（SPRINT-SPEC §2）：时间字段激活后出现，「修改全部」语义 ----
  let recKind = $derived(parseRecurrence(recurrenceSpec)?.kind ?? "");
  let recLabel = $derived(recurrenceSpec ? recurrenceLabel(recurrenceSpec) : "");
  let recVisible = $derived(
    !isTpl &&
      !typeUndetermined &&
      (effType === "event"
        ? groupActive("time") || !!recurrenceSpec
        : effType === "task"
          ? groupActive("due") || !!recurrenceSpec
          : false),
  );
  function setRecKind(mode: "" | "daily" | "weekly" | "monthly") {
    if (mode === "") {
      recurrenceSpec = "";
      return;
    }
    const cur = parseRecurrence(recurrenceSpec);
    if (mode === "daily") recurrenceSpec = "@daily";
    else if (mode === "weekly")
      recurrenceSpec = recurrenceToString({
        kind: "weekly",
        n: cur?.kind === "weekly" ? cur.n : anchorWeekday(),
      });
    else recurrenceSpec = recurrenceToString({ kind: "monthly", d: cur?.kind === "monthly" ? cur.d : 1 });
  }
  /** 「每周…」的默认星期 = 当前开始/截止日期的星期（无值则周一） */
  function anchorWeekday(): number {
    const v = startLocal || dueLocal;
    if (!v) return 1;
    const wd = new Date(fromLocalInput(v)).getDay();
    return wd === 0 ? 7 : wd;
  }
  function setWeeklyDay(n: number) {
    recurrenceSpec = recurrenceToString({ kind: "weekly", n });
  }
  function setMonthlyDay(d: number) {
    const clamped = Math.min(31, Math.max(1, Math.round(d) || 1));
    recurrenceSpec = recurrenceToString({ kind: "monthly", d: clamped });
  }

  // ---- 受控 NL 时间提示（SPRINT-SPEC §8）：只出 chip，Tab/点击才应用 --------
  // 仅创建态且未激活任何时间字段时出现（避免与手填值冲突）
  let timeHint = $derived<TimeHint | null>(
    !isTpl && !isEdit && activeTimes.length === 0 ? parseTimeHint(title) : null,
  );
  function applyTimeHint() {
    const h = timeHint;
    if (!h) return;
    if (h.dateOnly) {
      // 纯日期词 → 截止（待办语义），默认当天 23:59
      activateTime("due");
      const d = new Date(h.date);
      d.setHours(23, 59, 0, 0);
      setTimeValue("due", toLocalInput(d.toISOString()));
    } else {
      // 带时刻 → 开始（激活时间组，识别为日程）
      activateTime("start");
      if (allDay) allDay = false;
      setTimeValue("start", toLocalInput(h.date.toISOString()));
    }
    // 应用 = 用户显式确认：把时间词从标题剥离（时间已落到字段，标题留着会过时误导）
    title = title.replace(h.matched, " ").replace(/\s+/g, " ").trim();
    // 应用后 activeTimes 非空，chip 随派生条件自然消失
  }

  $effect(() => {
    if (isEdit || isTpl) return;
    api
      .listTemplates()
      .then((ts) => (templates = ts))
      .catch(() => {});
  });

  // 记录页「补值」入口：预设模板到手后自动应用一次（激活其字段并注入默认）。
  // 不走 applyTemplate：其 toggle 分支会把「已按 presetTplId 预置」误判为取消。
  $effect(() => {
    if (isEdit || isTpl || !presetTplId || activeTpl || templates.length === 0) return;
    const tpl = templates.find((x) => x.id === presetTplId);
    if (tpl) {
      activeTpl = tpl;
      applyDefaults(tpl);
    }
  });

  // ---- 文件链接：整窗拖入 / 手动粘路径 -----------------------------------
  function addLinks(paths: string[]) {
    const next = [...fileLinks];
    for (const p of paths) {
      const clean = p.trim();
      if (clean && !next.includes(clean)) next.push(clean);
    }
    fileLinks = next;
  }

  $effect(() => {
    if (isTpl) return; // 模板模式不收附件
    // 拖放是窗口级的：面板开着时，文件拖进窗口任意位置都算。
    // onDragDropEvent 异步返回反注册函数，注意清理时机
    let un: (() => void) | null = null;
    let disposed = false;
    void getCurrentWindow()
      .onDragDropEvent((e) => {
        if (e.payload.type === "enter") dragHover = true;
        else if (e.payload.type === "leave" || e.payload.type === "drop") {
          dragHover = false;
          if (e.payload.type === "drop" && e.payload.paths.length) addLinks(e.payload.paths);
        }
      })
      .then((f) => {
        if (disposed) f();
        else un = f;
      });
    return () => {
      disposed = true;
      un?.();
    };
  });

  // ---- 时间快捷值 --------------------------------------------------------
  function nextHour(): string {
    const d = new Date();
    d.setMinutes(0, 0, 0);
    d.setHours(d.getHours() + 1);
    return toLocalInput(d.toISOString());
  }
  function tomorrow(): string {
    const d = new Date();
    d.setDate(d.getDate() + 1);
    d.setHours(23, 59, 0, 0);
    return toLocalInput(d.toISOString());
  }

  // ---- 渐进披露：创建时默认只露「标题 + 模板」，时间/字段/标签收进「更多」。
  // 一旦有内容（激活字段、标签、备注）或处于编辑/模板态则自动展开。
  // 展开状态记忆在设置库，跨面板打开 / 应用重启保持上次的选择；
  // 附件区（拖文件 / 粘贴截图）常驻可见，不参与折叠
  let more = $state(false);
  if (!isEdit && !isTpl) {
    void api
      .getSetting("quick_add_expanded")
      .then((v) => {
        // 只在尚未展开时应用（若 preset 模板已先行强制展开，以其为准）
        if (v === "1" && !more) more = true;
      })
      .catch(() => {});
  }
  /** 切换展开并落库；应用模板也走这里强制展开 */
  function setExpanded(v: boolean) {
    more = v;
    void api.setSetting("quick_add_expanded", v ? "1" : "0").catch(() => {});
  }
  let showDetails = $derived(
    isEdit ||
      isTpl ||
      more ||
      activeTimes.length > 0 ||
      picked.length > 0 ||
      tags.length > 0 ||
      !!note,
  );

  // ---- 字段 / 标签 / 模板操作 --------------------------------------------
  function pickField() {
    const id = pickerSel;
    if (!id || picked.includes(id)) return;
    picked = [...picked, id];
    pickerSel = "";
  }
  function dropField(id: string) {
    picked = picked.filter((x) => x !== id);
    const next = { ...extraVals };
    delete next[id];
    extraVals = next;
  }
  function setField(id: string, v: unknown) {
    extraVals = { ...extraVals, [id]: v };
  }

  /** 模板可应用：类型未定（裸面板）时全部可用（应用即定类型），类型已定只留同型。 */
  function tplUsable(tpl: Template): boolean {
    return typeUndetermined || tpl.item_type === effType;
  }

  /** 模板 defaults 应用：类型即定（§5.8 规则 1），列键写输入，字段 id 写 extra。
   *  时间键走 activateTime（激活即注入 token 解析后的默认值）。 */
  function applyDefaults(tpl: Template) {
    const byId = fieldDefMap();
    typeMan = tpl.item_type;
    tplSetType = true;
    for (const [k, v] of Object.entries(tpl.defaults ?? {})) {
      switch (k) {
        case "title":
          if (!title.trim()) title = String(v ?? "");
          continue;
        case "note":
          if (!note.trim()) note = String(v ?? "");
          continue;
        case "all_day":
          if (v === true) allDay = true;
          continue;
        case "start_at":
        case "end_at":
        case "due_at":
        case "occurred_at": {
          const id = columnToTimeField(k);
          if (!activeTimes.includes(id)) activateTime(id);
          continue;
        }
        default:
          break;
      }
      if (byId.has(k)) {
        if (!picked.includes(k)) picked = [...picked, k];
        extraVals = { ...extraVals, [k]: v };
      }
    }
    // 随模板启用但未设默认值的字段：只激活不填值（应用时现场填）
    for (const f of (tpl.fields ?? []) as { id?: string }[]) {
      if (f.id && byId.has(f.id) && !picked.includes(f.id)) picked = [...picked, f.id];
    }
    // 已停用（软删）的模板字段不静默消失——提示用户本次未启用
    const deadFields = ((tpl.fields ?? []) as { id?: string }[]).filter(
      (f) => f.id && !byId.has(f.id),
    ).length;
    if (deadFields > 0) toast.show(t("panel.toast.disabledFields", { n: deadFields }));
    if (tpl.tag && !tags.includes(tpl.tag)) tags = [...tags, tpl.tag];
    // 应用模板 = 直接展开成预填好的内容（含记录页「补值」预设路径），并记住展开态
    setExpanded(true);
  }

  function unapplyTemplate(tpl: Template) {
    const byId = fieldDefMap();
    for (const [k, v] of Object.entries(tpl.defaults ?? {})) {
      switch (k) {
        case "title":
          if (title === v) title = "";
          continue;
        case "all_day":
          if (v === true && allDay) allDay = false;
          continue;
        case "start_at":
        case "end_at":
        case "due_at":
        case "occurred_at": {
          const id = columnToTimeField(k);
          const resolved = resolveTimeValue(v, { now: new Date(), anchorDay });
          // 用户改过就不动；仍是模板默认值才随模板一起撤掉
          if (resolved && timeValue(id) === toLocalInput(resolved)) deactivateTime(id);
          continue;
        }
        default:
          break;
      }
      if (byId.has(k)) dropField(k);
    }
    // 随模板启用但无默认值的字段：一并撤掉（有默认值的上面已随 defaults 撤过，重复无害）
    for (const f of (tpl.fields ?? []) as { id?: string }[]) {
      if (f.id && byId.has(f.id)) dropField(f.id);
    }
    if (tpl.tag) tags = tags.filter((x) => x !== tpl.tag);
  }

  function applyTemplate(tpl: Template) {
    if (activeTpl?.id === tpl.id || presetTplId === tpl.id) {
      unapplyTemplate(tpl);
      activeTpl = null;
      presetTplId = null;
      if (tplSetType) {
        typeMan = "";
        tplSetType = false;
      }
      return;
    }
    if (activeTpl) unapplyTemplate(activeTpl);
    activeTpl = tpl;
    applyDefaults(tpl);
  }

  function addTag() {
    const tag = tagInput.trim().replace(/^#/, "");
    if (tag && !tags.includes(tag)) tags = [...tags, tag];
    tagInput = "";
  }

  // ---- 提醒（1:N）：存意图 spec，不做一次性时刻 ---------------------------
  // spec = @token（相对，随条目开始/截止自动跟随）或 RFC3339 绝对（自定义时刻）。
  // 池按类型收窄：日程锚开始、待办锚截止（用户显式选择）+ 期间每日；记录通常不提醒。
  const REMINDER_POOL: { token: string; label: MessageKey; types: ItemType[] }[] = [
    { token: "@start", label: "panel.rem.atStart", types: ["event"] },
    { token: "@start-10m", label: "panel.rem.startBefore10m", types: ["event"] },
    { token: "@start-1h", label: "panel.rem.startBefore1h", types: ["event"] },
    { token: "@start-1d", label: "panel.rem.startBefore1d", types: ["event"] },
    { token: "@due", label: "panel.rem.atDue", types: ["task"] },
    { token: "@due-1h", label: "panel.rem.dueBefore1h", types: ["task"] },
    { token: "@due-1d", label: "panel.rem.dueBefore1d", types: ["task"] },
    { token: "@dailyT09:00", label: "panel.rem.daily9", types: ["event", "task"] },
  ];
  let reminders = $state<string[]>(src ? src.reminders.map((r) => r.spec) : []);
  let remPick = $state("");

  let reminderPool: { token: string; label: MessageKey }[] = $derived([
    ...REMINDER_POOL.filter((p) => p.types.includes(effType)),
    { token: "custom", label: "panel.reminders.custom" },
  ]);

  /** spec → 展示标签：token 查池（未收录的原文显示），绝对时刻格式化 */
  function reminderLabel(spec: string): string {
    if (spec.startsWith("@")) {
      const hit = REMINDER_POOL.find((p) => p.token === spec);
      return hit ? t(hit.label) : spec;
    }
    return spec ? fmtDateTime(spec) : t("panel.reminders.pickTime");
  }
  function isAbsoluteSpec(spec: string): boolean {
    return spec !== "" && !spec.startsWith("@");
  }

  function addReminder() {
    const pick = remPick;
    remPick = "";
    if (!pick) return;
    // 自定义 = 空 spec 占位，由 datetime-local 填入具体时刻
    reminders = [...reminders, pick === "custom" ? "" : pick];
  }
  function setReminderSpec(i: number, spec: string) {
    reminders = reminders.map((s, x) => (x === i ? spec : s));
  }
  function removeReminder(i: number) {
    reminders = reminders.filter((_, x) => x !== i);
  }

  // ---- 粘贴截图（创建） ---------------------------------------------------
  async function onPaste(e: ClipboardEvent) {
    if (isEdit || isTpl) return;
    const files = e.clipboardData?.items ?? [];
    const fromEvent: { blob: Blob; ext: string; url: string }[] = [];
    for (const item of Array.from(files)) {
      if (item.type.startsWith("image/")) {
        const blob = item.getAsFile();
        if (!blob) continue;
        const ext = item.type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
        fromEvent.push({ blob, ext, url: URL.createObjectURL(blob) });
      }
    }
    if (fromEvent.length) {
      e.preventDefault();
      pastedImages = [...pastedImages, ...fromEvent];
      return;
    }
    // 剪贴板里是文本：走系统默认粘贴，不做图片兜底
    if (Array.from(files).some((i) => i.type.startsWith("text/"))) return;
    // WebKitGTK(X11) 的 paste 事件可能不带图片项：回退异步剪贴板 API 读图
    if (!navigator.clipboard?.read) return;
    e.preventDefault();
    try {
      const items = await navigator.clipboard.read();
      const fromApi: { blob: Blob; ext: string; url: string }[] = [];
      for (const ci of items) {
        const type = ci.types.find((x) => x.startsWith("image/"));
        if (!type) continue;
        const blob = await ci.getType(type);
        const ext = type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
        fromApi.push({ blob, ext, url: URL.createObjectURL(blob) });
      }
      if (fromApi.length) pastedImages = [...pastedImages, ...fromApi];
    } catch {
      /* 剪贴板 API 不可用或被拒：无副作用 */
    }
  }

  async function uploadPastes(id: string) {
    for (const img of pastedImages) {
      const buf = new Uint8Array(await img.blob.arrayBuffer());
      let binary = "";
      for (const byte of buf) binary += String.fromCharCode(byte);
      await api.addAttachmentB64(id, btoa(binary), img.ext);
      URL.revokeObjectURL(img.url);
    }
    pastedImages = [];
  }

  // ---- 汇总字段值：只收活跃定义的键（值随 kind 严格校验由 core 兜底）；
  // 软删键原样透传（旧条目历史值不被编辑洗掉）；已「清理」的残留键不再回写
  // （core 对未知键报错）；系统记账键与文件链接单独处理
  function buildExtra(): Record<string, unknown> {
    const byId = fieldDefMap();
    const displayById = displayFieldMap();
    const out: Record<string, unknown> = {};
    if (isEdit) {
      for (const [k, v] of Object.entries(initialExtra)) {
        if (k === FILE_LINKS_KEY) continue;
        if (k === "recurred_done_at") {
          out[k] = v;
          continue;
        }
        if (!byId.has(k) && displayById.has(k)) out[k] = v;
      }
    }
    for (const id of picked) {
      const v = extraVals[id];
      if (v === null || v === undefined || v === "" || v === false) continue;
      if (Array.isArray(v) && v.length === 0) continue;
      out[id] = v;
    }
    if (fileLinks.length) out[FILE_LINKS_KEY] = [...fileLinks];
    return out;
  }

  // ---- 空标题自动生成：有内容就该能保存，别让"标题必填"挡住速记 -----------
  // 优先级：模板名 > 数值/开关字段 > 截图 > 文件名 > 时间戳速记
  let autoTitle = $derived.by<string>(() => {
    const tplId = activeTpl?.id ?? presetTplId;
    const tpl = tplId ? templates.find((x) => x.id === tplId) : null;
    if (tpl) return tpl.name;
    const byId = fieldDefMap();
    const parts: string[] = [];
    for (const id of picked) {
      const def = byId.get(id);
      const v = extraVals[id];
      if (!def || v === null || v === undefined || v === "" || v === false) continue;
      if (def.kind === "number") parts.push(`${def.name} ${v}`);
      else if (def.kind === "bool") parts.push(def.name);
    }
    if (parts.length) return parts.join(" · ");
    const now = new Date();
    const hm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;
    if (pastedImages.length) return t("panel.autotitle.shot", { time: hm });
    if (fileLinks.length) {
      const first = fileLinks[0].split("/").filter(Boolean).pop() ?? t("panel.autotitle.file");
      return fileLinks.length > 1
        ? t("panel.autotitle.files", { name: first, n: fileLinks.length })
        : first;
    }
    return t("panel.autotitle.quick", { time: hm });
  });
  let effTitle = $derived(title.trim() || autoTitle);

  /** 保存前校验：宁可报错，不静默改写 */
  function validate(): string {
    if (isTpl) {
      if (!tplName.trim()) return t("panel.validate.tplNameRequired");
      for (const id of activeTimes) {
        const tok = timeValue(id);
        if (!tok) continue;
        const e = validateForColumn(tok, TIME_FIELD_META[id].column);
        if (e) return e;
        if (id === "occurred" && !isPastNowToken(tok)) {
          return t("panel.validate.occurredToken");
        }
      }
      return "";
    }
    // 冲突检查：改口后与已激活值并存 → 报错（不静默清除）
    if (effType === "event" && (dueLocal || occurredLocal)) {
      return t("panel.validate.eventExtraTimes");
    }
    if (effType === "task" && occurredLocal) return t("panel.validate.taskOccurred");
    if (effType === "log" && (startLocal || endLocal || dueLocal)) {
      return t("panel.validate.logTimes");
    }
    if (effType === "event") {
      if (allDay) {
        if (!allDayStart) return t("panel.validate.allDayDate");
      } else {
        if (!startLocal) return t("panel.validate.eventStart");
        if (endLocal && fromLocalInput(endLocal) <= fromLocalInput(startLocal)) {
          return t("panel.validate.endAfterStart");
        }
      }
    }
    if (effType === "log") {
      if (!effTitle.trim() && !note.trim()) return t("panel.validate.logTitleOrNote");
      if (occurredLocal && fromLocalInput(occurredLocal) > new Date().toISOString()) {
        return t("panel.validate.logNotFuture");
      }
    }
    if (effType !== "log" && !effTitle.trim()) return t("panel.validate.titleRequired");
    return "";
  }

  async function saveCreate() {
    const common: NewItem = {
      item_type: effType,
      title: effTitle.trim() || null,
      note: note.trim() || null,
      tags: [...tags],
      extra: buildExtra(),
      reminders: reminders
        .filter((s) => s)
        .map((spec) => ({ spec })),
      ...(recurrenceSpec ? { recurrence: recurrenceSpec } : {}),
      ...(activeTpl || presetTplId
        ? { template_id: (activeTpl ?? { id: presetTplId }).id }
        : {}),
      idempotency_key: quickAddIdempotencyKey({
        t: effTitle,
        n: note,
        s: startLocal,
        e: endLocal,
        d: dueLocal,
        o: occurredLocal,
        x: buildExtra(),
        g: tags,
      }),
    };
    let item: Item;
    if (effType === "event") {
      const startIso = allDay
        ? fromLocalInput(`${allDayStart}T00:00`)
        : fromLocalInput(startLocal || nextHour());
      const endIso = allDay
        ? fromLocalInput(`${allDayStart}T23:59`)
        : endLocal
          ? fromLocalInput(endLocal)
          : new Date(new Date(startIso).getTime() + 3600_000).toISOString();
      item = await api.addItem({
        ...common,
        all_day: allDay || undefined,
        start_at: startIso,
        end_at: endIso,
      });
      // 冲突提示但不阻止（需求 §5）
      warnConflict(startIso, endIso, item.id);
    } else if (effType === "task") {
      item = await api.addItem({
        ...common,
        ...(startLocal ? { start_at: fromLocalInput(startLocal) } : {}),
        ...(dueLocal ? { due_at: fromLocalInput(dueLocal) } : {}),
      });
    } else {
      item = await api.addItem({
        ...common,
        occurred_at: occurredLocal ? fromLocalInput(occurredLocal) : new Date().toISOString(),
      });
    }
    await uploadPastes(item.id);
    // 空标题的「速记」兜底是特性，但要明确告知（否则像误触静默建了条目）
    if (!title.trim()) toast.show(t("panel.toast.quickSaved", { title: q(effTitle) }));
  }

  async function saveEdit(item: Item) {
    // 状态切换（仅待办，两态）：勾选 = 完成，取消勾选 = 重开
    if (effType === "task" && status !== item.status) {
      if (status === "done") await api.completeTask(item.id);
      else await api.updateItem(item.id, { status: "todo" });
    }
    const patch: ItemPatch = {
      title: effTitle.trim() || null,
      note: note.trim() || null,
      tags: [...tags],
      extra: buildExtra(),
    };
    // 重复规则变更（编辑 = 修改整个系列）
    if (recurrenceSpec !== (src.recurrence ?? "")) {
      if (recurrenceSpec) patch.recurrence = recurrenceSpec;
      else patch.clear_recurrence = true;
    }
    if (allDay !== item.all_day) patch.all_day = allDay;
    const remChanged =
      JSON.stringify(reminders) !== JSON.stringify(item.reminders.map((r) => r.spec));
    if (remChanged) {
      patch.reminders = reminders
        .filter((s) => s)
        .map((spec) => ({ spec }));
    }
    if (effType === "log") {
      if (occurredLocal) patch.occurred_at = fromLocalInput(occurredLocal);
    } else {
      if (allDay) {
        if (allDayStart) {
          patch.start_at = fromLocalInput(`${allDayStart}T00:00`);
          patch.end_at = fromLocalInput(`${allDayStart}T23:59`);
        }
      } else {
        if (startLocal) patch.start_at = fromLocalInput(startLocal);
        else if (item.start_at) patch.clear_start_at = true;
        if (endLocal) patch.end_at = fromLocalInput(endLocal);
        else if (item.end_at) patch.clear_end_at = true;
      }
      if (dueLocal) patch.due_at = fromLocalInput(dueLocal);
      else if (item.due_at) patch.clear_due_at = true;
    }
    await api.updateItem(item.id, patch);
    if (effType === "event" && item.start_at && item.end_at) {
      // 冲突提示但不阻止（需求 §5）；编辑面板冲突相对已保存的值判定
      warnConflict(item.start_at, item.end_at, item.id);
    }
  }

  /** 非阻断冲突警告（SPRINT-SPEC §3.2）：保存成功后提示，不回滚 */
  async function warnConflict(startIso: string, endIso: string, excludeId: string) {
    try {
      const hits = await api.checkConflict(startIso, endIso, excludeId);
      if (hits.length) {
        const names = hits.slice(0, 3).map(displayTitle).join(t("panel.toast.conflictSep"));
        const more = hits.length > 3 ? t("panel.toast.conflictMore", { n: hits.length }) : "";
        toast.show(t("panel.toast.conflict", { n: hits.length, names }) + (more ? ` ${more}` : ""));
      }
    } catch {
      /* 检测失败不影响保存 */
    }
  }

  /** 模板模式保存：面板当前值序列化为 defaults（时间列 = token，core 再校验）。 */
  async function saveTemplate() {
    const v = validate();
    if (v) {
      error = v;
      return;
    }
    const defaults: Record<string, unknown> = {};
    if (title.trim()) defaults.title = title.trim();
    if (note.trim()) defaults.note = note.trim();
    if (effType === "event" && allDay) defaults.all_day = true;
    // due_all_day UI 不再暴露（§6.3）：编辑已有模板时原值透传，避免保存静默丢失
    if (effType === "task" && initialExtra.due_all_day === true) defaults.due_all_day = true;
    for (const id of activeTimes) {
      const tok = timeValue(id);
      if (tok) defaults[TIME_FIELD_META[id].column] = tok;
    }
    for (const fid of picked) {
      const val = extraVals[fid];
      if (val === null || val === undefined || val === "" || val === false) continue;
      if (Array.isArray(val) && val.length === 0) continue;
      defaults[fid] = val;
    }
    // fields = 随模板启用的字段（含未设默认值的占位字段），以完整定义传给 core 物化
    const allDefs = fieldDefMap();
    const fields = picked
      .map((fid) => allDefs.get(fid))
      .filter((d) => !!d)
      .map((d) => ({ id: d.id, name: d.name, kind: d.kind, scope: d.scope, options: d.options }));
    saving = true;
    error = "";
    try {
      let id: string;
      if (editingTpl) {
        await api.updateTemplate(
          editingTpl.id,
          tplName.trim(),
          tplTag.trim() || null,
          tplIcon.trim() || null,
          tplItemType,
          defaults,
          fields,
          tplNote.trim() || null,
        );
        id = editingTpl.id;
      } else {
        id = (
          await api.addTemplate(
            tplName.trim(),
            tplTag.trim() || null,
            tplIcon.trim() || null,
            tplItemType,
            defaults,
            fields,
            tplNote.trim() || null,
          )
        ).id;
      }
      if (tplItemType === "log") await api.setTemplatePinned(id, tplPinned);
      finish();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function save() {
    if (saving) return;
    const v = validate();
    if (v) {
      error = v;
      return;
    }
    saving = true;
    error = "";
    try {
      if (isTpl) await saveTemplate();
      else if (isEdit && src) await saveEdit(src);
      else await saveCreate();
      finish();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function finish() {
    closePanel();
    win?.hide();
  }

  function cancel() {
    finish();
  }

  // ---- 编辑态类型转换（SPRINT2-SPEC §7 的第二个入口）：两步确认，成功后关面板。
  // 详情浮层之外，编辑面板也提供转换（否则「类型不可改」文案把用户锁死）
  let confirmConvert = $state<"" | "event" | "log">("");
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  async function convertType(target: "event" | "log") {
    if (!isEdit || !src) return;
    if (confirmConvert !== target) {
      confirmConvert = target;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmConvert = ""), 8000);
      return;
    }
    clearTimeout(confirmTimer);
    confirmConvert = "";
    try {
      if (target === "event") {
        const ev = await api.convertTaskToEvent(src.id);
        toast.show(t("panel.toast.toEventDone", { title: q(displayTitle(ev)) }));
      } else {
        const log = await api.eventToLog(src.id);
        toast.show(t("panel.toast.toLogDone", { title: q(displayTitle(log)) }));
      }
      finish();
    } catch (e) {
      error = String(e);
    }
  }

  function remove() {
    if (!src) return;
    void deletions.request(src);
    finish();
  }

  /** 打开面板即聚焦（快速录入：点日历格子 / 唤起窗口后直接打字）。
   *  窗口刚 show 时 webview 可能尚未拿到系统焦点，重试几次保证落上。
   *  enabled = false 时不动焦点（模板态聚焦的是模板名）。 */
  function focusOnMount(el: HTMLElement, enabled?: boolean) {
    if (enabled === false) return { destroy: () => {} };
    const timers = [0, 150, 400].map((ms) =>
      setTimeout(() => {
        if (document.activeElement !== el) el.focus();
      }, ms),
    );
    return {
      destroy: () => timers.forEach(clearTimeout),
    };
  }

  // ---- 独立窗口模式：顶缘拖动窗口，右下/下/右缘调整大小 -------------------
  type ResizeDir = "South" | "East" | "SouthEast";

  function startMove(e: MouseEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    void getCurrentWindow().startDragging();
  }

  function startResize(e: MouseEvent, dir: ResizeDir) {
    if (e.button !== 0) return;
    e.preventDefault();
    void getCurrentWindow().startResizeDragging(dir);
  }

  const resizeDirs: { dir: ResizeDir; cls: string }[] = [
    { dir: "South", cls: "s" },
    { dir: "East", cls: "e" },
    { dir: "SouthEast", cls: "se" },
  ];

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // 时间弹层开着时 Esc 只关弹层（弹层自带处理），不关面板
      if (timePopoverOpen.count > 0) return;
      // 预览打开时 Esc 只关预览，再按才关面板
      if (previewUrl) {
        previewUrl = "";
        return;
      }
      cancel();
    }
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      void save();
    }
  }

  function displayOf(item: Item): string {
    return displayTitle(item);
  }
</script>

<svelte:window onkeydown={onKeydown} onpaste={onPaste} />

{#snippet fieldValue(f: FieldDef)}
  {#if f.kind === "bool"}
    <label class="fcheck">
      <input
        type="checkbox"
        checked={extraVals[f.id] === true}
        onchange={(e) => setField(f.id, e.currentTarget.checked)}
      />
      {f.name}
    </label>
  {:else if f.kind === "select"}
    <select
      value={String(extraVals[f.id] ?? "")}
      onchange={(e) => setField(f.id, e.currentTarget.value || null)}
    >
      <option value="">{t("panel.field.unset", { name: f.name })}</option>
      {#each f.options.choices ?? [] as c (c)}
        <option value={c}>{c}</option>
      {/each}
    </select>
  {:else if f.kind === "multiselect"}
    <select
      multiple
      size="2"
      title={t("panel.field.multiHint")}
      onchange={(e) =>
        setField(f.id, Array.from(e.currentTarget.selectedOptions).map((o) => o.value))}
    >
      {#each f.options.choices ?? [] as c (c)}
        <option
          value={c}
          selected={Array.isArray(extraVals[f.id]) && (extraVals[f.id] as string[]).includes(c)}
        >
          {c}
        </option>
      {/each}
    </select>
  {:else if f.kind === "number"}
    <input
      class="fnum"
      type="number"
      step="any"
      placeholder={f.options.unit
        ? t("panel.field.unitPlaceholder", { name: f.name, unit: f.options.unit })
        : f.name}
      value={extraVals[f.id] === null || extraVals[f.id] === undefined ? "" : String(extraVals[f.id])}
      oninput={(e) => setField(f.id, e.currentTarget.value === "" ? null : Number(e.currentTarget.value))}
    />
  {:else if f.kind === "date"}
    <input
      type="datetime-local"
      value={toLocalInput(typeof extraVals[f.id] === "string" ? (extraVals[f.id] as string) : null)}
      onchange={(e) => setField(f.id, e.currentTarget.value ? fromLocalInput(e.currentTarget.value) : null)}
    />
  {:else}
    <input
      class="ftext"
      type={f.kind === "url" ? "url" : "text"}
      placeholder={f.name}
      value={String(extraVals[f.id] ?? "")}
      oninput={(e) => setField(f.id, e.currentTarget.value || null)}
    />
  {/if}
{/snippet}

<!-- 时间字段行：chip（未激活）/ 展开控件（激活）；模板态展开为占位池 -->
{#snippet timeRow(id: TimeFieldId)}
  {@const meta = TIME_FIELD_META[id]}
  {@const val = timeValue(id)}
  <div class="field">
    {#if activeTimes.includes(id)}
      <span class="label">{t(meta.label)}</span>
      {#if isTpl}
        <select value={val} onchange={(e) => setTimeValue(id, e.currentTarget.value)}>
          <option value="">{t("panel.tpl.noDefault")}</option>
          {#each poolFor(meta.column) as p (p.token)}
            <option value={p.token}>{p.label}</option>
          {/each}
        </select>
        {#if val}
          {@const preview = resolveTimeValue(val, resolveCtx(groupOf(id) === "time" && id === "end"))}
          <span class="dim">
            {t("panel.tpl.resolve", { time: preview ? fmtDateTime(preview) : t("panel.tpl.invalidToken") })}
          </span>
        {/if}
      {:else if id === "start" && allDay}
        <input type="date" bind:value={allDayStart} />
      {:else if id === "end" && effType === "event" && allDay}
        <span class="dim">{t("panel.time.allDayEnd")}</span>
      {:else}
        <!-- 时间选择器五行面板（§3.2 / SPRINT-SPEC §5）：点值区展开，
             快捷即点即用即关；datetime-local 不再直排暴露 -->
        <TimePopover value={val} variant={id} onchange={(v) => setTimeValue(id, v)} />
      {/if}
      {#if !isTpl && id === "start"}
        {#each START_LABELS as l (l.token)}
          <button class="chip" onclick={() => fillStart(l.token)}>{t(l.label)}</button>
        {/each}
      {/if}
      {#if !isTpl && id === "end" && effType === "event"}
        {#each END_LABELS as l (l.token)}
          <button class="chip" onclick={() => fillEnd(l.token)}>{t(l.label)}</button>
        {/each}
        {#if !val && !allDay}
          <span class="dim">{t("panel.time.emptyEnd")}</span>
        {/if}
      {/if}
      {#if !isTpl && !isEdit && effType === "task" && id === "due"}
        <button class="chip" onclick={() => setTimeValue("due", tomorrow())}>{t("panel.quick.tomorrow")}</button>
      {/if}
      {#if !isTpl && id === "occurred" && !isEdit}
        <button class="chip" onclick={() => setTimeValue("occurred", nowLocalInput())}>{t("panel.quick.now")}</button>
      {/if}
      {#if isTpl && id === "end"}
        <span class="dim">{t("panel.tpl.endHint")}</span>
      {/if}
      {#if !(isEdit && effType === "event" && (id === "start" || id === "end"))}
        <button
          class="chip"
          title={isTpl ? t("panel.tpl.removeDefault") : t("panel.time.deactivate")}
          onclick={() => deactivateGroup(groupOf(id))}>×</button
        >
      {/if}
    {:else}
      <button class="chip ghost" onclick={() => activateTime(id)}>＋ {t(meta.label)}</button>
    {/if}
  </div>
{/snippet}

{#snippet body()}
  <header>
    {#if isTpl}
      <span class="type-badge">{t("panel.badge.template")}</span>
      <span class="id">{t("panel.tpl.headerHint")}</span>
    {:else if isEdit}
      <span class="type-badge">{typeLabel(effType)}</span>
      <span class="id">{t("panel.header.editHint")}</span>
    {:else}
      <label class="type-sel">
        {t("panel.header.type")}
        <select bind:value={typeMan}>
          <option value="">{t("panel.header.autoType", { type: typeLabel(inferred) })}</option>
          {#each [["event", "type.event"], ["task", "type.task"], ["log", "type.log"]] as [val, key] (val)}
            <option value={val}>{t(key)}</option>
          {/each}
        </select>
      </label>
      <span class="id">{t("panel.header.inferHint")}</span>
    {/if}
  </header>

  {#if isTpl}
    <div class="field">
      <input
        class="tpl-name"
        placeholder={t("panel.tpl.namePlaceholder")}
        bind:value={tplName}
        use:focusOnMount
      />
      <input class="icon" placeholder="😀" bind:value={tplIcon} maxlength="4" title={t("panel.tpl.iconTitle")} />
      <label class="type-sel">
        {t("panel.header.type")}
        <select bind:value={tplItemType}>
          {#each [["event", "type.event"], ["task", "type.task"], ["log", "type.log"]] as [val, key] (val)}
            <option value={val}>{t(key)}</option>
          {/each}
        </select>
      </label>
      <input class="short" placeholder={t("panel.tpl.tagPlaceholder")} bind:value={tplTag} />
    </div>
    <div class="field">
      <input class="grow" placeholder={t("panel.tpl.notePlaceholder")} bind:value={tplNote} />
      {#if tplItemType === "log"}
        <label class="fcheck">
          <input type="checkbox" bind:checked={tplPinned} />
          {t("panel.tpl.showInLogPage")}
        </label>
      {/if}
    </div>
  {/if}

  <input
    class="title"
    placeholder={isTpl
      ? t("panel.title.tplPlaceholder")
      : isEdit
        ? displayOf(src!)
        : t("panel.title.createPlaceholder")}
    bind:value={title}
    use:focusOnMount={!isTpl}
    onkeydown={(e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void save();
      } else if (e.key === "Tab" && timeHint) {
        e.preventDefault();
        applyTimeHint();
      }
    }}
  />

  {#if timeHint}
    <button class="chip time-hint" onclick={applyTimeHint}>
      ⿻ {timeHint.label} · {t("panel.timeHint.tabApply")}
    </button>
  {/if}
  {#if isEdit && effType === "task"}
    <div class="field">
      <label class="fcheck">
        <input
          type="checkbox"
          checked={status === "done"}
          onchange={(e) => (status = e.currentTarget.checked ? "done" : "todo")}
        />
        {t("common.done")}
      </label>
    </div>
  {/if}

  {#if !isTpl && !isEdit && templates.some((x) => tplUsable(x))}
    <div class="field">
      <span class="label">{t("panel.label.template")}</span>
      {#each templates.filter((x) => tplUsable(x)) as tpl (tpl.id)}
        <button
          class="chip tpl"
          class:active={activeTpl?.id === tpl.id || presetTplId === tpl.id}
          title={tpl.note ?? t("panel.tpl.applyTitle", { name: q(tpl.name) })}
          onclick={() => applyTemplate(tpl)}
        >
          {#if tpl.icon}<span>{tpl.icon}</span>{/if}
          {tpl.name}
          {#if typeUndetermined}<span class="tpl-type">{typeLabel(tpl.item_type)}</span>{/if}
        </button>
      {/each}
    </div>
  {/if}

  {#if isTpl && effType === "task"}
    <div class="field">
      <span class="dim">{t("panel.tpl.taskHint")}</span>
    </div>
  {/if}

  <!-- 附件区（常驻）：拖文件进窗口任意位置 / Ctrl+V 粘贴截图 / 粘贴路径回车，只记路径不复制（模板态不收） -->
  {#if !isTpl}
  <div class="field drop-zone" class:drag={dragHover}>
    <span class="label">{t("panel.label.attachments")}</span>
    <FileLinkChips
      links={fileLinks}
      onremove={(p) => (fileLinks = fileLinks.filter((x) => x !== p))}
    />
    {#each pastedImages as img (img.url)}
      <span class="fwrap">
        <img
          src={img.url}
          alt={t("panel.attach.pastedAlt")}
          ondblclick={() => (previewUrl = img.url)}
        />
        <button
          class="chip"
          title={t("panel.attach.removeImage")}
          onclick={() => {
            URL.revokeObjectURL(img.url);
            pastedImages = pastedImages.filter((x) => x.url !== img.url);
          }}>×</button
        >
      </span>
    {/each}
    {#if fileLinks.length === 0 && pastedImages.length === 0}
      <span class="dim">{isEdit ? t("panel.attach.dropEdit") : t("panel.attach.dropCreate")}</span>
    {/if}
    <input
      class="link-input"
      placeholder={t("panel.attach.pathPlaceholder")}
      bind:value={linkInput}
      onkeydown={(e) => {
        if (e.key === "Enter" && linkInput.trim()) {
          addLinks([linkInput]);
          linkInput = "";
        }
      }}
    />
  </div>
  {/if}

  {#if !isTpl && !isEdit && (!showDetails || more)}
    <button class="ghost more-btn" onclick={() => setExpanded(!more)}>
      {more ? t("panel.more.collapse") : t("panel.more.expand")}
    </button>
  {/if}

  {#if showDetails}
  {#each TIME_GROUP_IDS.filter(groupVisible) as g (g)}
    {#if groupActive(g)}
      {#each TIME_GROUP_META[g].members.filter(timeVisible) as mid (mid)}
        {@render timeRow(mid)}
      {/each}
      {#if g === "time" && !isTpl}
        <div class="field">
          <span class="label">{t("panel.label.ranges")}</span>
          {#each RANGE_LABELS as r (r.label)}
            <button
              class="chip"
              class:active={!!r.allDay && allDay}
              onclick={() => fillRange(r)}
            >
              {t(r.label)}
            </button>
          {/each}
        </div>
      {/if}
    {:else}
      <div class="field">
        <button class="chip ghost" onclick={() => activateGroup(g)}>
          ＋ {t(TIME_GROUP_META[g].label)}
        </button>
      </div>
    {/if}
  {/each}

  {#if recVisible}
  <div class="field">
    <span class="label">{t("panel.label.repeat")}</span>
    {#each [["", "panel.rec.none"], ["daily", "panel.rec.daily"], ["weekly", "panel.rec.weekly"], ["monthly", "panel.rec.monthly"]] as [mode, key] (mode)}
      <button class="chip" class:active={recKind === mode} onclick={() => setRecKind(mode as "")}>
        {t(key)}
      </button>
    {/each}
    {#if recKind === "weekly"}
      {@const cur = parseRecurrence(recurrenceSpec)}
      {#each [1, 2, 3, 4, 5, 6, 7] as n (n)}
        <button
          class="chip"
          class:active={cur?.kind === "weekly" && cur.n === n}
          onclick={() => setWeeklyDay(n)}
        >
          {t("panel.rec.dows").split(",")[n - 1]}
        </button>
      {/each}
    {:else if recKind === "monthly"}
      {@const cur = parseRecurrence(recurrenceSpec)}
      <input
        type="number"
        min="1"
        max="31"
        class="month-day"
        value={cur?.kind === "monthly" ? cur.d : 1}
        onchange={(e) => setMonthlyDay(Number(e.currentTarget.value))}
      />
      <span class="dim">{t("panel.rec.monthDay")}</span>
    {:else if recKind === "daily"}
      <span class="dim">{recLabel} · {t("panel.rec.editAll")}</span>
    {/if}
  </div>
  {/if}

  <div class="field">
    <span class="label">{t("panel.label.fields")}</span>
    <select
      bind:value={pickerSel}
      onchange={(e) => {
        e.preventDefault();
        pickField();
      }}
    >
      <option value="">{t("panel.fields.addOption")}</option>
      {#each defs as f (f.id)}
        {#if !picked.includes(f.id)}
          <option value={f.id}>{f.name}</option>
        {/if}
      {/each}
    </select>
    {#each picked as id (id)}
      {@const f = defs.find((d) => d.id === id)}
      {#if f}
        <span class="fwrap">
          {@render fieldValue(f)}
          <button class="chip" onclick={() => dropField(id)}>×</button>
        </span>
      {/if}
    {/each}
  </div>

  {#if !isTpl}
  <div class="field">
    <span class="label">{t("panel.label.tags")}</span>
    <div class="tags">
      {#each tags as tag (tag)}
        <button class="chip" onclick={() => (tags = tags.filter((x) => x !== tag))}>#{tag} ×</button>
      {/each}
      <input
        class="tag-input"
        placeholder={t("panel.tags.placeholder")}
        bind:value={tagInput}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            addTag();
          }
        }}
      />
    </div>
  </div>

  <div class="field">
    <span class="label">{t("panel.label.reminders")}</span>
    {#each reminders as spec, i (i)}
      <span class="fwrap">
        {#if isAbsoluteSpec(spec) || spec === ""}
          <input
            type="datetime-local"
            class="rem"
            value={spec ? toLocalInput(spec) : ""}
            onchange={(e) =>
              setReminderSpec(i, e.currentTarget.value ? fromLocalInput(e.currentTarget.value) : "")}
          />
        {:else}
          <span class="chip rem-chip" title={spec}>{reminderLabel(spec)}</span>
        {/if}
        <button class="chip" onclick={() => removeReminder(i)}>×</button>
      </span>
    {/each}
    <select bind:value={remPick} onchange={addReminder}>
      <option value="">{t("panel.reminders.addOption")}</option>
      {#each reminderPool as p (p.token)}
        <option value={p.token}>{t(p.label)}</option>
      {/each}
    </select>
    <span class="dim">{t("panel.reminders.relativeHint")}</span>
  </div>
  {/if}

  <textarea rows="2" placeholder={isTpl ? t("panel.note.tplPlaceholder") : t("panel.note.placeholder")} bind:value={note}></textarea>

  {#if isEdit && src}
    <p class="meta">{t("panel.meta.timestamps", { created: fmtDateTime(src.created_at), updated: fmtDateTime(src.updated_at) })}</p>
  {/if}
  {/if}
  <!-- /showDetails -->


  {#if error}<p class="error">{error}</p>{/if}

  <div class="actions">
    {#if isTpl}
      <span class="inferred">
        {t("panel.actions.tplLine", {
          action: editingTpl ? t("panel.actions.editTpl") : t("panel.actions.newTpl"),
          type: typeLabel(tplItemType),
        })}
      </span>
    {:else if isEdit && src}
      {#if effType === "task"}
        <button class="ghost" onclick={() => convertType("event")}>
          {confirmConvert === "event" ? t("panel.convert.toEventConfirm") : t("panel.convert.toEvent")}
        </button>
      {:else if effType === "event"}
        <button class="ghost" onclick={() => convertType("log")}>
          {confirmConvert === "log" ? t("panel.convert.toLogConfirm") : t("panel.convert.toLog")}
        </button>
      {/if}
      <DeleteButton onconfirm={remove} />
    {:else}
      <span class="inferred">{t("panel.actions.willCreate", { type: typeLabel(effType) })}</span>
    {/if}
    <span class="hint">{t("panel.actions.shortcutHint")}</span>
    <button class="ghost" onclick={cancel}>{t("common.cancel")}</button>
    <button
      class="primary"
      disabled={saving}
      onclick={() => void save()}
    >
      {saving
        ? t("panel.actions.saving")
        : isTpl
          ? (editingTpl ? t("panel.actions.saveTpl") : t("panel.actions.saveAsTpl"))
          : isEdit
            ? t("panel.actions.saveEdit")
            : t("common.save")}
    </button>
  </div>
{/snippet}

{#if windowMode}
  <div class="quick">
    <div class="titlebar">
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="grip" data-tauri-drag-region onmousedown={startMove}></div>
      <button class="close" title={t("panel.win.closeTitle")} onclick={cancel}>×</button>
    </div>
    <div class="quick-body">
      {@render body()}
    </div>
    {#each resizeDirs as r (r.cls)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class={`rz ${r.cls}`} onmousedown={(e) => startResize(e, r.dir)}></div>
    {/each}
  </div>
{:else}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_interactive_supports_focus -->
  <div class="overlay" onclick={cancel} role="presentation">
    <div
      class="modal"
      role="dialog"
      tabindex="-1"
      aria-label={isTpl ? t("panel.aria.tplEditor") : isEdit ? t("panel.aria.editItem") : t("panel.aria.newItem")}
      onclick={(e) => e.stopPropagation()}
    >
      {@render body()}
    </div>
  </div>
{/if}

{#if previewUrl}
  <!-- 粘贴截图大图预览：点任意处 / Esc 关闭（独立窗口内即全窗口遮罩） -->
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="preview" role="presentation" onclick={() => (previewUrl = "")}>
    <img src={previewUrl} alt={t("panel.attach.previewAlt")} />
  </div>
{/if}

<style>
  /* 独立窗口：窗口透明（tauri.conf.json transparent: true）+ 本容器圆角描边成纸卡造型。
     配色用全局纸黄主题（app.css），这里不再覆盖变量。
     代价：透明窗会退回灰度抗锯齿（无子像素），换圆角是已知取舍。
     顶栏（拖动区+关闭按钮）固定不滚动，只有内容区滚动 */
  .quick {
    position: relative;
    height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 16px 14px;
    background: var(--bg);
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 14px;
  }

  .quick-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  /* 内容行不再被 flex 压扁（备注曾被挤没了），放不下时内容区滚动 */
  .quick-body > * {
    flex-shrink: 0;
  }

  .quick textarea {
    min-height: 48px;
  }

  /* 顶栏：拖动区 + 关闭按钮 */
  .titlebar {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .titlebar .grip {
    flex: 1;
    height: 14px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: grab;
  }

  .titlebar .grip::before {
    content: '';
    width: 44px;
    height: 4px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--text) 22%, transparent);
  }

  .titlebar .grip:hover::before {
    background: color-mix(in srgb, var(--text) 45%, transparent);
  }

  .titlebar .close {
    border: none;
    background: transparent;
    color: var(--text-dim);
    font-size: 16px;
    line-height: 1;
    padding: 2px 8px;
    border-radius: 6px;
  }

  .titlebar .close:hover {
    color: var(--text);
    background: color-mix(in srgb, var(--text) 12%, transparent);
    filter: none;
  }

  /* 右下 / 下 / 右缘调整手柄（贴窗口边缘） */
  .rz {
    position: absolute;
    z-index: 10;
  }

  .rz.s {
    left: 10px;
    right: 10px;
    bottom: 0;
    height: 6px;
    cursor: s-resize;
  }

  .rz.e {
    top: 10px;
    bottom: 10px;
    right: 0;
    width: 6px;
    cursor: e-resize;
  }

  .rz.se {
    right: 0;
    bottom: 0;
    width: 16px;
    height: 16px;
    cursor: se-resize;
  }

  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .modal {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 36px rgb(0 0 0 / 0.28);
    width: 520px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 48px);
    overflow: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }

  .type-sel {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-dim);
    font-size: 13px;
  }

  .type-badge {
    color: var(--accent);
    font-size: 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 999px;
    padding: 2px 10px;
  }

  .id {
    color: var(--text-dim);
    font-size: 11px;
    margin-left: auto;
  }

  .title {
    font-size: 15px;
    padding: 9px 11px;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .label {
    color: var(--text-dim);
    font-size: 13px;
    min-width: 32px;
  }

  .chip {
    border-radius: 999px;
    padding: 3px 10px;
    font-size: 12px;
  }

  .chip.ghost {
    border-style: dashed;
    color: var(--text-dim);
  }

  .chip.active {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
    border-color: var(--accent);
    font-weight: 600;
  }

  .chip.tpl.active {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
    border-color: var(--accent);
    font-weight: 600;
  }

  /* 混排时标注模板归属类型（裸面板全部模板可见） */
  .tpl-type {
    color: var(--text-dim);
    font-size: 10px;
    margin-left: 2px;
    font-weight: 400;
  }

  .more-btn {
    align-self: flex-start;
    font-size: 12px;
    padding: 3px 10px;
  }

  .dim {
    color: var(--text-dim);
    font-size: 12px;
  }

  .tpl-name {
    flex: 1;
    min-width: 140px;
    font-weight: 600;
  }

  input.icon {
    width: 52px;
    text-align: center;
  }

  input.short {
    width: 140px;
  }

  input.grow {
    flex: 1;
    min-width: 160px;
  }

  /* 附件区：拖放目标（拖文件进窗口任意位置 / Ctrl+V 粘贴截图都落在这里） */
  .drop-zone {
    border: 1.5px dashed var(--border);
    border-radius: var(--radius);
    padding: 6px 10px;
    min-height: 34px;
  }

  .drop-zone.drag {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .drop-zone img {
    height: 60px;
    border-radius: 6px;
    cursor: zoom-in;
  }

  /* 截图大图预览：盖住所在窗口（快速添加小窗内即全窗口） */
  .preview {
    position: fixed;
    inset: 0;
    z-index: 300;
    background: rgb(0 0 0 / 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: zoom-out;
  }

  .preview img {
    max-width: min(92vw, 1200px);
    max-height: 92vh;
    border-radius: 8px;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
  }

  .link-input {
    flex: 1;
    min-width: 140px;
    font-size: 12px;
    padding: 3px 8px;
    border: none;
    background: transparent;
    color: var(--text);
  }

  .link-input:focus {
    outline: none;
  }

  .fwrap {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .month-day {
    width: 64px;
  }

  .time-hint {
    margin: -4px 0 6px;
    align-self: flex-start;
    border-color: var(--accent);
    color: var(--accent);
  }

  .fcheck {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text);
    font-size: 13px;
  }

  .fnum {
    width: 130px;
  }

  .ftext {
    width: 150px;
  }

  input.rem {
    width: 190px;
    font-size: 12px;
    padding: 3px 6px;
  }

  .rem-chip {
    border-style: dashed;
    color: var(--accent);
  }

  .tags {
    display: flex;
    gap: 6px;
    align-items: center;
    flex: 1;
    min-width: 160px;
  }

  .tag-input {
    flex: 1;
    min-width: 80px;
    padding: 4px 8px;
    font-size: 12px;
  }

  textarea {
    resize: vertical;
    font: inherit;
  }

  .meta {
    margin: 0;
    color: var(--text-dim);
    font-size: 12px;
  }

  .actions {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }

  .inferred {
    margin-right: auto;
    color: var(--accent);
    font-size: 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 999px;
    padding: 2px 10px;
  }

  .hint {
    color: var(--text-dim);
    font-size: 12px;
  }

  .error {
    color: var(--danger);
    margin: 0;
    font-size: 13px;
  }
</style>

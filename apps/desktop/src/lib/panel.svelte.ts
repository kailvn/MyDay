/**
 * 统一条目面板（创建 / 编辑 / 模板同一组件）+ 只读详情面板的全局状态。
 *
 * - openCreate(...)：主窗口任意入口（日历格子带锚点日、Ctrl+N）与快速添加窗口共用；
 * - openEdit(item)：全应用只有一个编辑弹层；
 * - openDetail(item)：只读详情面板，各视图点击条目行打开（rowDetail）；
 * - openTemplate(...)：模板编辑器 = 同一面板的模板模式（填一遍存为模板），
 *   模板页「新建 / 编辑模板」与将来的其他入口共用；
 * - 面板组件按 current 非空挂载 / 卸载，每次打开都是全新初值。
 */
import type { Item, ItemType, Template } from "./api";

export interface CreateRequest {
  mode: "create";
  /** 预设类型；缺省由激活的字段推断（截止→待办、开始→日程、记录字段→记录） */
  item_type?: ItemType | null;
  title?: string | null;
  /** 锚点日（YYYY-MM-DD 本地日，如日历格子点击）：激活时间字段时的默认值来源 */
  anchorDay?: string | null;
  /** 预设开始时间（RFC3339，绝对值；一般用 anchorDay 让激活时注入） */
  presetStart?: string | null;
  /** 预设结束时间（拖选创建时段用；需与 presetStart 同给） */
  presetEnd?: string | null;
  /** 预设截止时间 */
  presetDue?: string | null;
  /** 预设模板：无默认值的模板点击时改为打开面板让用户补值，而不是静默生成空记录 */
  presetTemplateId?: string | null;
  presetDefaults?: Record<string, unknown> | null;
  presetTags?: string[] | null;
}

export interface EditRequest {
  mode: "edit";
  item: Item;
}

/** 只读详情：点击条目行打开，编辑从详情内「编辑」进入 */
export interface DetailRequest {
  mode: "detail";
  item: Item;
}

export interface TemplateRequest {
  mode: "template";
  /** null = 新建模板；否则编辑该模板（defaults 回填为面板初值） */
  template: Template | null;
}

export type PanelRequest = CreateRequest | EditRequest | DetailRequest | TemplateRequest;

let current = $state<PanelRequest | null>(null);

export function panelRequest(): PanelRequest | null {
  return current;
}

export function openCreate(req: Omit<CreateRequest, "mode"> = {}): void {
  current = { mode: "create", ...req };
}

export function openEdit(item: Item): void {
  current = { mode: "edit", item };
}

export function openDetail(item: Item): void {
  current = { mode: "detail", item };
}

export function openTemplate(template: Template | null): void {
  current = { mode: "template", template };
}

export function closePanel(): void {
  current = null;
}

/** 列表行点击 → 打开详情；行内按钮 / 复选框 / 链接上的点击不触发 */
export function rowDetail(item: Item): (e: MouseEvent) => void {
  return (e) => {
    const t = e.target as HTMLElement | null;
    if (t?.closest("button, input, a, label, select, textarea")) return;
    openDetail(item);
  };
}

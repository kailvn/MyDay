/**
 * 全局轻提示（底部浮出，数秒自动消失）。
 * 用于非阻断通知：冲突警告、批量操作结果、拖拽改期等；可带一个动作按钮
 * （如「撤销」）。与删除撤销条（UndoToast）互不影响。
 */

export interface ToastAction {
  label: string;
  run: () => void | Promise<void>;
}

let message = $state("");
let action = $state<ToastAction | null>(null);
let timer: ReturnType<typeof setTimeout> | undefined;

export const toast = {
  get message(): string {
    return message;
  },
  get action(): ToastAction | null {
    return action;
  },
  /** 显示提示，默认 4 秒后消失；连续调用覆盖前一条 */
  show(m: string, opts: { action?: ToastAction; ms?: number } = {}) {
    message = m;
    action = opts.action ?? null;
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => this.clear(), opts.ms ?? 4000);
  },
  dismiss() {
    if (timer) clearTimeout(timer);
    this.clear();
  },
  clear() {
    message = "";
    action = null;
  },
};

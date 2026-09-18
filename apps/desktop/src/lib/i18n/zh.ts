/**
 * 中文词典（源语言）。key 基准 = 本文件导出对象的全部 key；
 * 各命名空间分片在 parts/ 下维护，en.ts 与这里 key 集合类型锁死。
 */
import { part_common } from "./parts/common";
import { part_shell } from "./parts/shell";
import { part_settings } from "./parts/settings";
import { part_views } from "./parts/views";
import { part_vm } from "./parts/vm";
import { part_misc } from "./parts/misc";
import { part_help } from "./parts/help";
import { part_chrome } from "./parts/chrome";
import { part_calendar } from "./parts/calendar";
import { part_panel } from "./parts/panel";

export const zh = {
  ...part_common,
  ...part_shell,
  ...part_settings,
  ...part_views,
  ...part_vm,
  ...part_misc,
  ...part_help,
  ...part_chrome,
  ...part_calendar,
  ...part_panel,
} as const;

export type MessageKey = keyof typeof zh;

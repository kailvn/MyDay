/** 英文词典：key 集合与 zh.ts 类型锁死（缺漏 = 编译错误）。 */
import type { MessageKey } from "./zh";
import { part_common_en } from "./parts/common";
import { part_shell_en } from "./parts/shell";
import { part_settings_en } from "./parts/settings";
import { part_views_en } from "./parts/views";
import { part_vm_en } from "./parts/vm";
import { part_misc_en } from "./parts/misc";
import { part_help_en } from "./parts/help";
import { part_chrome_en } from "./parts/chrome";
import { part_calendar_en } from "./parts/calendar";
import { part_panel_en } from "./parts/panel";

export const en: Record<MessageKey, string> = {
  ...part_common_en,
  ...part_shell_en,
  ...part_settings_en,
  ...part_views_en,
  ...part_vm_en,
  ...part_misc_en,
  ...part_help_en,
  ...part_chrome_en,
  ...part_calendar_en,
  ...part_panel_en,
};

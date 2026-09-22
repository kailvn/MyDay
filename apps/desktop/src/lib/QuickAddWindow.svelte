<script lang="ts">
  /**
   * 快速添加窗口根：监听 quick-add 事件（托盘 / GNOME 快捷键 / CLI），
   * 以创建模式打开统一面板 ItemPanel；保存或取消后隐藏窗口。
   * 与主窗口弹层共用同一组件，创建 / 编辑不再维护两套逻辑。
   */
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { listen } from "@tauri-apps/api/event";
  import ItemPanel from "./ItemPanel.svelte";
  import { closePanel, openCreate, panelRequest } from "./panel.svelte";
  import { presetDueLocal, type ItemType, type QuickAddPreset } from "./api";

  const win = getCurrentWindow();

  onMount(async () => {
    // 圆角窗口：窗口本身透明（tauri.conf.json transparent: true），
    // html/body 也得让出底色，圆角外才露得出桌面
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    await listen<{
      type: ItemType | null;
      title: string | null;
      preset_due: QuickAddPreset | null;
    }>("quick-add", async (e) => {
      openCreate({
        item_type: e.payload?.type ?? null,
        title: e.payload?.title ?? null,
        // 主窗口快捷钮：按档位预填该日 23:59 截止（激活即推断待办并展开面板）
        presetDue: e.payload?.preset_due ? presetDueLocal(e.payload.preset_due) : null,
      });
      // 窗口刚 show 时 WM 可能还没把焦点交过来，JS 侧再要一次
      await win.setFocus();
    });
  });

  function onKeydown(e: KeyboardEvent) {
    // 面板未开时按 Esc 也能收起窗口（托盘唤起后反悔）
    if (e.key === "Escape" && !panelRequest()) win.hide();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#key panelRequest()}
  {#if panelRequest()}
    <ItemPanel windowMode />
  {/if}
{/key}

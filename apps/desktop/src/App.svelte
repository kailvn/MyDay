<script lang="ts">
  /**
   * 窗口路由：同一份前端按窗口 label 渲染（需求 §8 主窗口、§1.2 快速弹窗、
   * OVERLAY-SPEC §3 今日悬浮窗）。
   * - main → 主界面（侧边栏 + 视图 + 统一条目面板弹层）
   * - quick-add → 快速添加窗口（同一个 ItemPanel 组件，windowMode）
   * - overlay → 今日悬浮窗（只读今日未完成 + 勾选完成）
   * 每个窗口都初始化字段 store（data-changed 自动刷新）。
   */
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { initFieldStore } from "./lib/fields.svelte";
  import Shell from "./lib/Shell.svelte";
  import QuickAddWindow from "./lib/QuickAddWindow.svelte";
  import OverlayWindow from "./lib/OverlayWindow.svelte";

  let label = "main";
  onMount(() => {
    label = getCurrentWindow().label;
    void initFieldStore();
  });
</script>

{#if label === "quick-add"}
  <QuickAddWindow />
{:else if label === "overlay"}
  <OverlayWindow />
{:else}
  <Shell />
{/if}

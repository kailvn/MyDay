<script lang="ts">
  /**
   * 闹钟弹窗（窗口 label = "alarm"，由 Rust 侧提醒环按需创建）。
   * 提醒「闹钟」档的桌面呈现：置顶弹窗 + WebAudio 循环提示音（关窗即停），
   * 动作回流 alarm_dismiss（完成 / 稍后 / 打开 / 忽略）。
   * 列表以 ready 时拉取（alarm_pending）+ 事件推送（alarm-ringing 全量）合并。
   */
  import { onMount, onDestroy } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { t } from "./i18n";
  import { api, type AlarmEntry } from "./api";

  let list = $state<AlarmEntry[]>([]);
  let working = $state<string | null>(null);

  // ---- 循环提示音（WebAudio 合成，不依赖音频资源；用户手势前可能被挂起，
  //      点任意按钮时 resume） ------------------------------------------------
  let audioCtx: AudioContext | null = null;
  let beepTimer: ReturnType<typeof setInterval> | null = null;

  function beepOnce(ctx: AudioContext) {
    const t0 = ctx.currentTime;
    // 双短一长的经典闹铃节奏
    for (const [off, freq] of [
      [0, 880],
      [0.2, 880],
      [0.4, 660],
    ] as const) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "square";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, t0 + off);
      gain.gain.exponentialRampToValueAtTime(0.12, t0 + off + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + off + 0.16);
      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(t0 + off);
      osc.stop(t0 + off + 0.18);
    }
  }

  function startBeep() {
    if (beepTimer) return;
    try {
      audioCtx ??= new AudioContext();
      if (audioCtx.state === "suspended") void audioCtx.resume();
      beepOnce(audioCtx);
      beepTimer = setInterval(() => {
        if (audioCtx && audioCtx.state === "running") beepOnce(audioCtx);
      }, 2200);
    } catch (e) {
      console.error("myday: 闹钟提示音启动失败", e);
    }
  }

  function stopBeep() {
    if (beepTimer) {
      clearInterval(beepTimer);
      beepTimer = null;
    }
  }

  $effect(() => {
    if (list.length > 0) startBeep();
  });

  let unlisten: (() => void) | null = null;
  onDestroy(() => {
    unlisten?.();
    stopBeep();
  });

  async function act(key: string, action: "complete" | "snooze" | "open" | "dismiss") {
    try {
      audioCtx?.resume().catch(() => {}); // 首次手势解锁音频
      working = key;
      await api.alarmDismiss(key, action);
      list = list.filter((e) => e.key !== key);
    } catch (e) {
      console.error("myday: 闹钟动作失败", e);
    } finally {
      working = null;
    }
  }

  onMount(async () => {
    try {
      list = await api.alarmPending();
    } catch (e) {
      console.error("myday: 闹钟列表拉取失败", e);
    }
    unlisten = await listen<AlarmEntry[]>("alarm-ringing", (e) => {
      list = e.payload ?? [];
    });
  });
</script>

<div class="alarm-root" role="alertdialog" aria-label={t("alarm.title")}>
  <div class="alarm-head">⏰ {t("alarm.title")}</div>
  {#each list as entry (entry.key)}
    <div class="alarm-item">
      <div class="alarm-text">
        <div class="alarm-title">{entry.title}</div>
        <div class="alarm-body">{entry.body}</div>
      </div>
      <div class="alarm-actions">
        <button class="btn primary" disabled={working === entry.key} onclick={() => act(entry.key, "complete")}>
          {t("alarm.complete")}
        </button>
        <button class="btn" disabled={working === entry.key} onclick={() => act(entry.key, "snooze")}>
          {t("alarm.snooze")}
        </button>
        <button class="btn" disabled={working === entry.key} onclick={() => act(entry.key, "open")}>
          {t("alarm.open")}
        </button>
        <button class="btn ghost" disabled={working === entry.key} onclick={() => act(entry.key, "dismiss")}>
          {t("alarm.dismiss")}
        </button>
      </div>
    </div>
  {:else}
    <div class="alarm-empty">{t("alarm.empty")}</div>
  {/each}
</div>

<style>
  .alarm-root {
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
    background: var(--panel-bg, #1c1c24);
    color: var(--fg, #eee);
    border: 1px solid var(--border, #3a3a4a);
    border-radius: 12px;
    overflow: auto;
  }
  .alarm-head {
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.05em;
    opacity: 0.9;
  }
  .alarm-item {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--border, #3a3a4a);
    border-radius: 10px;
    background: var(--card-bg, #26262f);
  }
  .alarm-title {
    font-size: 16px;
    font-weight: 600;
  }
  .alarm-body {
    margin-top: 2px;
    font-size: 13px;
    opacity: 0.75;
    white-space: pre-line;
  }
  .alarm-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
  }
  .btn {
    border: 1px solid var(--border, #3a3a4a);
    border-radius: 8px;
    padding: 6px 12px;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font-size: 13px;
  }
  .btn:hover {
    background: var(--hover, #33333f);
  }
  .btn.primary {
    background: var(--accent, #4f7cff);
    border-color: transparent;
    color: #fff;
  }
  .btn.ghost {
    opacity: 0.7;
  }
  .btn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .alarm-empty {
    opacity: 0.6;
    font-size: 13px;
    text-align: center;
    padding: 12px 0;
  }
</style>

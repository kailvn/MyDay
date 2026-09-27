<script lang="ts">
  /**
   * 手机端同步弹层：扫码配对（扫桌面端二维码一步填好并同步）+ 手动地址/配对码。
   * 协议与合并全在 Rust(core::sync + sync_client 命令),此层只收集输入与展示。
   */
  import { onMount } from "svelte";
  import { t } from "../lib/i18n";
  import { api, type SyncRunReport } from "../lib/api";
  import { toast } from "../lib/toast.svelte";
  import { scan, checkPermissions, requestPermissions } from "@tauri-apps/plugin-barcode-scanner";

  let { onclose }: { onclose: () => void } = $props();

  let server = $state("");
  let token = $state("");
  let last = $state<string | null>(null);
  let running = $state(false);
  let scanning = $state(false);
  let report = $state<SyncRunReport | null>(null);

  onMount(async () => {
    server = (await api.getSetting("sync.server").catch(() => null)) ?? "";
    token = (await api.getSetting("sync.client_token").catch(() => null)) ?? "";
    last = await api.getSetting("sync.last_sync_at").catch(() => null);
  });

  async function run() {
    if (running) return;
    if (!token.trim()) {
      toast.show(t("mobile.sync_need_token"));
      return;
    }
    running = true;
    report = null;
    try {
      report = await api.syncRun(server || null, token.trim());
      void api.setSetting("sync.server", server.trim());
      void api.setSetting("sync.client_token", token.trim());
      last = report.last_sync;
    } catch (e) {
      toast.show(String(e));
    } finally {
      running = false;
    }
  }

  /** 扫桌面端二维码（myday-sync://ip:port/token）：填入并立即同步 */
  async function scanFill() {
    if (scanning || running) return;
    scanning = true;
    try {
      const perm = await checkPermissions();
      if (perm !== "granted" && (await requestPermissions()) !== "granted") {
        toast.show(t("mobile.sync_scan_denied"));
        return;
      }
      const res = await scan();
      const m = /^myday-sync:\/\/([^/\s]+)\/([A-Za-z0-9-]+)\s*$/.exec(res.content);
      if (!m) {
        toast.show(t("mobile.sync_scan_bad"));
        return;
      }
      server = m[1];
      token = m[2];
      void api.setSetting("sync.server", server);
      void api.setSetting("sync.client_token", token);
      await run();
    } catch (e) {
      toast.show(String(e));
    } finally {
      scanning = false;
    }
  }

  const lastText = $derived(
    last
      ? new Date(last).toLocaleString([], {
          month: "numeric",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        })
      : t("mobile.sync_none"),
  );
</script>

<div class="m-backdrop" onclick={onclose} aria-hidden="true"></div>
<div class="m-sheet" role="dialog" aria-label={t("mobile.sync_title")}>
  <div class="m-grip"></div>
  <div class="m-sheet-head">
    <h2 class="m-sheet-title">{t("mobile.sync_title")}</h2>
    <button class="m-iconbtn" onclick={onclose}>✕</button>
  </div>
  <div class="m-sheet-body">
    <p class="m-hint">{t("mobile.sync_need_token")}</p>
    <button class="m-scan" disabled={scanning || running} onclick={scanFill}>
      {scanning ? t("mobile.sync_scanning") : t("mobile.sync_scan")}
    </button>
    <div class="m-formrow">
      <span class="lbl">{t("mobile.sync_server")}</span>
      <input type="text" placeholder={t("mobile.sync_server_ph")} bind:value={server} spellcheck="false" />
    </div>
    <div class="m-formrow">
      <span class="lbl">{t("mobile.sync_token")}</span>
      <input type="text" bind:value={token} spellcheck="false" />
    </div>
    <button class="m-primary" disabled={running} onclick={run}>
      {running ? t("mobile.sync_running") : t("mobile.sync_run")}
    </button>

    {#if report}
      <p class="result">
        {t("mobile.sync_result", {
          pulled: report.pulled,
          conflicts_in: report.conflicts_in,
          pushed: report.pushed,
          conflicts_server: report.conflicts_server,
        })}
      </p>
    {/if}
    <p class="m-hint">{t("mobile.sync_last")}：{lastText}</p>
  </div>
</div>

<style>
  .m-formrow input {
    font-size: 14px;
  }
  .m-scan {
    appearance: none;
    border: 1px solid var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--accent);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    border-radius: var(--radius);
    padding: 10px;
    width: 100%;
    margin: 8px 0 4px;
  }
  .m-primary {
    margin-top: 10px;
  }
  .result {
    margin: 12px 0 6px;
    font-size: 13px;
    color: var(--text);
  }
</style>

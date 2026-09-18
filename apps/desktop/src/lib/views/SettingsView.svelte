<script lang="ts">
  /** 需求 §12：设置（数据目录 / IPC / 默认提醒 / 全局快捷键 / 今日悬浮窗）。
   *  模板与字段管理在「模板」页。 */
  import { api, type OverlayConfig } from "../api";
  import { toast } from "../toast.svelte";
  import { holidaysMeta, importHolidaysJson, resetHolidays } from "../holidays.svelte";
  import { t, i18n, setLocale, type Locale } from "../i18n";

  let info = $state<{ version: string; data_root: string; socket_path: string; backend?: string } | null>(null);
  let reminderMinutes = $state("10");
  let catchupMinutes = $state("120");
  let saved = $state("");
  let exporting = $state(false);
  let backing = $state(false);

  // 通用：开机自启（Linux XDG autostart / Windows Run 键）
  let autostart = $state(false);

  // 今日悬浮窗（OVERLAY-SPEC §6/§9.6）
  let overlayCfg = $state<OverlayConfig | null>(null);

  // 节假日（SPRINT2-SPEC §4：JSON 导入，不硬编码）
  let holidays = $state(holidaysMeta());
  let holidayInput = $state("");
  let holidayError = $state("");
  let importingHolidays = $state(false);

  function refreshHolidayMeta() {
    holidays = holidaysMeta();
  }

  async function load() {
    info = await api.appInfo();
    reminderMinutes = (await api.getSetting("default_reminder_minutes")) ?? "10";
    catchupMinutes = (await api.getSetting("reminder_catchup_minutes")) ?? "120";
    overlayCfg = await api.getOverlayConfig().catch(() => null);
    autostart = await api.getAutostart().catch(() => false);
    refreshHolidayMeta();
  }

  load();

  /** 切换界面语言：前端词典即时生效；Rust 侧同步托盘 / 副窗口标题 */
  async function changeLang(l: Locale) {
    await setLocale(l);
    await api.setUiLang(l).catch(() => {});
  }

  async function toggleAutostart(checked: boolean) {
    try {
      await api.setAutostart(checked);
      autostart = checked;
    } catch (e) {
      toast.show(t("settings.autostartFailed", { e: String(e) }));
      autostart = await api.getAutostart().catch(() => false);
    }
  }

  /** 显示开关 = 写配置 + 立即显示 / 隐藏（托盘勾选由 Rust 侧同步） */
  async function toggleOverlayEnabled(checked: boolean) {
    if (!overlayCfg) return;
    overlayCfg = { ...overlayCfg, enabled: checked };
    await api.setOverlayConfig(overlayCfg);
    await api.overlaySetVisible(checked);
  }

  /** 透明度 / 角落 / 锁定：写配置，锁定与吸附位置由 Rust 侧落到真实窗口 */
  async function saveOverlay() {
    if (!overlayCfg) return;
    overlayCfg = { ...overlayCfg };
    await api.setOverlayConfig(overlayCfg);
  }

  async function saveReminder() {
    try {
      // number 输入的 bind 值可能是 number,后端要 string,必须显式转换
      await api.setSetting("default_reminder_minutes", String(reminderMinutes));
      // 补发窗口（SPRINT-SPEC §1.1）：0 = 错过的全部只进摘要
      const clamped = Math.min(1440, Math.max(0, Number(catchupMinutes) || 0));
      await api.setSetting("reminder_catchup_minutes", String(clamped));
      saved = t("settings.savedOk");
      setTimeout(() => (saved = ""), 1800);
    } catch (e) {
      saved = t("settings.saveFailed", { e: String(e) });
    }
  }

  /** 导出 ICS（SPRINT2-SPEC §6）：写入数据目录 exports/ 并在文件管理器定位 */
  async function exportIcs() {
    exporting = true;
    try {
      const path = await api.exportIcs();
      toast.show(t("settings.icsExported", { path }), {
        ms: 8000,
        action: { label: t("settings.view"), run: () => void api.revealFilePath(path) },
      });
    } catch (e) {
      toast.show(t("settings.exportFailed", { e: String(e) }));
    } finally {
      exporting = false;
    }
  }

  /** 一键备份 zip：db + 附件，保留最近 7 份（SPRINT2-SPEC §6） */
  async function backup() {
    backing = true;
    try {
      const path = await api.backupZip();
      toast.show(t("settings.backupDone", { path }), {
        ms: 8000,
        action: { label: t("settings.view"), run: () => void api.revealFilePath(path) },
      });
    } catch (e) {
      toast.show(t("settings.backupFailed", { e: String(e) }));
    } finally {
      backing = false;
    }
  }

  /** 导入节假日 JSON：校验通过才写数据目录并即时生效（日历/今天页角标随之刷新） */
  async function importHolidays() {
    holidayError = "";
    importingHolidays = true;
    try {
      const err = await importHolidaysJson(holidayInput);
      if (err) {
        holidayError = err;
      } else {
        holidayInput = "";
        refreshHolidayMeta();
        toast.show(t("settings.holidaysImported"));
      }
    } catch (e) {
      holidayError = String(e);
    } finally {
      importingHolidays = false;
    }
  }

  async function resetHolidaysToBuiltin() {
    try {
      await resetHolidays();
      refreshHolidayMeta();
      toast.show(t("settings.holidaysReset"));
    } catch (e) {
      toast.show(t("settings.resetFailed", { e: String(e) }));
    }
  }
</script>

<h1>{t("settings.title")}</h1>

<section>
  <h2>{t("settings.general")}</h2>
  <label>
    {t("settings.language")}
    <select
      value={i18n.locale}
      onchange={(e) => void changeLang(e.currentTarget.value as Locale)}
    >
      <option value="zh">中文</option>
      <option value="en">English</option>
    </select>
  </label>
  <label class="col-check">
    <input
      type="checkbox"
      data-testid="autostart"
      checked={autostart}
      onchange={(e) => void toggleAutostart(e.currentTarget.checked)}
    />
    {t("settings.autostart")}
  </label>
</section>

{#if info}
  <section>
    <h2>{t("settings.dataIpc")}</h2>
    <dl>
      <dt>{t("settings.version")}</dt>
      <dd>v{info.version}</dd>
      <dt>{t("settings.dataDir")}</dt>
      <dd><code>{info.data_root}</code></dd>
      <dt>IPC socket</dt>
      <dd><code>{info.socket_path}</code></dd>
    </dl>
    <div class="actions">
      <button class="primary" disabled={exporting} onclick={exportIcs}>
        {exporting ? t("settings.exporting") : t("settings.exportIcs")}
      </button>
      <button disabled={backing} onclick={backup}>{backing ? t("settings.backingUp") : t("settings.backupZip")}</button>
      <button onclick={() => void api.openLogDir().catch((e) => toast.show(String(e)))}>
        {t("settings.openLogDir")}
      </button>
    </div>
    <p class="hint">
      {t("settings.dataHint")}
    </p>
  </section>
{/if}

<section>
  <h2>{t("settings.holidays")}</h2>
  <p class="hint">
    {t("settings.source")}
    {#if holidays.source === "user"}<b>{t("settings.sourceUser")}</b>{:else if holidays.source === "builtin"}{t("settings.sourceBuiltin")}{:else}{t("settings.sourceNone")}{/if}
    {#if holidays.years.length}
      {t("settings.years", { years: holidays.years.join(", ") })}
    {/if}
    {#if holidays.error}
      <span class="import-error">{holidays.error}</span>
    {/if}
  </p>
  {#if holidays.sources.length}
    <p class="hint">{t("settings.basis", { sources: holidays.sources.join("; ") })}</p>
  {/if}
  <label class="col">
    {t("settings.importLabel")}
    <textarea
      rows="6"
      bind:value={holidayInput}
      placeholder={'{\n  "version": 1,\n  "sources": ["国务院办公厅关于20XX年部分节假日安排的通知"],\n  "holidays": [{ "name": "元旦", "from": "20XX-01-01", "to": "20XX-01-03" }],\n  "workdays": [{ "date": "20XX-01-0X", "name": "元旦" }]\n}'}
    ></textarea>
  </label>
  {#if holidayError}
    <p class="import-error">{t("settings.importFailed", { e: holidayError })}</p>
  {/if}
  <div class="actions">
    <button
      class="primary"
      disabled={importingHolidays || !holidayInput.trim()}
      onclick={importHolidays}
    >
      {importingHolidays ? t("settings.importing") : t("settings.importApply")}
    </button>
    {#if holidays.source === "user"}
      <button onclick={resetHolidaysToBuiltin}>{t("settings.restoreBuiltin")}</button>
    {/if}
  </div>
  <p class="hint">
    {t("settings.holidayHintPre")} <code>holidays.json</code> {t("settings.holidayHintPost")}
  </p>
</section>

<section>
  <h2>{t("settings.reminders")}</h2>
  <label>
    {t("settings.reminderMinutes")}
    <input type="number" min="0" max="1440" bind:value={reminderMinutes} />
  </label>
  <label>
    {t("settings.catchupMinutes")}
    <input type="number" min="0" max="1440" bind:value={catchupMinutes} />
  </label>
  <button class="primary" onclick={saveReminder}>{t("common.save")}</button>
  {#if saved}<span class="saved">{saved}</span>{/if}
    <p class="hint">
      {t("settings.reminderHint")}<br />
      {t("settings.catchupHint")}
    </p>
</section>

<section>
  <h2>{t("settings.overlay")}</h2>
  {#if info?.backend === "wayland"}
    <p class="hint">
      {t("settings.waylandHintPre")} <code>MYDAY_BACKEND=x11</code> {t("settings.waylandHintPost")}
    </p>
  {/if}
  {#if overlayCfg}
    <label class="col-check">
      <input
        type="checkbox"
        data-testid="overlay-enabled"
        checked={overlayCfg.enabled}
        onchange={(e) => toggleOverlayEnabled(e.currentTarget.checked)}
      />
      {t("settings.overlayEnabled")}
    </label>
    <label>
      {t("settings.opacity")}
      <input
        type="range"
        data-testid="overlay-opacity"
        min="0.3"
        max="1"
        step="0.05"
        bind:value={overlayCfg.opacity}
        onchange={saveOverlay}
      />
      <span class="opacity-num">{Math.round(overlayCfg.opacity * 100)}%</span>
    </label>
    <label>
      {t("settings.corner")}
      <select bind:value={overlayCfg.corner} onchange={saveOverlay} data-testid="overlay-corner">
        <option value="tr">{t("settings.cornerTr")}</option>
        <option value="tl">{t("settings.cornerTl")}</option>
      </select>
    </label>
    <label class="col-check">
      <input
        type="checkbox"
        data-testid="overlay-locked"
        checked={overlayCfg.locked}
        onchange={(e) => {
          overlayCfg = { ...overlayCfg, locked: e.currentTarget.checked };
          void saveOverlay();
        }}
      />
      {t("settings.overlayLocked")}
    </label>
    <p class="hint">
      {t("settings.overlayHint")}
    </p>
  {/if}
</section>

<section>
  <h2>{t("settings.shortcuts")}</h2>
  <p class="hint">
    {t("settings.shortcutHint")}
  </p>
  <dl>
    <dt>{t("settings.cmdQuickAdd")}</dt><dd><code>myday quick-add</code></dd>
    <dt>{t("settings.cmdEvent")}</dt><dd><code>myday quick-add --type event</code></dd>
    <dt>{t("settings.cmdTask")}</dt><dd><code>myday quick-add --type task</code></dd>
    <dt>{t("settings.cmdLog")}</dt><dd><code>myday quick-add --type log</code></dd>
  </dl>
</section>

<style>
  h1 {
    margin: 0 0 16px;
  }

  section {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 18px;
    margin-bottom: 14px;
    max-width: 640px;
  }

  h2 {
    font-size: 14px;
    margin: 0 0 10px;
  }

  dl {
    display: grid;
    grid-template-columns: 130px 1fr;
    gap: 6px 12px;
    margin: 0;
  }

  dt {
    color: var(--text-dim);
  }

  dd {
    margin: 0;
  }

  code {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 12.5px;
  }

  label {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 12px;
  }

  .saved {
    color: var(--accent);
    font-size: 13px;
    margin-left: 8px;
  }

  .hint {
    color: var(--text-dim);
    font-size: 13px;
  }

  .actions {
    display: flex;
    gap: 10px;
    margin: 12px 0 8px;
  }

  .col {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 10px;
    font-size: 13.5px;
  }

  .col-check {
    justify-content: flex-start;
  }

  .opacity-num {
    font-variant-numeric: tabular-nums;
    min-width: 38px;
    text-align: right;
  }

  textarea {
    font-family: ui-monospace, monospace;
    font-size: 12.5px;
    resize: vertical;
  }

  .import-error {
    color: var(--danger);
    font-size: 12.5px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>

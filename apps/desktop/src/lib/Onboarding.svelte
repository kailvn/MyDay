<script lang="ts">
  /**
   * 首次启动引导（§6）：settings 无 onboarded 标记时弹出「选择启用模板」。
   * 勾选的模板置 pinned = 1；跳过则维持种子默认（三个内置模板已 pinned）。
   * 禁用模板 = pinned 0 + UI 隐藏，不删模板行、不删字段。
   */
  import { onMount } from "svelte";
  import { api, type Template } from "./api";
  import { t } from "./i18n";

  let visible = $state(false);
  let templates = $state<Template[]>([]);
  let selected = $state<Record<string, boolean>>({});
  let busy = $state(false);

  onMount(async () => {
    try {
      const marker = await api.getSetting("onboarded");
      if (marker) return;
      templates = await api.listTemplates();
      selected = Object.fromEntries(templates.map((t) => [t.id, t.pinned]));
      visible = true;
    } catch (e) {
      console.error("引导加载失败", e);
    }
  });

  async function finish(checked: Record<string, boolean>) {
    busy = true;
    try {
      for (const t of templates) {
        if (t.pinned !== checked[t.id]) {
          await api.setTemplatePinned(t.id, checked[t.id]);
        }
      }
      await api.setSetting("onboarded", "1");
      visible = false;
    } catch (e) {
      console.error("保存引导选择失败", e);
    } finally {
      busy = false;
    }
  }
</script>

{#if visible}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_interactive_supports_focus -->
  <div class="overlay" role="presentation">
    <div
      class="modal"
      role="dialog"
      tabindex="-1"
      aria-label={t("onboarding.dialogLabel")}
      onclick={(e) => e.stopPropagation()}
    >
      <h2>{t("onboarding.title")}</h2>
      <p class="hint">
        {t("onboarding.hint")}
      </p>
      <div class="list">
        {#each templates as t (t.id)}
          <label class="row">
            <input
              type="checkbox"
              checked={selected[t.id]}
              onchange={(e) => (selected = { ...selected, [t.id]: e.currentTarget.checked })}
            />
            {#if t.icon}<span>{t.icon}</span>{/if}
            <span class="name">{t.name}</span>
            <span class="note">{t.note ?? ""}</span>
          </label>
        {/each}
      </div>
      <div class="actions">
        <button class="ghost" disabled={busy} onclick={() => finish(selected)}>
          {t("onboarding.skip")}
        </button>
        <button class="primary" disabled={busy} onclick={() => finish(selected)}>
          {busy ? t("onboarding.saving") : t("onboarding.start")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 300;
  }

  .modal {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 36px rgb(0 0 0 / 0.28);
    width: 440px;
    max-width: calc(100vw - 32px);
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h2 {
    margin: 0;
    font-size: 17px;
  }

  .hint {
    margin: 0;
    color: var(--text-dim);
    font-size: 13px;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 8px;
    border-radius: 8px;
    font-size: 14px;
  }

  .row:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }

  .name {
    font-weight: 600;
  }

  .note {
    color: var(--text-dim);
    font-size: 12px;
    margin-left: auto;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 4px;
  }
</style>

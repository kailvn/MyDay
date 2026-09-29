<script lang="ts">
  /**
   * 重复条目拖拽落点二选一（Google Calendar 式）：拖的是系列的一次发生，
   * 落库前问作用域——「只改这一期」= 拆为独立条目后应用落点（原系列记单次
   * 例外，其余期不动）；「整个系列」= 既有行为（规则智能改写）。
   * Esc / 点遮罩 = 取消本次拖拽，不写库。
   */
  import { t } from "./i18n";

  let {
    title,
    onpick,
    oncancel,
  }: {
    title: string;
    onpick: (scope: "once" | "series") => void;
    oncancel: () => void;
  } = $props();

  function onkey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      oncancel();
    }
  }
</script>

<svelte:window onkeydown={onkey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" role="presentation" onclick={oncancel}>
  <div
    class="choice"
    role="dialog"
    tabindex="-1"
    aria-label={title}
    onclick={(e) => e.stopPropagation()}
  >
    <p class="q">{title}</p>
    <div class="btns">
      <button class="once" onclick={() => onpick("once")}>{t("reschedule.once")}</button>
      <button class="series" onclick={() => onpick("series")}>{t("reschedule.series_btn")}</button>
    </div>
    <p class="hint">{t("reschedule.once_hint")}</p>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: grid;
    place-items: center;
    z-index: 90;
  }
  .choice {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 36px rgb(0 0 0 / 0.28);
    padding: 16px 18px;
    width: 320px;
    max-width: calc(100vw - 32px);
  }
  .q {
    margin: 0 0 12px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }
  .btns {
    display: flex;
    gap: 8px;
  }
  button {
    appearance: none;
    font: inherit;
    font-size: 13px;
    border-radius: 8px;
    padding: 8px 12px;
    cursor: pointer;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
  }
  .once {
    flex: 1;
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
  }
  .series {
    flex: 1;
  }
  .hint {
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--text-dim);
  }
</style>

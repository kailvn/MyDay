<script lang="ts">
  /**
   * 两步确认删除按钮：第一次点击变为「确认？」，2.5 秒内再点才触发；
   * 不弹对话框，符合「快、少点击」原则，又防误触。
   */
  let { onconfirm }: { onconfirm: () => void } = $props();

  let armed = $state(false);
  let timer: number | undefined;

  function click() {
    if (armed) {
      window.clearTimeout(timer);
      armed = false;
      onconfirm();
    } else {
      armed = true;
      timer = window.setTimeout(() => (armed = false), 2500);
    }
  }
</script>

<button class="del" class:armed onclick={click} title="删除">
  {armed ? "确认？" : "✕"}
</button>

<style>
  .del {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 13px;
    min-width: 28px;
  }

  .del:hover {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 12%, transparent);
    filter: none;
  }

  .del.armed {
    color: var(--danger);
    border: 1px solid var(--danger);
    background: color-mix(in srgb, var(--danger) 10%, transparent);
    font-size: 12px;
  }
</style>

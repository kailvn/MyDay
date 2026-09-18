<script lang="ts">
  /** 行内编辑按钮，打开条目编辑弹窗。 */
  let { onedit }: { onedit: () => void } = $props();
</script>

<button class="edit" onclick={onedit} title="编辑">✎</button>

<style>
  .edit {
    border: none;
    background: transparent;
    color: var(--text-dim);
    padding: 2px 8px;
    border-radius: 6px;
    font-size: 13px;
  }

  .edit:hover {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    filter: none;
  }
</style>

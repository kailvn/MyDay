<script lang="ts">
  /**
   * 帮助页：集中收纳「用户猜不到的机制说明」与快捷键表。
   * 界面内不再放长篇提示（1.0 提示瘦身），这里是唯一详细出处。
   * 全部文案走 help.* 词典（中英）。
   */
  import { t } from "../i18n";

  interface Section {
    title: string;
    rows: [string, string][];
  }

  const sections = $derived<Section[]>([
    {
      title: t("help.concepts.title"),
      rows: [
        [t("help.concepts.types.k"), t("help.concepts.types.v")],
        [t("help.concepts.immutable.k"), t("help.concepts.immutable.v")],
        [t("help.concepts.fields.k"), t("help.concepts.fields.v")],
      ],
    },
    {
      title: t("help.capture.title"),
      rows: [
        [t("help.capture.quick.k"), t("help.capture.quick.v")],
        [t("help.capture.nl.k"), t("help.capture.nl.v")],
        [t("help.capture.paste.k"), t("help.capture.paste.v")],
        [t("help.capture.filelink.k"), t("help.capture.filelink.v")],
      ],
    },
    {
      title: t("help.calendar.title"),
      rows: [
        [t("help.calendar.drag.k"), t("help.calendar.drag.v")],
        [t("help.calendar.recur.k"), t("help.calendar.recur.v")],
        [t("help.calendar.neighbor.k"), t("help.calendar.neighbor.v")],
        [t("help.calendar.holidays.k"), t("help.calendar.holidays.v")],
        [t("help.calendar.conflict.k"), t("help.calendar.conflict.v")],
      ],
    },
    {
      title: t("help.tasks.title"),
      rows: [
        [t("help.tasks.done.k"), t("help.tasks.done.v")],
        [t("help.tasks.recur.k"), t("help.tasks.recur.v")],
        [t("help.tasks.batch.k"), t("help.tasks.batch.v")],
      ],
    },
    {
      title: t("help.reminders.title"),
      rows: [
        [t("help.reminders.intent.k"), t("help.reminders.intent.v")],
        [t("help.reminders.catchup.k"), t("help.reminders.catchup.v")],
        [t("help.reminders.center.k"), t("help.reminders.center.v")],
      ],
    },
    {
      title: t("help.stats.title"),
      rows: [
        [t("help.stats.widgets.k"), t("help.stats.widgets.v")],
        [t("help.stats.reset.k"), t("help.stats.reset.v")],
      ],
    },
    {
      title: t("help.overlay.title"),
      rows: [
        [t("help.overlay.lock.k"), t("help.overlay.lock.v")],
        [t("help.overlay.readonly.k"), t("help.overlay.readonly.v")],
      ],
    },
    {
      title: t("help.data.title"),
      rows: [
        [t("help.data.dir.k"), t("help.data.dir.v")],
        [t("help.data.backup.k"), t("help.data.backup.v")],
        [t("help.data.migrate.k"), t("help.data.migrate.v")],
        [t("help.data.cli.k"), t("help.data.cli.v")],
      ],
    },
  ]);

  const shortcuts = $derived<[string, string][]>([
    ["Ctrl+N", t("help.keys.quickadd")],
    ["Ctrl+K", t("help.keys.palette")],
    ["T / D / W / M", t("help.keys.viewswitch")],
    ["← / →", t("help.keys.navigate")],
    ["Enter / Esc", t("help.keys.enteresc")],
    ["Ctrl+V", t("help.keys.paste")],
    ["Tab", t("help.keys.tab")],
  ]);
</script>

<div class="help">
  <h2>{t("help.title")}</h2>
  {#each sections as s (s.title)}
    <section>
      <h3>{s.title}</h3>
      <dl>
        {#each s.rows as [k, v] (k)}
          <dt>{k}</dt>
          <dd>{v}</dd>
        {/each}
      </dl>
    </section>
  {/each}

  <section>
    <h3>{t("help.keys.title")}</h3>
    <dl class="keys">
      {#each shortcuts as [k, v] (k)}
        <dt><kbd>{k}</kbd></dt>
        <dd>{v}</dd>
      {/each}
    </dl>
  </section>
</div>

<style>
  .help {
    max-width: 760px;
    margin: 0 auto;
  }

  h2 {
    font-size: 18px;
    margin: 4px 0 14px;
  }

  section {
    margin-bottom: 20px;
  }

  h3 {
    font-size: 14px;
    color: var(--text-dim);
    border-bottom: 1px solid var(--border);
    padding-bottom: 6px;
    margin-bottom: 8px;
  }

  dl {
    display: grid;
    grid-template-columns: 150px 1fr;
    gap: 6px 14px;
    font-size: 13.5px;
  }

  dt {
    font-weight: 600;
  }

  dd {
    color: var(--text);
    line-height: 1.55;
  }

  kbd {
    background: color-mix(in srgb, var(--text) 10%, transparent);
    border-radius: 4px;
    padding: 1px 7px;
    font-size: 12px;
  }
</style>

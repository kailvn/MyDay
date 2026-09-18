<script lang="ts">
  /**
   * 模板页：模板管理（新建 / 编辑 / 排序 / 删除）+ 字段管理。
   * 编辑与新建走统一面板的模板模式（openTemplate）；此处只做列表级操作。
   */
  import { api, typeLabel, type FieldDef, type FieldKind, type ItemType, type Template } from "../api";
  import { tokenLabel } from "../tpltime";
  import { openTemplate } from "../panel.svelte";
  import { allDeletedFieldDefs, allFieldDefs, fieldDefMap, reloadFieldDefs } from "../fields.svelte";
  import { toast } from "../toast.svelte";
  import { t } from "../i18n";

  let { dataVersion = 0 } = $props();

  // ---- 模板管理：新建 / 编辑走统一面板的模板模式（openTemplate） -----------
  let templates = $state<Template[]>([]);
  let movingId = $state<string | null>(null);

  async function loadTemplates() {
    try {
      templates = await api.listTemplates();
    } catch (e) {
      console.error("加载模板失败", e);
    }
  }

  async function removeTpl(tpl: Template) {
    if (movingId !== tpl.id) {
      movingId = tpl.id;
      setTimeout(() => {
        if (movingId === tpl.id) movingId = null;
      }, 2500);
      return;
    }
    await api.deleteTemplate(tpl.id);
    movingId = null;
    await loadTemplates();
  }

  /** 模板 defaults 展示：字段 id 换字段名，@token 换占位标签（列键原样） */
  function defaultsText(tpl: Template): string {
    const byId = fieldDefMap();
    const entries = Object.entries(tpl.defaults ?? {});
    if (entries.length === 0) return t("templates.defaults_empty");
    return entries
      .map(([k, v]) => {
        const label = byId.get(k)?.name ?? k;
        return typeof v === "string" && v.startsWith("@")
          ? ` ${label}=${tokenLabel(v)}`
          : ` ${label}=${JSON.stringify(v)}`;
      })
      .join(" ")
      .trim();
  }

  /** 随模板启用的字段名（含未设默认值的占位字段）；已停用的字段不显示原始 id */
  function fieldsText(tpl: Template): string {
    const byId = fieldDefMap();
    const names = ((tpl.fields ?? []) as { id?: string }[])
      .map((f) => f.id && byId.get(f.id)?.name)
      .filter(Boolean);
    return names.join("、");
  }

  // 字段管理：列表来自全局 store（删除/修改后各处即时同步）；软删表供「清理」入口
  let defs = $derived(allFieldDefs());
  let deletedDefs = $derived(allDeletedFieldDefs());
  let purgeArmed = $state(false);
  let purgeTimer: number | undefined;

  function armPurge() {
    if (purgeArmed) {
      window.clearTimeout(purgeTimer);
      purgeArmed = false;
      purge();
      return;
    }
    purgeArmed = true;
    purgeTimer = window.setTimeout(() => (purgeArmed = false), 4000);
  }

  async function purge() {
    try {
      const n = await api.purgeDeletedFieldDefs();
      toast.show(t("templates.purged", { n }));
      await reloadFieldDefs();
    } catch (e) {
      ferror = String(e);
    }
  }
  let fname = $state("");
  let fkind = $state<FieldKind>("number");
  let fscope = $state<ItemType | "">("");
  let funit = $state("");
  let fchoices = $state("");
  let ferror = $state("");

  loadTemplates();
  // 面板里的模板增删改广播 data-changed → 这里自动刷新
  $effect(() => {
    dataVersion;
    loadTemplates();
  });

  async function addField() {
    ferror = "";
    if (!fname.trim()) {
      ferror = t("templates.name_required");
      return;
    }
    const options: Record<string, unknown> = {};
    if (funit.trim()) options.unit = funit.trim();
    if (fchoices.trim()) {
      options.choices = fchoices
        .split(/[,，]/)
        .map((c) => c.trim())
        .filter(Boolean);
    }
    try {
      await api.addFieldDef(fname.trim(), fkind, options, fscope || null);
      fname = "";
      funit = "";
      fchoices = "";
      await reloadFieldDefs();
    } catch (e) {
      ferror = String(e);
    }
  }

  // 两步确认删除（防误触）；提示会带受影响条目数
  let confirmingDelete = $state<string | null>(null);
  let confirmNote = $state(t("templates.confirm_delete"));

  async function removeField(def: FieldDef) {
    ferror = "";
    if (confirmingDelete !== def.id) {
      try {
        const n = await api.countItemsWithField(def.id);
        confirmNote =
          n > 0 ? t("templates.confirm_disable", { n }) : t("templates.confirm_delete");
      } catch {
        confirmNote = t("templates.confirm_delete");
      }
      confirmingDelete = def.id;
      setTimeout(() => {
        if (confirmingDelete === def.id) confirmingDelete = null;
      }, 4000);
      return;
    }
    try {
      confirmingDelete = null;
      await api.deleteFieldDef(def.id);
      if (editingId === def.id) editingId = null;
      await reloadFieldDefs();
    } catch (e) {
      ferror = String(e);
    }
  }

  // 行内编辑：改名（值随键迁移）/ 改单位 / 改选项
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editUnit = $state("");
  let editChoices = $state("");

  function startEdit(def: FieldDef) {
    ferror = "";
    editingId = def.id;
    editName = def.name;
    editUnit = (def.options.unit as string) ?? "";
    editChoices = (def.options.choices as string[] | undefined)?.join(", ") ?? "";
  }

  async function saveEdit(def: FieldDef) {
    ferror = "";
    const options: Record<string, unknown> = { ...def.options };
    if (def.kind === "number") {
      if (editUnit.trim()) options.unit = editUnit.trim();
      else delete options.unit;
    }
    if (def.kind === "select" || def.kind === "multiselect") {
      const list = editChoices
        .split(/[,，]/)
        .map((c) => c.trim())
        .filter(Boolean);
      if (list.length) options.choices = list;
      else delete options.choices;
    }
    try {
      await api.updateFieldDef(def.id, {
        name: editName.trim() || def.name,
        options,
      });
      editingId = null;
      await reloadFieldDefs();
    } catch (e) {
      ferror = String(e);
    }
  }
</script>

<h1>{t("templates.title")}</h1>

<section>
  <h2>{t("templates.manage_title")}</h2>
  <p class="hint">{t("templates.manage_hint")}</p>

  <table>
    <thead>
      <tr><th></th><th>{t("templates.th_name")}</th><th>{t("templates.th_type")}</th><th>{t("templates.th_tag")}</th><th>{t("templates.th_defaults")}</th><th>{t("templates.th_fields")}</th><th>{t("templates.th_pin")}</th><th></th></tr>
    </thead>
    <tbody>
      {#each templates as tpl (tpl.id)}
        <tr>
          <td>{tpl.icon ?? ""}</td>
          <td>{tpl.name}{#if tpl.builtin} <span class="bi">{t("templates.builtin")}</span>{/if}</td>
          <td>{typeLabel(tpl.item_type)}</td>
          <td>{tpl.tag ? `#${tpl.tag}` : ""}</td>
          <td class="opts">{defaultsText(tpl)}</td>
          <td class="opts">{fieldsText(tpl)}</td>
          <td>
            {#if tpl.item_type === "log"}
              <button
                class="edit"
                title={tpl.pinned ? t("templates.unpin_tip") : t("templates.pin_tip")}
                onclick={async () => {
                  await api.setTemplatePinned(tpl.id, !tpl.pinned);
                  await loadTemplates();
                }}>{tpl.pinned ? t("templates.pinned") : t("templates.unpinned")}</button>
            {:else}
              <span class="opts">—</span>
            {/if}
          </td>
          <td class="ops-btns">
            <button
              class="edit"
              title={t("templates.move_up")}
              onclick={async () => {
                await api.moveTemplate(tpl.id, true);
                await loadTemplates();
              }}>↑</button>
            <button
              class="edit"
              title={t("templates.move_down")}
              onclick={async () => {
                await api.moveTemplate(tpl.id, false);
                await loadTemplates();
              }}>↓</button>
            <button class="edit" onclick={() => openTemplate(tpl)}>{t("common.edit")}</button>
            {#if movingId === tpl.id}
              <button class="del confirm" onclick={() => removeTpl(tpl)}>{t("templates.confirm_delete")}</button>
            {:else}
              <button class="del" onclick={() => removeTpl(tpl)}>{t("common.delete")}</button>
            {/if}
          </td>
        </tr>
      {:else}
        <tr><td colspan="8" class="opts">{t("templates.empty")}</td></tr>
      {/each}
    </tbody>
  </table>

  <div class="add-row">
    <button class="primary" onclick={() => openTemplate(null)}>
      {t("templates.new")}
    </button>
  </div>
</section>

<section>
  <div class="sec-head">
    <h2>{t("templates.fields_title")}</h2>
    {#if deletedDefs.length > 0}
      <button class="del" class:confirm={purgeArmed} onclick={armPurge}>
        {purgeArmed
          ? t("templates.purge_confirm")
          : t("templates.purge", { n: deletedDefs.length })}
      </button>
    {/if}
  </div>
  <p class="hint">{t("templates.fields_hint")}</p>
  <table>
    <thead>
      <tr><th>{t("templates.th_name")}</th><th>{t("templates.th_type")}</th><th>{t("templates.th_scope")}</th><th>{t("templates.th_config")}</th><th></th></tr>
    </thead>
    <tbody>
      {#each defs as f (f.id)}
        {#if editingId === f.id}
          <tr class="editing">
            <td><input bind:value={editName} style="width: 90px" /></td>
            <td>{f.kind}</td>
            <td>{f.scope ? typeLabel(f.scope) : t("templates.global")}</td>
            <td>
              {#if f.kind === "number"}
                <input placeholder={t("templates.unit_ph")} bind:value={editUnit} style="width: 90px" />
              {:else if f.kind === "select" || f.kind === "multiselect"}
                <input placeholder={t("templates.choices_ph")} bind:value={editChoices} style="width: 180px" />
              {/if}
            </td>
            <td class="ops-btns">
              <button class="primary" onclick={() => saveEdit(f)}>{t("common.save")}</button>
              <button onclick={() => (editingId = null)}>{t("common.cancel")}</button>
            </td>
          </tr>
        {:else}
          <tr>
            <td>{f.name}{#if f.builtin} <span class="bi">{t("templates.default_badge")}</span>{/if}</td>
            <td>{f.kind}</td>
            <td>{f.scope ? typeLabel(f.scope) : t("templates.global")}</td>
            <td class="opts">{JSON.stringify(f.options)}</td>
            <td class="ops-btns">
              <button class="edit" onclick={() => startEdit(f)}>{t("common.edit")}</button>
              {#if confirmingDelete === f.id}
                <button class="del confirm" onclick={() => removeField(f)}>{confirmNote}</button>
              {:else}
                <button class="del" onclick={() => removeField(f)}>{t("common.delete")}</button>
              {/if}
            </td>
          </tr>
        {/if}
      {/each}
    </tbody>
  </table>

  <div class="add-row">
    <input placeholder={t("templates.name_ph")} bind:value={fname} />
    <select bind:value={fkind}>
      <option value="text">{t("templates.kind_text")}</option>
      <option value="number">{t("templates.kind_number")}</option>
      <option value="select">{t("templates.kind_select")}</option>
      <option value="multiselect">{t("templates.kind_multi")}</option>
      <option value="bool">{t("templates.kind_bool")}</option>
      <option value="date">{t("templates.kind_date")}</option>
      <option value="url">{t("templates.kind_url")}</option>
    </select>
    <select bind:value={fscope}>
      <option value="">{t("templates.scope_all")}</option>
      <option value="event">{t("templates.scope_event")}</option>
      <option value="task">{t("templates.scope_task")}</option>
      <option value="log">{t("templates.scope_log")}</option>
    </select>
    {#if fkind === "number"}
      <input placeholder={t("templates.unit_optional")} bind:value={funit} style="width: 110px" />
    {/if}
    {#if fkind === "select" || fkind === "multiselect"}
      <input placeholder={t("templates.choices_ph")} bind:value={fchoices} />
    {/if}
    <button class="primary" onclick={addField}>{t("templates.add_field")}</button>
  </div>
  {#if ferror}<p class="error">{ferror}</p>{/if}
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
    max-width: 760px;
  }

  h2 {
    font-size: 14px;
    margin: 0 0 10px;
  }

  .sec-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .sec-head h2 {
    margin: 0;
  }

  .hint {
    color: var(--text-dim);
    font-size: 13px;
  }

  table {
    border-collapse: collapse;
    width: 100%;
    font-size: 13px;
    margin-bottom: 12px;
  }

  th,
  td {
    text-align: left;
    padding: 5px 8px;
    border-bottom: 1px solid var(--border);
  }

  th {
    color: var(--text-dim);
    font-weight: 500;
    font-size: 12px;
  }

  .opts {
    color: var(--text-dim);
    font-size: 12px;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bi {
    font-size: 10px;
    color: var(--accent);
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 4px;
    padding: 0 4px;
  }

  .add-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
  }

  .add-row input {
    width: 140px;
  }

  .del {
    color: var(--danger);
    font-size: 12px;
    border: none;
    background: transparent;
    padding: 2px 6px;
  }

  .del.confirm {
    background: color-mix(in srgb, var(--danger) 18%, transparent);
    font-weight: 600;
  }

  .edit {
    font-size: 12px;
    border: none;
    background: transparent;
    color: var(--accent);
    padding: 2px 6px;
  }

  .ops-btns {
    white-space: nowrap;
  }

  tr.editing input {
    padding: 4px 8px;
    font-size: 13px;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
  }
</style>

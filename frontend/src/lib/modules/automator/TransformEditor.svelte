<script>
  // `transform` block: reshape what the previous blocks produced. Five operations, and the
  // form shows only the fields the chosen one uses.
  import Select from '$lib/ui/Select.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ExprField from './ExprField.svelte';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), catalog = null, sources = [] } = $props();

  const ops = $derived(
    (catalog?.ops?.transform ?? ['pick']).map((o) => ({
      value: o,
      label: $t(`automator.transform.${o}`)
    }))
  );
  const shapes = $derived(
    (catalog?.csv_shapes ?? ['objects', 'arrays', 'map']).map((s) => ({
      value: s,
      label: $t(`automator.transform.csv.${s}`)
    }))
  );

  // `set` builds an object. It is edited as an ordered list of rows, because an object
  // keyed by a name the user is still typing reorders and loses focus on every keystroke;
  // the rows are folded back into the object after each edit.
  let rows = $state(
    Object.entries(config.fields ?? {}).map(([key, value], i) => ({ id: i, key, value }))
  );
  let nextId = Object.keys(config.fields ?? {}).length;

  function sync() {
    const out = {};
    for (const row of rows) {
      const key = row.key.trim();
      if (key) out[key] = row.value;
    }
    config.fields = out;
  }
  function addField() {
    rows = [...rows, { id: nextId++, key: `field${rows.length + 1}`, value: '' }];
    sync();
  }
  function removeField(id) {
    rows = rows.filter((r) => r.id !== id);
    sync();
  }
</script>

<div class="stack">
  <Select label={$t('automator.transform.op')} options={ops} bind:value={config.op} />
  <p class="hint">{$t(`automator.transform.${config.op ?? 'pick'}.hint`)}</p>

  {#if ['pick', 'csv_parse', 'join'].includes(config.op)}
    <ExprField
      label={$t('automator.transform.source')}
      bind:value={config.source}
      placeholder={'{{steps.http1.output.body}}'}
      {sources}
    />
  {/if}

  {#if config.op === 'format'}
    <ExprField
      label={$t('automator.transform.template')}
      bind:value={config.template}
      rows={4}
      {sources}
    />
  {/if}

  {#if config.op === 'csv_parse'}
    <div class="row">
      <Select label={$t('automator.transform.csvShape')} options={shapes} bind:value={config.shape} />
      <Input label={$t('automator.transform.delimiter')} bind:value={config.delimiter} maxlength="1" placeholder="," />
    </div>
    <p class="hint">{$t(`automator.transform.csv.${config.shape || 'objects'}.hint`)}</p>
    {#if config.shape === 'map'}
      <div class="row">
        <Input label={$t('automator.transform.keyCol')} bind:value={config.key_col} placeholder={$t('automator.transform.colFirst')} />
        <Input label={$t('automator.transform.valueCol')} bind:value={config.value_col} placeholder={$t('automator.transform.colSecond')} />
      </div>
    {/if}
  {/if}

  {#if config.op === 'join'}
    <div class="row">
      <Input label={$t('automator.transform.separator')} bind:value={config.separator} placeholder=", " />
      <Input label={$t('automator.transform.field')} bind:value={config.field} placeholder="name" />
    </div>
  {/if}

  {#if config.op === 'set'}
    <div class="fields">
      {#each rows as row (row.id)}
        <div class="fieldrow">
          <input
            bind:value={row.key}
            oninput={sync}
            placeholder={$t('automator.transform.name')}
          />
          <ExprField
            bind:value={row.value}
            oninput={sync}
            {sources}
            placeholder={'{{steps.api1.output.body.id}}'}
          />
          <button type="button" class="icon" onclick={() => removeField(row.id)}>
            <Icon name="x" size={12} />
          </button>
        </div>
      {/each}
      <button type="button" class="link" onclick={addField}>
        <Icon name="plus" size={12} />
        {$t('automator.transform.addField')}
      </button>
    </div>
  {/if}
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .fieldrow {
    display: grid;
    grid-template-columns: 150px 1fr 24px;
    gap: var(--space-2);
    align-items: center;
    margin-bottom: var(--space-2);
  }
  .fieldrow input {
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    color: var(--red);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    background: none;
    border: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 0;
  }
</style>

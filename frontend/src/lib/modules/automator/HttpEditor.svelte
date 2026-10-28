<script>
  // `http` block: a call to something outside the app.
  //
  // Two fields carry the security story and both are visible rather than buried: headers
  // (the only place a vault reference is accepted, alongside the body) and the internal
  // targets switch, which is off until the user says otherwise.
  import Select from '$lib/ui/Select.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ExprField from './ExprField.svelte';
  import PairValue from './PairValue.svelte';
  import { t } from '$lib/i18n';

  let { config = $bindable({}), catalog = null, sources = [] } = $props();

  const methods = $derived((catalog?.http_methods ?? ['GET', 'POST']).map((m) => ({ value: m, label: m })));
  const parsers = $derived(
    (catalog?.parsers ?? ['json', 'text']).map((p) => ({
      value: p,
      label: $t(`automator.http.parse.${p}`)
    }))
  );
  const bodyKinds = ['none', 'json', 'text', 'form'];

  function addPair(key) {
    config[key] = [...(config[key] ?? []), { name: '', value: '' }];
  }
  function removePair(key, i) {
    config[key] = (config[key] ?? []).filter((_, j) => j !== i);
  }
</script>

<div class="stack">
  <div class="row">
    <Select label={$t('automator.http.method')} options={methods} bind:value={config.method} />
    <div class="grow">
      <ExprField
        label={$t('automator.http.url')}
        bind:value={config.url}
        placeholder="https://example.com/data.csv"
        {sources}
      />
    </div>
  </div>

  {#each ['query', 'headers'] as key (key)}
    <div class="pairs">
      <div class="pairhead">
        <span class="lbl">{$t(`automator.http.${key}`)}</span>
        <button type="button" class="link" onclick={() => addPair(key)}>
          <Icon name="plus" size={12} />
          {$t('automator.http.addPair')}
        </button>
      </div>
      {#each config[key] ?? [] as pair, i (i)}
        <div class="pair">
          <input class="pname" bind:value={pair.name} placeholder={$t('automator.http.name')} />
          <PairValue bind:value={pair.value} placeholder={$t('automator.http.value')} />
          <button type="button" class="icon" onclick={() => removePair(key, i)}>
            <Icon name="x" size={12} />
          </button>
        </div>
      {/each}
      <p class="hint">{$t(key === 'headers' ? 'automator.http.headerHint' : 'automator.http.queryHint')}</p>
    </div>
  {/each}

  <div class="row">
    <Select
      label={$t('automator.http.bodyKind')}
      options={bodyKinds.map((k) => ({ value: k, label: $t(`automator.http.body.${k}`) }))}
      bind:value={config.body_kind}
    />
    <Select label={$t('automator.http.parseAs')} options={parsers} bind:value={config.parse} />
  </div>

  {#if config.body_kind === 'json' || config.body_kind === 'text'}
    <ExprField
      label={$t('automator.http.body')}
      bind:value={config.body}
      rows={6}
      secrets
      {sources}
      hint={$t('automator.http.bodyHint')}
    />
  {/if}

  <label class="check">
    <input type="checkbox" bind:checked={config.allow_internal} />
    <span>
      {$t('automator.http.allowInternal')}
      <em>{$t('automator.http.allowInternalHint')}</em>
    </span>
  </label>

  <Input
    label={$t('automator.http.maxBytes')}
    type="number"
    min="1024"
    max="5242880"
    bind:value={config.max_bytes}
    placeholder="1048576"
  />
</div>

<style>
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: flex-end;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  .pairhead {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: var(--space-1);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
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
  .pair {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }
  .pair input {
    flex: 1;
    min-width: 0;
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  /* A header name is short and known ("Authorization"); the value is what needs the room. */
  .pair .pname {
    flex: none;
    width: 34%;
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
  .hint {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: var(--text-sm);
  }
  .check em {
    display: block;
    font-style: normal;
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>

<script>
  // A field that accepts `{{ … }}` references, with the list of what is actually readable
  // at this point in the graph one click away.
  //
  // That list is the difference between a canvas people use and one they abandon: the
  // references are checked on save, so guessing them by hand means saving to find out.
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let {
    value = $bindable(''),
    label = '',
    placeholder = '',
    hint = '',
    rows = 0,
    /** Ids of the blocks whose output this one can read. */
    sources = [],
    /** True for a field a vault reference is allowed in (headers, bodies, auth). */
    secrets = false,
    /** Called after every edit, for parents that keep a derived copy of the value. */
    oninput = null
  } = $props();

  let open = $state(false);
  let el = $state(null);

  const suggestions = $derived([
    ...sources.map((id) => ({ text: `{{steps.${id}.output}}`, note: id })),
    ...sources.map((id) => ({ text: `{{steps.${id}.error}}`, note: $t('automator.expr.error') })),
    { text: '{{run.started_at}}', note: $t('automator.expr.run') },
    { text: '{{workflow.name}}', note: $t('automator.expr.workflow') },
    { text: '{{input.key}}', note: $t('automator.expr.input') },
    ...(secrets ? [{ text: '{{vault.myvault.mykey}}', note: $t('automator.expr.vault') }] : [])
  ]);

  function insert(text) {
    const start = el?.selectionStart ?? value.length;
    const end = el?.selectionEnd ?? value.length;
    value = `${value.slice(0, start)}${text}${value.slice(end)}`;
    oninput?.(value);
    open = false;
    queueMicrotask(() => {
      el?.focus();
      const at = start + text.length;
      el?.setSelectionRange?.(at, at);
    });
  }
</script>

<div class="field">
  {#if label}
    <div class="top">
      <span class="lbl">{label}</span>
      <button class="ref" onclick={() => (open = !open)} type="button">
        <Icon name="link" size={12} />
        {$t('automator.expr.insert')}
      </button>
    </div>
  {/if}

  {#if rows > 0}
    <textarea bind:this={el} bind:value {placeholder} {rows} oninput={() => oninput?.(value)}
    ></textarea>
  {:else}
    <input bind:this={el} bind:value {placeholder} oninput={() => oninput?.(value)} />
  {/if}

  {#if hint}<p class="hint">{hint}</p>{/if}

  {#if open}
    <ul class="menu">
      {#each suggestions as s (s.text)}
        <li>
          <button type="button" onclick={() => insert(s.text)}>
            <code>{s.text}</code>
            <em>{s.note}</em>
          </button>
        </li>
      {/each}
      {#if !sources.length}
        <li class="none">{$t('automator.expr.noSources')}</li>
      {/if}
    </ul>
  {/if}
</div>

<style>
  .field {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .ref {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: 0;
    color: var(--accent);
    font-size: var(--text-xs);
    cursor: pointer;
    padding: 0;
  }
  input,
  textarea {
    width: 100%;
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .menu {
    position: absolute;
    z-index: 5;
    right: 0;
    top: 100%;
    width: 100%;
    max-height: 220px;
    overflow-y: auto;
    list-style: none;
    margin: 2px 0 0;
    padding: 0;
    background: var(--surface);
    border: 0.5px solid var(--border);
  }
  .menu button {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    text-align: left;
    background: none;
    border: 0;
    padding: var(--space-1) var(--space-2);
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--surface-2);
  }
  .menu code {
    font-size: var(--text-xs);
    color: var(--text);
  }
  .menu em {
    font-style: normal;
    font-size: 10px;
    color: var(--muted);
  }
  .none {
    padding: var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>

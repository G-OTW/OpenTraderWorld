<script>
  // The script's `input.*` declarations as a form. `values` holds raw form values keyed by
  // `InputMeta.key`; a key left out runs on the script's own default. A refused value (out of
  // range, wrong type, not an option) is shown next to its field and never clamped.
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { t } from '$lib/i18n';
  import { SOURCES, inputValues, defaultRaw } from './host.js';

  let { inputs = [], values = $bindable({}) } = $props();

  const errors = $derived(Object.fromEntries(inputValues(inputs, values).errors.map((e) => [e.key, e.reason])));
  const raw = (m) => values[m.key] ?? defaultRaw(m);
  const set = (key, v) => (values = { ...values, [key]: v });

  // A color is `#rrggbb` plus an opacity percent, stored as `#rrggbbaa`.
  const rgb = (m) => String(raw(m)).slice(0, 7);
  const alpha = (m) => {
    const s = String(raw(m));
    return s.length === 9 ? Math.round((parseInt(s.slice(7), 16) / 255) * 100) : 100;
  };
  const withAlpha = (hex, pct) => {
    const a = Math.round((Math.max(0, Math.min(100, pct)) / 100) * 255);
    return a === 255 ? hex : hex + a.toString(16).padStart(2, '0');
  };
  const num = (e) => (e.currentTarget.value === '' ? '' : Number(e.currentTarget.value));
</script>

{#if !inputs.length}
  <p class="hint">{$t('histviz.script.noInputs')}</p>
{/if}
{#each inputs as m (m.key)}
  <div class="field">
    <span>{m.title || m.key}</span>
    {#if m.kind === 'Int' || m.kind === 'Float'}
      <input
        type="number"
        min={m.minval}
        max={m.maxval}
        step={m.step ?? (m.kind === 'Int' ? 1 : 'any')}
        value={raw(m)}
        oninput={(e) => set(m.key, num(e))}
      />
    {:else if m.kind === 'Bool'}
      <input type="checkbox" checked={raw(m)} onchange={(e) => set(m.key, e.currentTarget.checked)} />
    {:else if m.kind === 'Source'}
      <Dropdown value={raw(m)} ariaLabel={m.title || m.key} options={SOURCES.map((s) => ({ value: s, label: s }))} onpick={(v) => set(m.key, v)} />
    {:else if m.kind === 'Color'}
      <span class="color">
        <input type="color" value={rgb(m)} oninput={(e) => set(m.key, withAlpha(e.currentTarget.value, alpha(m)))} />
        <label class="opacity">
          {$t('histviz.script.opacity')}
          <input type="number" min="0" max="100" step="1" value={alpha(m)} oninput={(e) => set(m.key, withAlpha(rgb(m), Number(e.currentTarget.value)))} />
        </label>
      </span>
    {:else if m.options?.length}
      <Dropdown value={raw(m)} ariaLabel={m.title || m.key} options={m.options.map((o) => ({ value: o, label: o }))} onpick={(v) => set(m.key, v)} />
    {:else}
      <input value={raw(m)} spellcheck="false" oninput={(e) => set(m.key, e.currentTarget.value)} />
    {/if}
    {#if errors[m.key]}<small class="err">{$t(`histviz.script.input.${errors[m.key]}`)}</small>{/if}
  </div>
{/each}

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .field > span {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .field input[type='checkbox'] {
    align-self: flex-start;
  }
  .color {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .opacity {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .opacity input {
    width: 72px;
  }
  .hint {
    margin: 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
</style>

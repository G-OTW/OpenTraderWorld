<script>
  // The value half of a header row: a plain value, or a vault reference.
  //
  // A secret is never typed in here. The vault holds it, this field holds the reference
  // `{{vault.<vault>.<item>}}`, and the engine resolves it at run time and scrubs it back
  // out of the stored trace. The switch is per row because a header list mixes the two:
  // `Accept: application/json` next to `Authorization: Bearer {{vault.broker.key}}`.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import { vaultApi } from '$lib/vault/api.js';
  import { t } from '$lib/i18n';

  let { value = $bindable(''), placeholder = '' } = $props();

  const REF = /^\{\{\s*vault\.([^.\s}]+)\.([^\s}]+)\s*\}\}$/;

  let vaults = $state([]);
  let loaded = $state(false);
  // Sticky: an empty row switched to vault mode has no reference to read the mode back
  // from, so the choice has to be remembered rather than derived.
  let wantVault = $state(REF.test(value ?? ''));

  const asVault = $derived(wantVault || REF.test(value ?? ''));
  const current = $derived(REF.test(value ?? '') ? value : '');

  // A header per vault, the Dropdown's stand-in for optgroup. The reference that is
  // stored but no longer in the vault is kept as its own row, so opening the menu on a
  // deleted item does not silently blank a working header.
  const options = $derived([
    ...vaults.flatMap((v) =>
      (v.items ?? []).length
        ? [
            { header: true, label: v.name },
            ...v.items.map((it) => ({
              value: `{{vault.${v.name}.${it.name}}}`,
              label: `${v.name}.${it.name}`
            }))
          ]
        : []
    ),
    ...(current && !vaults.some((v) => (v.items ?? []).some((it) => `{{vault.${v.name}.${it.name}}}` === current))
      ? [{ value: current, label: current }]
      : [])
  ]);

  async function toVault() {
    wantVault = true;
    if (loaded) return;
    try {
      vaults = await vaultApi.list();
    } catch {
      vaults = [];
    } finally {
      loaded = true;
    }
  }

  function toPlain() {
    wantVault = false;
    if (REF.test(value ?? '')) value = '';
  }
</script>

<div class="val">
  {#if asVault}
    <Dropdown
      bind:value
      {options}
      placeholder={$t('automator.http.pickSecret')}
      ariaLabel={$t('automator.http.vaultValue')}
    />
    {#if loaded && !options.length}
      <p class="hint">{$t('automator.http.noSecret')}</p>
    {/if}
  {:else}
    <input bind:value {placeholder} />
  {/if}
</div>

<div class="switch" role="group" aria-label={$t('automator.http.valueKind')}>
  <button
    type="button"
    class:on={!asVault}
    onclick={toPlain}
    title={$t('automator.http.plainValue')}
    aria-pressed={!asVault}
  >
    <Icon name="type" size={12} />
  </button>
  <button
    type="button"
    class:on={asVault}
    onclick={toVault}
    title={$t('automator.http.vaultValue')}
    aria-pressed={asVault}
  >
    <Icon name="key" size={12} />
  </button>
</div>

<style>
  .val {
    flex: 1;
    min-width: 0;
  }
  .val input {
    width: 100%;
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .hint {
    margin: 2px 0 0;
    font-size: 10px;
    color: var(--muted);
  }
  /* One frame around the pair of buttons, none around each: same rule as everywhere else.
     The frame is a control, so it stands exactly as tall as the field beside it. */
  .switch {
    display: flex;
    flex: none;
    height: var(--control-h);
    border: 0.5px solid var(--border);
  }
  .switch button {
    display: inline-flex;
    align-items: center;
    padding: 0 var(--space-2);
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
  }
  .switch button.on {
    background: var(--surface-2);
    color: var(--accent);
  }
</style>

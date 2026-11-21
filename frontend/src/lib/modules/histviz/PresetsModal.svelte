<script>
  // Entry presets (presets.js): the list, or one preset being written in its place, and the
  // keys 1 to 9 that fill the order ticket with one. Kept in the global chart settings by the
  // page.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import SegPick from './SegPick.svelte';
  import { t } from '$lib/i18n';
  import { KEYS, UNITS, blankPreset, byKey, savePreset } from './presets.js';

  let {
    open = $bindable(false),
    presets = [],
    keys = false,
    /** (list): the whole list after a change. */
    onsave,
    /** (on): the keys turned on or off. */
    onkeys
  } = $props();

  /** The preset being written, with switches for its optional parts. */
  let draft = $state(null);

  /** A preset for both directions: the ticket keeps its side, the quick bar takes the button's. */
  const bothText = () => `${$t('histviz.presets.long')} / ${$t('histviz.presets.short')}`;
  const unitOptions = $derived(UNITS.map((u) => ({ value: u, label: $t(`histviz.presets.unit.${u}`) })));
  const sideOptions = $derived([
    { value: 'buy', label: $t('histviz.presets.long') },
    { value: 'sell', label: $t('histviz.presets.short') },
    { value: 'both', label: bothText() }
  ]);
  /** Which key each other preset holds: a key taken here moves to this one. */
  const keyOwner = $derived(Object.fromEntries(presets.filter((p) => p.key != null && p.id !== draft?.id).map((p) => [p.key, p.name])));

  function edit(p) {
    const b = blankPreset();
    draft = {
      ...b,
      ...p,
      key: p?.key ?? null,
      qtyOn: p?.qty != null,
      qty: p?.qty ?? 1,
      tpOn: p ? p.tp != null : true,
      tp: { ...(p?.tp ?? b.tp) },
      slOn: p ? p.sl != null : true,
      sl: { ...(p?.sl ?? b.sl) }
    };
  }
  function save() {
    const d = draft;
    const p = { id: d.id, name: d.name, side: d.side, qty: d.qtyOn ? d.qty : null, tp: d.tpOn ? d.tp : null, sl: d.slOn ? d.sl : null, key: d.key };
    onsave?.(savePreset(presets, p));
    draft = null;
  }
  const remove = (id) => onsave?.(presets.filter((p) => p.id !== id));

  const unitText = (x) => (x.unit === 'pct' ? `${x.value}%` : `${x.value} ${$t(`histviz.presets.unit.${x.unit}`)}`);
  const draftOk = $derived(
    !!draft &&
      String(draft.name).trim() !== '' &&
      (!draft.qtyOn || Number(draft.qty) > 0) &&
      (!draft.tpOn || Number(draft.tp.value) > 0) &&
      (!draft.slOn || Number(draft.sl.value) > 0)
  );
</script>

<Modal bind:open title={$t('histviz.presets.title')} size="md">
  <div class="pm">
    {#if draft}
      <div class="ehead">
        <button type="button" class="back" onclick={() => (draft = null)}><Icon name="chevron-left" size={14} />{$t('histviz.presets.list')}</button>
        <span class="etitle">{draft.id ? $t('histviz.presets.editTitle') : $t('histviz.presets.add')}</span>
      </div>

      <div class="form">
        <div class="row">
          <span class="lab">{$t('histviz.presets.name')}</span>
          <div class="ctl"><input type="text" bind:value={draft.name} maxlength="40" placeholder={$t('histviz.presets.namePlaceholder')} /></div>
        </div>
        <div class="row">
          <span class="lab">{$t('histviz.presets.side')}</span>
          <div class="ctl"><SegPick bind:value={draft.side} options={sideOptions} ariaLabel={$t('histviz.presets.side')} /></div>
        </div>
        <div class="row">
          <span class="lab">{$t('histviz.presets.entry')}</span>
          <div class="ctl"><span class="fixed">{$t('histviz.presets.marketEntry')}</span></div>
        </div>
        <div class="row">
          <span class="lab">{$t('histviz.presets.sizeLabel')}</span>
          <div class="ctl inline">
            <button type="button" class="sw" role="switch" aria-checked={draft.qtyOn} aria-label={$t('histviz.presets.sizeLabel')} onclick={() => (draft.qtyOn = !draft.qtyOn)}></button>
            {#if draft.qtyOn}
              <input class="num" type="number" min="0" step="any" bind:value={draft.qty} aria-label={$t('histviz.presets.sizeLabel')} />
            {:else}
              <span class="muted">{$t('histviz.presets.sizeChart')}</span>
            {/if}
          </div>
        </div>

        {#each [['tp', 'takeProfit', 'green'], ['sl', 'stopLoss', 'red']] as [k, label, tone] (k)}
          <div class="row">
            <span class="lab">{$t(`histviz.trade.${label}`)}</span>
            <div class="ctl inline">
              <button
                type="button"
                class="sw {tone}"
                role="switch"
                aria-checked={draft[`${k}On`]}
                aria-label={$t(`histviz.trade.${label}`)}
                onclick={() => (draft[`${k}On`] = !draft[`${k}On`])}
              ></button>
              {#if draft[`${k}On`]}
                <input class="num" type="number" min="0" step="any" bind:value={draft[k].value} aria-label={$t(`histviz.trade.${label}`)} />
                <SegPick bind:value={draft[k].unit} options={unitOptions} ariaLabel={$t(`histviz.trade.${label}`)} />
              {:else}
                <span class="muted">{$t('histviz.presets.off')}</span>
              {/if}
            </div>
          </div>
        {/each}

        <div class="row">
          <span class="lab">{$t('histviz.presets.key')}</span>
          <div class="ctl inline">
            <div class="keys" role="radiogroup" aria-label={$t('histviz.presets.key')}>
              <button type="button" role="radio" aria-checked={draft.key == null} class:on={draft.key == null} onclick={() => (draft.key = null)}>
                {$t('histviz.presets.noKey')}
              </button>
              {#each KEYS as k (k)}
                <button
                  type="button"
                  role="radio"
                  aria-checked={draft.key === k}
                  class:on={draft.key === k}
                  class:taken={keyOwner[k] != null && draft.key !== k}
                  title={keyOwner[k] ? $t('histviz.presets.keyTaken', { name: keyOwner[k] }) : ''}
                  onclick={() => (draft.key = k)}
                >
                  {k}
                </button>
              {/each}
            </div>
          </div>
        </div>
        <div class="row">
          <span class="lab"></span>
          <p class="note">{$t('histviz.presets.unitsHint')}</p>
        </div>
      </div>

      <div class="foot">
        <Button variant="ghost" onclick={() => (draft = null)}>{$t('common.cancel')}</Button>
        <Button variant="primary" onclick={save} disabled={!draftOk}>{$t('common.save')}</Button>
      </div>
    {:else}
      <div class="bar">
        <p class="hint">{$t('histviz.presets.intro')}</p>
        <Button variant="primary" size="sm" icon="plus" onclick={() => edit(null)}>{$t('histviz.presets.add')}</Button>
      </div>

      <div class="keyrow">
        <span class="main">
          <span class="name">{$t('histviz.presets.keysOn')}</span>
          <span class="sub">{$t('histviz.presets.keysHint')}</span>
        </span>
        <button type="button" class="sw" role="switch" aria-checked={keys} aria-label={$t('histviz.presets.keysOn')} onclick={() => onkeys?.(!keys)}></button>
      </div>

      {#if presets.length}
        <ul class="list">
          {#each byKey(presets) as p (p.id)}
            <li>
              <span class="kbd" class:none={p.key == null}>{p.key ?? ''}</span>
              <span class="main">
                <span class="name" title={p.name}>{p.name}</span>
                <span class="chips">
                  <span class="chip {p.side === 'sell' ? 'red' : p.side === 'both' ? '' : 'green'}">{p.side === 'sell' ? $t('histviz.presets.short') : p.side === 'both' ? bothText() : $t('histviz.presets.long')}</span>
                  <span class="chip">{$t('histviz.presets.market')}</span>
                  {#if p.qty != null}<span class="chip">{$t('histviz.presets.size', { n: p.qty })}</span>{/if}
                  {#if p.tp}<span class="chip green">TP {unitText(p.tp)}</span>{/if}
                  {#if p.sl}<span class="chip red">SL {unitText(p.sl)}</span>{/if}
                </span>
              </span>
              <span class="acts">
                <button type="button" class="ibtn" aria-label={$t('common.edit')} title={$t('common.edit')} onclick={() => edit(p)}><Icon name="pencil" size={13} /></button>
                <button type="button" class="ibtn danger" aria-label={$t('common.delete')} title={$t('common.delete')} onclick={() => remove(p.id)}><Icon name="trash" size={13} /></button>
              </span>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="empty">
          <Icon name="layers" size={22} />
          <span>{$t('histviz.presets.none')}</span>
        </div>
      {/if}
    {/if}
  </div>
</Modal>

<style>
  .pm {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    min-height: 320px;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.45;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .bar {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .bar > :global(.btn) {
    flex: none;
  }
  .keyrow {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    background: var(--surface-2);
  }

  .list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .list li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    min-height: 56px;
    padding: var(--space-2) var(--space-3);
    background: var(--surface);
  }
  .list li + li {
    border-top: 1px solid var(--border);
  }
  .list li:hover {
    background: var(--surface-2);
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    min-width: 0;
  }
  .name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sub {
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    padding: 1px var(--space-2);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--text-xs);
    white-space: nowrap;
  }
  .chip.green {
    color: var(--green);
    background: color-mix(in srgb, var(--green) 12%, transparent);
  }
  .chip.red {
    color: var(--red);
    background: color-mix(in srgb, var(--red) 12%, transparent);
  }
  .kbd {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border: 1px solid var(--border);
    border-bottom-width: 2px;
    border-radius: var(--radius);
    background: var(--surface-2);
    font-family: var(--mono);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .kbd.none {
    border-style: dashed;
    border-bottom-width: 1px;
    background: transparent;
  }
  .acts {
    display: inline-flex;
    gap: 2px;
    opacity: 0.35;
    transition: opacity 0.12s;
  }
  .list li:hover .acts,
  .acts:focus-within {
    opacity: 1;
  }
  .ibtn {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .ibtn:hover {
    background: var(--surface);
    color: var(--text);
  }
  .ibtn.danger:hover {
    color: var(--red);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-8) var(--space-4);
    border: 1px dashed var(--border);
    border-radius: var(--radius-lg);
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .sw {
    position: relative;
    width: 30px;
    height: 17px;
    flex: none;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    cursor: pointer;
  }
  .sw::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 0.15s;
  }
  .sw[aria-checked='true'] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .sw.green[aria-checked='true'] {
    background: var(--green);
    border-color: var(--green);
  }
  .sw.red[aria-checked='true'] {
    background: var(--red);
    border-color: var(--red);
  }
  .sw[aria-checked='true']::after {
    transform: translateX(13px);
    background: var(--surface);
  }

  .ehead {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding-bottom: var(--space-3);
    border-bottom: 1px solid var(--border);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .back:hover {
    color: var(--text);
  }
  .etitle {
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: grid;
    grid-template-columns: 150px minmax(0, 1fr);
    align-items: center;
    gap: var(--space-3);
    min-height: 32px;
  }
  .lab {
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .ctl {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
    max-width: 320px;
  }
  .ctl.inline {
    flex-direction: row;
    align-items: center;
    flex-wrap: wrap;
    max-width: none;
  }
  .fixed {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .num {
    width: 88px;
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: 1.4;
  }
  .keys {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .keys button {
    min-width: 30px;
    height: 30px;
    padding: 0 var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    font-family: var(--mono);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .keys button:first-child {
    font-family: inherit;
    color: var(--muted);
  }
  .keys button:hover {
    border-color: color-mix(in srgb, var(--text) 25%, var(--border));
  }
  .keys button.taken {
    color: var(--muted);
    border-style: dashed;
  }
  .keys button.on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    border-style: solid;
  }
  .foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }

  @media (max-width: 640px) {
    .row {
      grid-template-columns: 1fr;
      gap: var(--space-1);
    }
    .bar {
      flex-direction: column;
    }
  }
</style>

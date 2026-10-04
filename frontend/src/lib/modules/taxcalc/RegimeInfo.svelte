<script>
  // The (i) beside a country: which rules apply and when they were last checked against
  // the official sources (month.year). Hover or focus shows it through the app-wide tip.
  import Icon from '$lib/ui/Icon.svelte';
  import { tip } from '$lib/ui/tip.svelte.js';
  import { t } from '$lib/i18n';

  /** `info`: a regime from the templates list, or the `regime` block of a result. */
  let { info = null, size = 13 } = $props();

  // "2026-09-25" -> "09.2026".
  const checked = $derived.by(() => {
    const m = /^(\d{4})-(\d{2})/.exec(info?.verified_on ?? '');
    return m ? `${m[2]}.${m[1]}` : '';
  });
  const custom = $derived(!info || info.custom || info.id === 'custom_flat' || info.regime === 'custom_flat');

  function rows() {
    if (custom) return [{ label: $t('taxcalc.regime.customInfo'), value: '' }];
    const out = [
      checked && info.status === 'verified'
        ? { label: $t('taxcalc.regime.checked'), value: checked }
        : { label: $t('taxcalc.regime.checked'), value: $t('taxcalc.regime.notChecked'), tone: 'warn' }
    ];
    const years = info.years ?? (info.year ? [info.year] : []);
    if (years.length) out.push({ label: $t('taxcalc.regime.yearsLabel'), value: years.join(', ') });
    if (info.coverage === 'simple') out.push({ label: $t('taxcalc.regime.coverage'), value: $t('taxcalc.regime.simpleBadge'), tone: 'warn' });
    for (const s of (info.sources ?? []).slice(0, 4)) out.push({ label: '·', value: s });
    return out;
  }

  const show = (e) => tip.show(e, { title: info?.label ?? '', rows: rows() });
  const showAt = (e) => {
    const r = e.currentTarget.getBoundingClientRect();
    tip.show({ clientX: r.right, clientY: r.bottom }, { title: info?.label ?? '', rows: rows() });
  };
</script>

<button
  type="button"
  class="rinfo"
  class:warn={!custom && info?.status !== 'verified'}
  aria-label={$t('taxcalc.regime.infoAria')}
  onpointerenter={show}
  onpointermove={(e) => tip.move(e)}
  onpointerleave={() => tip.hide()}
  onfocus={showAt}
  onblur={() => tip.hide()}
  onclick={(e) => e.stopPropagation()}
>
  <Icon name="info" {size} />
  {#if checked && info?.status === 'verified'}<span class="date">{checked}</span>{/if}
</button>

<style>
  .rinfo {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0 2px;
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: help;
    font-size: var(--text-xs);
    line-height: 1;
    vertical-align: middle;
  }
  .rinfo:hover,
  .rinfo:focus-visible {
    color: var(--accent);
  }
  .rinfo.warn {
    color: var(--amber);
  }
  .date {
    font-family: var(--mono, ui-monospace, monospace);
    font-variant-numeric: tabular-nums;
  }
</style>

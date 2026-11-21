<script>
  // File-wide reading rules: which table of a stacked statement to read, how to read its
  // dates and its numbers. They change how *every* column is read, which is why they sit
  // above the per-column table rather than inside it.
  //
  // Renders bare label+control blocks so the caller can lay them out in one settings row
  // together with its own module-specific controls.
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let {
    /** The mapping being edited (read-only here). */
    mapping,
    /** Section names found in a stacked statement; empty for an ordinary flat file. */
    sections = [],
    labels = {},
    /** Re-detect against another table of the statement. */
    onsection = () => {},
    /** Patch the mapping ({ date_order }, { decimal }, { tz_offset }, { tz }). */
    onchange = () => {},
    /** Offer named zones (`mapping.tz`, daylight saving followed) above the fixed offsets.
     *  Only for a backend whose mapping reads `tz`. */
    zones = false
  } = $props();

  const label = (key) => labels[key] ?? key;

  // A quarter-hour zone (India, Nepal, Chatham) is not offered: an operation keeps its
  // day, and no broker writes a naive timestamp that lands differently for those.
  const OFFSETS = [
    -720, -600, -540, -480, -420, -360, -300, -240, -180, -120, -60, 0, 60, 120, 180, 240, 300,
    330, 480, 540, 600, 660, 720
  ];

  const ZONE_NAMES = (() => {
    if (!zones) return [];
    try {
      return typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : [];
    } catch {
      return [];
    }
  })();
  const offsetLabel = (off) => `UTC${off >= 0 ? '+' : '−'}${Math.abs(off / 60)}`;
  // A named zone is its name, a fixed offset `off:<minutes>`; `#…` are group headers.
  const tzOptions = $derived(
    ZONE_NAMES.length
      ? [
          { value: '#zones', label: label('timezoneZones'), header: true },
          ...ZONE_NAMES.map((z) => ({ value: z, label: z })),
          { value: '#fixed', label: label('timezoneFixed'), header: true },
          ...OFFSETS.map((off) => ({ value: `off:${off}`, label: offsetLabel(off) }))
        ]
      : OFFSETS.map((off) => ({ value: `off:${off}`, label: offsetLabel(off) }))
  );
  const tzValue = $derived(zones && mapping.tz ? mapping.tz : `off:${mapping.tz_offset}`);
  function pickTz(v) {
    if (typeof v !== 'string' || v.startsWith('#')) return;
    if (!v.startsWith('off:')) return onchange({ tz: v });
    const tz_offset = Number(v.slice(4));
    onchange(zones ? { tz: null, tz_offset } : { tz_offset });
  }
</script>

{#if sections.length}
  <!-- A statement stacks several tables in one file; only one of them holds the ledger.
       Changing this re-reads the file, so the column mapping starts over. -->
  <div class="set">
    <span>{label('section')}</span>
    <Dropdown
      value={mapping.section ?? ''}
      onpick={onsection}
      ariaLabel={label('section')}
      options={[
        ...sections.map((s) => ({ value: s, label: s })),
        { value: '', label: label('sectionFlat') }
      ]}
    />
  </div>
{/if}

<div class="set">
  <span>{label('dateOrder')}</span>
  <Dropdown
    value={mapping.date_order}
    onpick={(v) => onchange({ date_order: v })}
    ariaLabel={label('dateOrder')}
    options={[
      { value: 'auto', label: label('dateOrderAuto') },
      { value: 'dmy', label: label('dateOrderDmy') },
      { value: 'mdy', label: label('dateOrderMdy') },
      { value: 'ymd', label: label('dateOrderYmd') }
    ]}
  />
</div>

<div class="set">
  <span>{label('decimal')}</span>
  <Dropdown
    value={mapping.decimal}
    onpick={(v) => onchange({ decimal: v })}
    ariaLabel={label('decimal')}
    options={[
      { value: 'auto', label: label('decimalAuto') },
      { value: '.', label: label('decimalDot') },
      { value: ',', label: label('decimalComma') }
    ]}
  />
</div>

<div class="set">
  <span>{label('timezone')}</span>
  <Dropdown value={tzValue} onpick={pickTz} ariaLabel={label('timezone')} options={tzOptions} />
</div>

<style>
  .set {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .set > span {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
</style>

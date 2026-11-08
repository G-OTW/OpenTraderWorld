<script>
  // What each source column becomes. One row per column: its header, what its values
  // look like, a sample, and the field it is mapped to.
  //
  // The confidence dot qualifies the detector's guess without competing with it, and is
  // shown only while that guess still stands — an edited column has nothing to be
  // confident about. Labels are passed in: the wording belongs to the module (a trade's
  // "entry price" is an operation's "price"), the layout does not.
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';

  let {
    columns = [],
    /** [{ id, label }] — the fields this module can map a column onto. */
    targets = [],
    /** (col) => 'ignore' | '<target id>' | 'custom' */
    value = () => 'ignore',
    /** (col) => 'none' | 'manual' | 'high' | 'medium' | 'low' */
    confidence = () => 'none',
    /** Offer "keep as a custom field" (only modules that have somewhere to put it). */
    allowCustom = false,
    labels = {},
    onselect = () => {}
  } = $props();

  const label = (key, fallback = '') => labels[key] ?? fallback;
  const headerOf = (col) =>
    col.header?.trim() || (labels.unnamed?.(col.index + 1) ?? `#${col.index + 1}`);
</script>

<ul class="collist">
  {#each columns as col (col.index)}
    <li class:mapped={value(col) !== 'ignore'}>
      <div class="col-id">
        <span class="col-name">{headerOf(col)}</span>
        <span class="col-kind">{labels.kind?.(col.kind) ?? col.kind}</span>
        {#if confidence(col) !== 'none' && confidence(col) !== 'manual'}
          <span class="dot {confidence(col)}" title={labels.confidence?.(confidence(col)) ?? ''}
          ></span>
        {/if}
        {#if col.ambiguous_dates}
          <span class="amb" title={label('ambiguousDates')}><Icon name="alert-triangle" size={11} /></span>
        {/if}
      </div>
      <div class="samples mono">
        {col.samples.length ? col.samples.join(' · ') : label('noSamples')}
      </div>
      <div class="target">
        <Dropdown
          value={value(col)}
          onpick={(v) => onselect(col, v)}
          ariaLabel={col.name}
          options={[
            { value: 'ignore', label: label('ignore') },
            ...targets.map((t0) => ({ value: t0.id, label: t0.label })),
            ...(allowCustom ? [{ value: 'custom', label: label('custom') }] : [])
          ]}
        />
      </div>
    </li>
  {/each}
</ul>

<style>
  .collist {
    list-style: none;
    border: 0.5px solid var(--border);
    max-height: 60vh;
    overflow-y: auto;
  }
  .collist li {
    display: grid;
    grid-template-columns: 1fr 150px;
    grid-template-areas: 'id target' 'samples target';
    gap: 2px var(--space-3);
    align-items: center;
    padding: 6px var(--space-3);
    border-bottom: 0.5px solid var(--border);
    border-left: 1.5px solid transparent;
  }
  .collist li:last-child {
    border-bottom: none;
  }
  .collist li.mapped {
    border-left-color: var(--accent);
  }
  .col-id {
    grid-area: id;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .col-name {
    font-weight: var(--fw-medium);
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .col-kind {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--dim);
  }
  /* Confidence is a dot, not a word: it qualifies the guess without competing with it. */
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .dot.high {
    background: var(--green);
  }
  .dot.medium {
    background: var(--amber);
  }
  .dot.low {
    background: var(--red);
  }
  .amb {
    color: var(--amber);
    display: inline-flex;
  }
  .samples {
    grid-area: samples;
    font-size: var(--text-xs);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--mono);
  }
  .target {
    grid-area: target;
    width: 100%;
  }
</style>

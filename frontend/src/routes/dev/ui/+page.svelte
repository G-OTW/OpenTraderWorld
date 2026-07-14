<script>
  // Design-system gallery (phase 7, A2.11). Every primitive, every state, both
  // themes. This is the review surface: judge the look here, once, before any
  // page migration — not spread across 32 routes.
  //
  // Not a module: absent from registry.js, listed in the layout's PUBLIC_ROUTES
  // so it renders chrome-less and needs no session. Delete it when phase 7 ends,
  // or keep it as living documentation of the system.
  import { theme } from '$lib/theme/store.svelte.js';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import IndicatorPicker from '$lib/ui/IndicatorPicker.svelte';
  import Card from '$lib/ui/Card.svelte';
  import Table from '$lib/ui/Table.svelte';
  import PageHeader from '$lib/ui/PageHeader.svelte';
  import StatCard from '$lib/ui/StatCard.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';

  const VARIANTS = ['primary', 'secondary', 'ghost', 'danger'];

  let text = $state('AAPL');
  let amount = $state(1240.5);
  let ccy = $state('EUR');
  let bad = $state('');
  let modalOpen = $state(false);
  let tab = $state('one');
  let sort = $state({ key: 'pnl', dir: 'desc' });
  let pickIndicator = $state('rsi');
  const demoPickerGroups = [
    { label: 'Moving averages', items: [
      { value: 'sma', label: 'SMA' }, { value: 'ema', label: 'EMA' }, { value: 'wma', label: 'WMA' },
      { value: 'hma', label: 'Hull MA' }, { value: 'dema', label: 'DEMA' }, { value: 'tema', label: 'TEMA' }
    ]},
    { label: 'Momentum', items: [
      { value: 'rsi', label: 'RSI' }, { value: 'macd', label: 'MACD' }, { value: 'stoch_k', label: 'Stochastic %K' },
      { value: 'cci', label: 'CCI' }, { value: 'roc', label: 'Rate of Change' }
    ]},
    { label: 'Volatility & bands', items: [
      { value: 'atr', label: 'ATR' }, { value: 'bb_upper', label: 'Bollinger upper' }, { value: 'stddev', label: 'Std deviation' }
    ]}
  ];

  const columns = [
    { key: 'symbol', label: 'Symbol', sortable: true },
    { key: 'side', label: 'Side' },
    { key: 'qty', label: 'Qty', numeric: true, sortable: true },
    { key: 'entry', label: 'Entry', numeric: true },
    { key: 'pnl', label: 'PnL', numeric: true, sortable: true },
    { key: 'status', label: 'Status' }
  ];

  // Deliberately mixed magnitudes: this is where tabular figures earn their keep.
  const rows = [
    { id: 1, symbol: 'AAPL', side: 'Long', qty: 100, entry: 182.4, pnl: 1240.5, status: 'closed' },
    { id: 2, symbol: 'BTC/USD', side: 'Long', qty: 0.75, entry: 61840.0, pnl: -318.2, status: 'open' },
    { id: 3, symbol: 'ES', side: 'Short', qty: 2, entry: 5218.25, pnl: 87.5, status: 'closed' },
    { id: 4, symbol: 'EURUSD', side: 'Short', qty: 50000, entry: 1.0842, pnl: -12.9, status: 'open' }
  ];

  const fmt = (n) => n.toLocaleString('fr-FR', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const sign = (n) => (n > 0 ? '+' : n < 0 ? '−' : '');
  const tone = (s) => (s === 'open' ? 'accent' : 'neutral');

  // Explicit flip off whatever is actually rendering, so 'system' resolves first.
  function cycleTheme() {
    theme.set(theme.resolved === 'dark' ? 'light' : 'dark');
  }
</script>

<svelte:head><title>OTW — UI gallery</title></svelte:head>

<div class="gallery">
  <PageHeader title="Design system" subtitle="Phase 7 primitives — judge here, once.">
    {#snippet actions()}
      <Button icon="moon" onclick={cycleTheme}>Toggle theme</Button>
    {/snippet}
  </PageHeader>

  <!-- ── Type scale ──────────────────────────────────────────────────────── -->
  <Card title="Type scale" subtitle="Body is 14px. The jump to 28px is what reads as product.">
    <div class="stack">
      <p style:font-size="var(--text-xl)" style:font-weight="var(--fw-semibold)">
        1 240,50 € — xl / 28px / semibold
      </p>
      <p style:font-size="var(--text-lg)" style:font-weight="var(--fw-semibold)">
        Page title — lg / 20px / semibold
      </p>
      <p style:font-size="var(--text-md)" style:font-weight="var(--fw-medium)">
        Section subtitle — md / 16px / medium
      </p>
      <p style:font-size="var(--text-base)">App body — base / 14px / normal</p>
      <p style:font-size="var(--text-sm)">Dense body, table cells — sm / 13px</p>
      <p style:font-size="var(--text-xs)" style:color="var(--muted)">
        Labels, meta, timestamps — xs / 12px / muted
      </p>
    </div>
  </Card>

  <!-- ── Color ───────────────────────────────────────────────────────────── -->
  <Card title="Color" subtitle="Accent is for the primary action and active state. Nothing else.">
    <div class="swatches">
      {#each ['--accent', '--green', '--amber', '--red', '--text', '--muted', '--border', '--surface-2'] as token (token)}
        <div class="swatch">
          <span class="chip-color" style:background="var({token})"></span>
          <code>{token}</code>
        </div>
      {/each}
    </div>
    <p class="note">
      Charts get their own ordered ramp: <code>--chart-1</code> … <code>--chart-8</code>.
      Assigned by entity in fixed order, never cycled — a 9th series folds into "Other".
      Both themes pass the OKLCH lightness band, chroma floor, adjacent-pair CVD
      separation, and 3:1 contrast against their surface.
    </p>
    <div class="swatches">
      {#each [1, 2, 3, 4, 5, 6, 7, 8] as n (n)}
        <div class="swatch">
          <span class="chip-color" style:background="var(--chart-{n})"></span>
          <code>{n}</code>
        </div>
      {/each}
    </div>
  </Card>

  <!-- ── Buttons ─────────────────────────────────────────────────────────── -->
  <Card title="Button" subtitle="Six states. Tab through them — the focus ring is the tell.">
    <div class="grid-label">
      <span></span><span class="hdr">default</span><span class="hdr">with icon</span>
      <span class="hdr">disabled</span><span class="hdr">loading</span><span class="hdr">sm</span>

      {#each VARIANTS as v (v)}
        <span class="rowlabel">{v}</span>
        <Button variant={v}>Save</Button>
        <Button variant={v} icon="check">Save</Button>
        <Button variant={v} disabled>Save</Button>
        <Button variant={v} loading>Save</Button>
        <Button variant={v} size="sm" icon="plus">New</Button>
      {/each}
    </div>
  </Card>

  <!-- ── Form controls ───────────────────────────────────────────────────── -->
  <Card title="Input & Select" subtitle="Error colors the border and the hint. Never a red fill.">
    <div class="form">
      <Input label="Symbol" bind:value={text} placeholder="AAPL" />
      <Input label="Amount" type="number" bind:value={amount} hint="Figures align: tabular." />
      <Select
        label="Currency"
        bind:value={ccy}
        options={[
          { value: 'EUR', label: 'Euro' },
          { value: 'USD', label: 'US Dollar' },
          { value: 'GBP', label: 'Pound' }
        ]}
      />
      <Input label="Email" type="email" bind:value={bad} error="Enter a valid email address." required />
      <Input label="Disabled" value="Read only" disabled />
      <Input label="Notes" multiline rows={2} placeholder="Free text…" />
    </div>
  </Card>

  <!-- ── IndicatorPicker ─────────────────────────────────────────────────── -->
  <Card title="IndicatorPicker" subtitle="Grouped, searchable replacement for a long <select>. Height-capped, sticky group headers, keyboard nav.">
    <div class="form">
      <div style="display:flex; flex-direction:column; gap:6px; max-width:220px;">
        <span style="font-size:var(--text-xs); color:var(--muted); text-transform:uppercase; letter-spacing:0.06em;">Indicator</span>
        <IndicatorPicker bind:value={pickIndicator} groups={demoPickerGroups} ariaLabel="Pick an indicator" />
        <span style="font-size:var(--text-xs); color:var(--muted); font-family:var(--mono,monospace);">selected: {pickIndicator}</span>
      </div>
    </div>
  </Card>

  <!-- ── Stats ───────────────────────────────────────────────────────────── -->
  <Card title="StatCard" subtitle="The sign is a glyph and an arrow, never color alone.">
    <div class="stats">
      <StatCard label="Net PnL" value="{fmt(1240.5)} €" delta={3.2} />
      <StatCard label="Drawdown" value="−{fmt(318.2)} €" delta={-1.8} />
      <StatCard label="Win rate" value="62%" delta={0} />
      <StatCard label="Open positions" value="7" hint="across 3 accounts" />
    </div>
  </Card>

  <!-- ── Badges ──────────────────────────────────────────────────────────── -->
  <Card title="Badge" subtitle="Tone is semantic — pages never name a color.">
    <div class="row">
      <Badge>neutral</Badge>
      <Badge tone="success" icon="check">success</Badge>
      <Badge tone="warn" icon="alert-triangle">warn</Badge>
      <Badge tone="danger" icon="x">danger</Badge>
      <Badge tone="accent">accent</Badge>
    </div>
  </Card>

  <!-- ── Table ───────────────────────────────────────────────────────────── -->
  <Card title="Table" subtitle="Row height from --row-h. Borders, not stripes. Sort a header." padded={false}>
    <Table {columns} {rows} {sort} onsort={(s) => (sort = s)}>
      {#snippet cell(row, col)}
        {#if col.key === 'pnl'}
          <span style:color={row.pnl >= 0 ? 'var(--green)' : 'var(--red)'}>
            {sign(row.pnl)}{fmt(Math.abs(row.pnl))}
          </span>
        {:else if col.key === 'status'}
          <Badge tone={tone(row.status)}>{row.status}</Badge>
        {:else if col.numeric}
          {fmt(row[col.key])}
        {:else}
          {row[col.key]}
        {/if}
      {/snippet}
    </Table>
  </Card>

  <Card title="Table — empty" padded={false}>
    <Table {columns} rows={[]}>
      {#snippet empty()}
        <EmptyState
          icon="candlestick"
          title="No trades yet"
          description="Import a statement or log your first trade to see it here."
          compact
        >
          {#snippet action()}
            <Button variant="primary" icon="plus">New trade</Button>
          {/snippet}
        </EmptyState>
      {/snippet}
    </Table>
  </Card>

  <!-- ── Empty & loading ─────────────────────────────────────────────────── -->
  <div class="two">
    <Card title="EmptyState" subtitle="What belongs here, why it isn't, and the fix.">
      <EmptyState
        icon="newspaper"
        title="No feeds configured"
        description="Add an RSS source and the scheduler will start pulling articles every hour."
      >
        {#snippet action()}
          <Button variant="primary" icon="plus">Add a feed</Button>
        {/snippet}
      </EmptyState>
    </Card>

    <Card title="Skeleton" subtitle="Holds the shape. Never a spinner where a table was.">
      <div class="stack">
        <div class="row">
          <Skeleton circle size="36px" />
          <div style:flex="1"><Skeleton rows={2} height="0.8rem" /></div>
        </div>
        <Skeleton rows={4} height="var(--row-h)" gap="1px" />
      </div>
    </Card>
  </div>

  <!-- ── Overlays ────────────────────────────────────────────────────────── -->
  <Card title="Modal & Tabs" subtitle="Level 2 floats: shadow, no border.">
    <div class="stack">
      <Tabs
        tabs={[
          { id: 'one', label: 'Trades' },
          { id: 'two', label: 'Strategies' },
          { id: 'three', label: 'Templates' }
        ]}
        bind:value={tab}
      />
      <p class="note">Active tab: <code>{tab}</code> — arrow keys move focus.</p>
      <div><Button variant="primary" onclick={() => (modalOpen = true)}>Open modal</Button></div>
    </div>
  </Card>

  <Modal bind:open={modalOpen} title="Close this position?" size="sm">
    <p style:margin="0" style:color="var(--muted)" style:font-size="var(--text-sm)">
      This books the PnL at the current mark and cannot be undone.
    </p>
    {#snippet footer()}
      <Button onclick={() => (modalOpen = false)}>Cancel</Button>
      <Button variant="danger" onclick={() => (modalOpen = false)}>Close position</Button>
    {/snippet}
  </Modal>

  <!-- ── Elevation ───────────────────────────────────────────────────────── -->
  <Card title="Elevation" subtitle="Border OR shadow. Both together is the tell of a template.">
    <div class="row">
      <div class="elev-1 demo-box">level 1 — border, no shadow</div>
      <div class="elev-2 demo-box">level 2 — shadow, no border</div>
    </div>
  </Card>
</div>

<style>
  .gallery {
    max-width: 1100px;
    margin: 0 auto;
    padding: var(--space-8) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .stack p {
    margin: 0;
    color: var(--text);
    line-height: var(--lh-tight);
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .two {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
    gap: var(--space-6);
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-4);
  }

  .form {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--space-4);
  }

  /* Button matrix: variant per row, state per column. */
  .grid-label {
    display: grid;
    grid-template-columns: auto repeat(5, max-content);
    align-items: center;
    gap: var(--space-3) var(--space-4);
    overflow-x: auto;
  }
  .hdr,
  .rowlabel {
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
  }
  .rowlabel {
    text-align: right;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
  }
  .swatch {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .chip-color {
    width: 22px;
    height: 22px;
    border-radius: 6px;
    border: 1px solid var(--border);
  }

  .note {
    margin: var(--space-3) 0 0;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  code {
    font-size: var(--text-xs);
    color: var(--muted);
  }

  .demo-box {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--space-6);
    font-size: var(--text-sm);
    color: var(--muted);
  }
</style>

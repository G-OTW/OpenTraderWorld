<script>
  // Instruments a broker sync held back. A fill says buy or sell, never open or close, so
  // when the fills of an instrument do not add up to what the account holds (or a cash
  // account sells what the window never bought), the sync asks instead of guessing. Each
  // answer is kept on the journal's sync and replayed by every later pull:
  //   - a position opened before: its price (and date) stand in for the missing history;
  //   - the fills are right: the instrument folds as it is from now on;
  //   - leave it out: the instrument is skipped by every later sync.
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import { journalApi } from './api.js';
  import { t, locale } from '$lib/i18n';

  let { conflicts = [], categories = [], onchanged = () => {} } = $props();

  let openId = $state(null);
  let price = $state('');
  let entryDay = $state('');
  let busy = $state('');
  let error = $state('');

  const fmtQty = (n) => String(Number(Math.abs(Number(n)).toFixed(8)));
  const fmtDay = (iso) =>
    iso
      ? new Date(iso).toLocaleDateString($locale, { day: '2-digit', month: 'short', year: 'numeric' })
      : '';
  const bookName = (id) => categories.find((c) => c.id === id)?.name ?? '';

  function describe(c) {
    const d = c.details ?? {};
    const side = d.qty < 0 ? $t('journal.side.short') : $t('journal.side.long');
    return c.kind === 'sell_without_open'
      ? $t('journal.brokerConflicts.sellWithoutOpen', { qty: fmtQty(d.qty), symbol: c.symbol })
      : $t('journal.brokerConflicts.unexplained', {
          qty: fmtQty(d.qty),
          side,
          symbol: c.symbol,
          day: fmtDay(d.window_from)
        });
  }

  function toggle(c) {
    error = '';
    if (openId === c.id) {
      openId = null;
      return;
    }
    openId = c.id;
    price = c.details?.last_price ? String(c.details.last_price) : '';
    entryDay = '';
  }

  async function answer(c, action) {
    error = '';
    busy = c.id;
    try {
      const body = { action };
      if (action === 'closing') {
        body.price = Number(price);
        if (entryDay) body.entry_at = `${entryDay}T00:00:00Z`;
      }
      await journalApi.brokerAnswer(c.id, body);
      openId = null;
      onchanged();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }
</script>

{#if conflicts.length}
  <ul class="list">
    {#each conflicts as c (c.id)}
      <li class="item">
        <div class="head">
          <Icon name="alert-triangle" size={13} />
          <div class="txt">
            <span class="what">{describe(c)}</span>
            <span class="meta">
              {#if bookName(c.target_id)}{bookName(c.target_id)} · {/if}
              {$t('journal.brokerConflicts.fills', { n: c.details?.fills ?? 0 })}
            </span>
          </div>
          <Button variant="ghost" onclick={() => toggle(c)}>
            {openId === c.id ? $t('common.close') : $t('journal.brokerConflicts.answer')}
          </Button>
        </div>
        {#if openId === c.id}
          <div class="body">
            <div class="choice">
              <p class="q">{$t('journal.brokerConflicts.closingQ')}</p>
              <div class="row">
                <label class="fld">
                  <span>{$t('journal.brokerConflicts.price', { ccy: c.details?.currency ?? '' })}</span>
                  <input type="number" step="any" min="0" bind:value={price} />
                </label>
                <label class="fld">
                  <span>{$t('journal.brokerConflicts.openedOn')}</span>
                  <input type="date" bind:value={entryDay} />
                </label>
                <Button
                  variant="primary"
                  disabled={busy === c.id || !(Number(price) > 0)}
                  onclick={() => answer(c, 'closing')}
                >
                  {$t('journal.brokerConflicts.closing')}
                </Button>
              </div>
              <span class="help">{$t('journal.brokerConflicts.closingHelp')}</span>
            </div>
            {#if c.kind === 'unexplained_position'}
              <div class="choice">
                <p class="q">{$t('journal.brokerConflicts.openingQ')}</p>
                <div class="row">
                  <Button disabled={busy === c.id} onclick={() => answer(c, 'opening')}>
                    {$t('journal.brokerConflicts.opening')}
                  </Button>
                </div>
              </div>
            {/if}
            <div class="choice">
              <div class="row">
                <Button variant="ghost" disabled={busy === c.id} onclick={() => answer(c, 'ignore')}>
                  {$t('journal.brokerConflicts.ignore')}
                </Button>
              </div>
            </div>
          </div>
        {/if}
      </li>
    {/each}
  </ul>
  <ErrorText {error} />
{/if}

<style>
  .list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .item {
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--amber);
  }
  .head :global(svg) {
    flex: none;
  }
  .txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .what {
    color: var(--text);
    font-size: var(--text-sm);
    line-height: 1.4;
  }
  .meta {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3) 0 var(--space-1);
  }
  .choice {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .q {
    font-size: var(--text-sm);
    color: var(--text);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .fld input {
    width: 160px;
  }
  .help {
    font-size: 11px;
    color: var(--muted);
    line-height: 1.4;
  }
</style>

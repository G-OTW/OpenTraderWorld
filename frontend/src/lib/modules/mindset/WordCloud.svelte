<script>
  // Two clouds side by side: the words that repeat on good days and on bad days, sized by
  // how many days each was written on. `good` / `bad` = [{ word, n }].
  import { t } from '$lib/i18n';

  let { good = [], bad = [], goodLabel = '', badLabel = '', docs = 0, minDocs = 4 } = $props();

  const maxN = $derived(Math.max(1, ...good.map((w) => w.n), ...bad.map((w) => w.n)));
  // sqrt so a word on 16 days is twice the size of one on 4, not four times.
  const size = (n) => `${(0.8 + 1.2 * Math.sqrt((n - 1) / Math.max(1, maxN - 1))).toFixed(2)}rem`;
</script>

{#if docs < minDocs || (good.length === 0 && bad.length === 0)}
  <p class="muted">{$t('mindset.history.cloudNone')}</p>
{:else}
  <div class="cols">
    {#each [['bad', bad, badLabel], ['good', good, goodLabel]] as [k, list, label] (k)}
      <div class="col">
        <span class="lbl {k}">{label}</span>
        {#if list.length === 0}
          <p class="muted">{$t('mindset.history.none')}</p>
        {:else}
          <p class="cloud">
            {#each list as w (w.word)}
              <span class="w" style:font-size={size(w.n)} title={$t('mindset.history.cloudDays', { n: w.n })}>{w.word}</span>
            {/each}
          </p>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  .cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: var(--space-4);
  }
  .col {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 0;
  }
  .lbl {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: var(--fw-medium);
  }
  .lbl.bad {
    color: var(--red);
  }
  .lbl.good {
    color: var(--green);
  }
  .cloud {
    margin: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-3);
    line-height: 1.15;
  }
  .w {
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-sm);
  }
</style>

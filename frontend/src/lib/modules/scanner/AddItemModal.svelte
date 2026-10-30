<script>
  // Add items with the chart's own instrument picker, on the connectors granted to the
  // Scanner. The symbol is the provider's own, as the lookup returned it: nothing typed is
  // ever guessed into an instrument. A connector that could not answer says why.
  import Modal from '$lib/ui/Modal.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import DataTab from '$lib/modules/histviz/DataTab.svelte';
  import { scannerApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), hives = [], onchanged } = $props();

  let error = $state('');
  let timeframe = $state('1h');
  let hiveId = $state('');
  let added = $state([]); // tickers added while the modal is open, newest first
  let busy = $state(false);

  $effect(() => {
    if (!open) {
      added = [];
      error = '';
    }
  });

  async function add(h) {
    if (!h.timeframes?.includes(timeframe)) {
      error = $t('scanner.add.noTimeframe', { tf: timeframe, connector: h.provider });
      return;
    }
    busy = true;
    try {
      await scannerApi.createItem({
        provider: h.provider,
        asset_type: h.asset_type,
        ticker: h.ticker,
        timeframe,
        connector_id: h.connector_id,
        label: h.name ?? '',
        hive_ids: hiveId ? [hiveId] : []
      });
      added = [`${h.ticker} ${timeframe}`, ...added.filter((x) => x !== `${h.ticker} ${timeframe}`)];
      error = '';
      onchanged?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  const TIMEFRAMES = ['1m', '5m', '15m', '30m', '1h', '4h', '1d', '1w'];
</script>

<Modal bind:open title={$t('scanner.add.title')} size="lg">
  <div class="form">
    <div class="row">
      <div class="tf">
        <Dropdown bind:value={timeframe} ariaLabel={$t('scanner.timeframe')} options={TIMEFRAMES.map((v) => ({ value: v, label: v }))} />
      </div>
      <div class="hv">
        <Dropdown
          bind:value={hiveId}
          ariaLabel={$t('scanner.add.joinHive')}
          options={[{ value: '', label: $t('scanner.add.noHive') }, ...hives.map((h) => ({ value: h.id, label: h.name }))]}
        />
      </div>
    </div>
    {#if error}<ErrorText {error} />{/if}
    {#if added.length}
      <p class="ok">{$t('scanner.add.added')}: {added.join(', ')}</p>
    {/if}
    <div class="pick">
      <DataTab module="scanner" shelves={false} {busy} onselect={add} />
    </div>
  </div>
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .row {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .tf {
    width: 90px;
  }
  .hv {
    min-width: 150px;
  }
  .pick {
    display: flex;
    flex-direction: column;
    min-height: 360px;
  }
  .ok {
    margin: 0;
    color: var(--green);
    font-size: var(--text-xs);
  }
</style>

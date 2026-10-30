<script>
  // One scanner alert: where it scans (an item or a hive), which signals must all hold on the
  // same closed bar (OR is two alerts), and how to be told. It only notifies.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { scannerApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), alert = null, items = [], hives = [], signals = [], onsaved } = $props();

  let name = $state('');
  let targetKind = $state('hive');
  let targetId = $state('');
  let picked = $state([]);
  let notify = $state('digest');
  let cooldownMin = $state(0);
  let channels = $state([]);
  let chosen = $state([]);
  let enabled = $state(true);
  let error = $state('');
  let saving = $state(false);

  let seeded = false;
  $effect(() => {
    if (!open) {
      seeded = false;
      return;
    }
    if (seeded) return;
    seeded = true;
    name = alert?.name ?? '';
    targetKind = alert?.target_kind ?? (hives.length ? 'hive' : 'item');
    targetId = alert?.target_id ?? (targetKind === 'hive' ? hives[0]?.id : items[0]?.id) ?? '';
    picked = [...(alert?.signal_ids ?? [])];
    notify = alert?.notify ?? 'digest';
    cooldownMin = Math.round((alert?.cooldown_secs ?? 0) / 60);
    chosen = Array.isArray(alert?.channels) ? [...alert.channels] : [];
    enabled = alert?.enabled ?? true;
    error = '';
    loadChannels();
  });

  function loadChannels() {
    channelsApi
      .list('scanner')
      .then((rows) => (channels = rows))
      .catch(() => {
        /* no channel granted: the in-app notification still arrives */
      });
  }

  function setKind(k) {
    targetKind = k;
    targetId = (k === 'hive' ? hives[0]?.id : items[0]?.id) ?? '';
  }

  const toggle = (list, id) => (list.includes(id) ? list.filter((x) => x !== id) : [...list, id]);

  async function save() {
    saving = true;
    const body = {
      name,
      enabled,
      target_kind: targetKind,
      target_id: targetId,
      signal_ids: picked,
      notify,
      channels: chosen,
      cooldown_secs: Math.max(0, Math.round(Number(cooldownMin) * 60))
    };
    try {
      if (alert) await scannerApi.saveAlert(alert.id, body);
      else await scannerApi.createAlert(body);
      open = false;
      onsaved?.();
    } catch (e) {
      error = e.message;
    } finally {
      saving = false;
    }
  }

  const signalOptions = $derived(signals.map((s) => ({ value: s.id, label: s.name })));

  const targets = $derived(
    targetKind === 'hive'
      ? hives.map((h) => ({ value: h.id, label: `${h.name} (${h.item_ids.length})` }))
      : items.map((i) => ({ value: i.id, label: `${i.ticker} ${i.timeframe}` }))
  );
</script>

<Modal bind:open size="md" title={alert ? $t('scanner.alerts.edit') : $t('scanner.alerts.new')}>
  <div class="form">
    <Input label={$t('scanner.name')} placeholder={$t('scanner.alerts.namePh')} bind:value={name} />

    <div class="form-grid">
      <label class="field">
        <span>{$t('scanner.alerts.scans')}</span>
        <Dropdown
          value={targetKind}
          float
          onpick={setKind}
          options={[
            { value: 'hive', label: $t('scanner.alerts.aHive') },
            { value: 'item', label: $t('scanner.alerts.anItem') }
          ]}
        />
      </label>
      <label class="field">
        <span>{targetKind === 'hive' ? $t('scanner.alerts.hive') : $t('scanner.alerts.item')}</span>
        <Dropdown bind:value={targetId} float searchable={targets.length > 8} options={targets} placeholder={$t('scanner.alerts.pick')} />
      </label>
    </div>

    <div class="field">
      <span>{$t('scanner.alerts.signals')}</span>
      {#if signals.length}
        <div class="sigs">
          <MultiSelect bind:value={picked} options={signalOptions} allLabel={$t('scanner.alerts.pick')} width="100%" />
        </div>
        <span class="hint">{$t('scanner.alerts.andHint')}</span>
      {:else}
        <span class="hint">{$t('scanner.alerts.noSignals')}</span>
      {/if}
    </div>

    <div class="form-grid">
      <label class="field">
        <span>{$t('scanner.alerts.notify')}</span>
        <Dropdown
          bind:value={notify}
          float
          options={[
            { value: 'digest', label: $t('scanner.alerts.digest') },
            { value: 'per_hit', label: $t('scanner.alerts.perHit') }
          ]}
        />
      </label>
      <Input type="number" label={$t('scanner.alerts.cooldown')} hint={$t('scanner.alerts.cooldownHint')} bind:value={cooldownMin} />
    </div>

    <div class="field">
      <span>{$t('scanner.alerts.channels')}</span>
      <div class="chips">
        {#each channels as c (c.id)}
          <button type="button" class="chip" class:active={chosen.includes(c.id)} aria-pressed={chosen.includes(c.id)} onclick={() => (chosen = toggle(chosen, c.id))}>
            {c.name}
          </button>
        {/each}
        <ChannelButton module="scanner" size="sm" onchanged={loadChannels} />
      </div>
      <span class="hint">{$t('scanner.alerts.channelsHint')}</span>
    </div>

    <label class="check">
      <input type="checkbox" bind:checked={enabled} />
      {$t('scanner.alerts.enabled')}
    </label>
    {#if alert?.last_error}
      <p class="err">{alert.last_error}</p>
    {/if}
    {#if error}<ErrorText {error} />{/if}
  </div>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>{$t('scanner.cancel')}</Button>
    <Button variant="primary" loading={saving} disabled={!targetId || !picked.length} onclick={save}>{$t('scanner.save')}</Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    align-items: center;
  }
  /* Same frame as the Dropdowns above it. */
  .sigs :global(.ms-toggle) {
    height: var(--control-h);
    padding: 0 var(--space-3);
    background: var(--surface);
    border-color: var(--border-control);
    border-radius: var(--radius-sm);
    font-size: var(--fs-body);
  }
  .check {
    display: flex;
    gap: var(--space-2);
    align-items: center;
    font-size: var(--text-sm);
  }
  .err {
    color: var(--red);
    font-size: var(--text-xs);
  }
</style>

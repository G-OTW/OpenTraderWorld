<script>
  // List settings: the daily movers digest. Once a day, at or after the chosen hour (UTC),
  // the three biggest gainers and losers over 24h go to the picked channels (none picked =
  // every channel granted to Watchlists). Built from cached quotes: no extra request.
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Button from '$lib/ui/Button.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';
  import { watchlistsApi } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), wl = null, onsaved = () => {} } = $props();

  let digest = $state('off');
  let hour = $state(21);
  let picked = $state([]);
  let channels = $state([]);
  let busy = $state(false);
  let error = $state('');

  function loadChannels() {
    channelsApi
      .list('watchlists')
      .then((c) => (channels = (c ?? []).filter((x) => x.enabled)))
      .catch(() => {});
  }
  $effect(() => {
    if (!open || !wl) return;
    digest = wl.digest ?? 'off';
    hour = wl.digest_hour ?? 21;
    picked = [...(wl.digest_channels ?? [])];
    error = '';
    loadChannels();
  });

  const toggle = (id) =>
    (picked = picked.includes(id) ? picked.filter((c) => c !== id) : [...picked, id]);

  async function save() {
    busy = true;
    error = '';
    try {
      const next = await watchlistsApi.update(wl.id, {
        digest,
        digest_hour: Number(hour),
        digest_channels: picked
      });
      open = false;
      onsaved(next);
    } catch (e) {
      error = e.message;
    } finally {
      busy = false;
    }
  }

  const HOURS = Array.from({ length: 24 }, (_, h) => ({ value: h, label: `${String(h).padStart(2, '0')}:00 UTC` }));
</script>

<Modal bind:open size="sm" title={$t('watchlists.settings.title')}>
  <div class="form">
    <label class="opt">
      <input type="checkbox" checked={digest === 'daily'} onchange={(e) => (digest = e.currentTarget.checked ? 'daily' : 'off')} />
      <span>
        {$t('watchlists.settings.digest')}
        <em>{$t('watchlists.settings.digestHint')}</em>
      </span>
    </label>
    {#if digest === 'daily'}
      <div class="group">
        <div class="field">
          <span class="lbl">{$t('watchlists.settings.hour')}</span>
          <div class="dd">
            <Dropdown bind:value={hour} ariaLabel={$t('watchlists.settings.hour')} options={HOURS} />
          </div>
        </div>
        <div class="field">
          <div class="lblrow">
            <span class="lbl">{$t('watchlists.alerts.sendTo')}</span>
            <ChannelButton module="watchlists" size="sm" onchanged={loadChannels} />
          </div>
          {#if channels.length === 0}
            <p class="none">{$t('watchlists.alerts.noChannels')}</p>
          {:else}
            <div class="chans">
              {#each channels as c (c.id)}
                <button type="button" class="chip" class:active={picked.includes(c.id)} aria-pressed={picked.includes(c.id)} onclick={() => toggle(c.id)}>
                  <Icon name={picked.includes(c.id) ? 'check-square' : 'square'} size={12} />
                  {c.name || c.kind}
                </button>
              {/each}
            </div>
            <p class="none">
              {picked.length === 0 ? $t('watchlists.alerts.allChannelsHint') : $t('watchlists.alerts.someChannelsHint')}
            </p>
          {/if}
        </div>
      </div>
    {/if}
    <ErrorText {error} copyable />
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={save} loading={busy}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .opt {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
    font-size: 13px;
    cursor: pointer;
    text-transform: none;
    letter-spacing: normal;
    color: var(--text);
  }
  .opt em {
    display: block;
    margin-top: 2px;
    font-style: normal;
    color: var(--dim);
    font-size: 12px;
    line-height: 1.4;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .lblrow {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  /* Same label as the shared Field. */
  .lbl {
    font-size: 12px;
    font-weight: 500;
    color: var(--muted);
    line-height: var(--lh-tight);
  }
  .dd {
    max-width: 200px;
  }
  .chans {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .none {
    font-size: 12px;
    color: var(--dim);
    margin: 0;
  }
</style>

<script>
  // Add/edit a subscription. Platform, category and payment method autocomplete from
  // existing values. Reminders are off by default; a channel list that stays empty sends
  // to every channel granted to Subscriptions.
  import { CURRENCIES, FREQUENCIES } from './api.js';
  import { untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import Seg from './Seg.svelte';
  import MultiSelect from '$lib/ui/MultiSelect.svelte';
  import ChannelButton from '$lib/notifications/ChannelButton.svelte';
  import { channelsApi } from '$lib/notifications/api.js';

  let {
    initial = null,
    suggestions = { platforms: [], categories: [], payment_methods: [] },
    // New subscriptions default to the module's display currency; still editable, and an
    // existing sub always keeps its own stored currency (it's the FX reference).
    defaultCurrency = 'USD',
    onsubmit = () => {},
    oncancel = () => {}
  } = $props();

  // FREQUENCIES labels come from api.js (not localized there); map id -> key here for display.
  const FREQUENCY_KEYS = {
    weekly: 'subscriptions.form.freqWeekly',
    monthly: 'subscriptions.form.freqMonthly',
    quarterly: 'subscriptions.form.freqQuarterly',
    yearly: 'subscriptions.form.freqYearly'
  };

  let s = $state(blank());
  let error = $state('');
  // The fold opens by itself when the row already uses one of its fields.
  let advanced = $state(untrack(() => (s.yearly_price !== null && s.yearly_price !== undefined) || Number(s.my_share_pct) < 100));

  function blank() {
    const base = {
      name: '',
      platform: '',
      url: '',
      price: null,
      currency: defaultCurrency,
      frequency: 'monthly',
      category: '',
      started_on: '',
      active: true,
      remind: false,
      remind_days: 3,
      remind_channels: [],
      trial: false,
      trial_end: '',
      ended_at: '',
      yearly_price: null,
      payment_method: '',
      card_expiry: '',
      my_share_pct: 100
    };
    if (initial) {
      return {
        ...base,
        ...initial,
        platform: initial.platform ?? '',
        url: initial.url ?? '',
        category: initial.category ?? '',
        started_on: initial.started_on ?? '',
        remind: initial.remind_days !== null && initial.remind_days !== undefined,
        remind_days: initial.remind_days ?? 3,
        remind_channels: initial.remind_channels ?? [],
        trial: !!initial.trial_end,
        trial_end: initial.trial_end ?? '',
        ended_at: initial.ended_at ?? '',
        yearly_price: initial.yearly_price ?? null,
        payment_method: initial.payment_method ?? '',
        card_expiry: initial.card_expiry ?? '',
        my_share_pct: initial.my_share_pct ?? 100
      };
    }
    return base;
  }

  let channels = $state([]);
  async function loadChannels() {
    try {
      channels = await channelsApi.list('subscriptions');
    } catch {
      channels = [];
    }
  }
  $effect(() => {
    if (s.remind) loadChannels();
  });
  const channelOptions = $derived(channels.map((c) => ({ value: c.id, label: `${c.name} (${c.kind})` })));

  function num(v) {
    return v === '' || v === null || v === undefined ? 0 : Number(v);
  }
  function optNum(v) {
    return v === '' || v === null || v === undefined ? null : Number(v);
  }

  function submit() {
    error = '';
    if (s.remind && !s.started_on && !(s.trial && s.trial_end)) {
      error = $t('subscriptions.form.remindNeedsDate');
      return;
    }
    onsubmit({
      name: s.name,
      platform: s.platform || null,
      url: s.url || null,
      price: num(s.price),
      currency: s.currency,
      frequency: s.frequency,
      category: s.category || null,
      started_on: s.started_on || null,
      active: !!s.active,
      remind_days: s.remind ? Math.max(0, Math.min(60, Math.round(num(s.remind_days)))) : null,
      remind_channels: s.remind ? s.remind_channels : [],
      trial_end: s.trial && s.trial_end ? s.trial_end : null,
      ended_at: s.ended_at || null,
      yearly_price: s.frequency === 'yearly' ? null : optNum(s.yearly_price),
      payment_method: s.payment_method || null,
      card_expiry: s.card_expiry || null,
      my_share_pct: optNum(s.my_share_pct) ?? 100
    });
  }
</script>

<form
  class="sub-form"
  onsubmit={(e) => {
    e.preventDefault();
    submit();
  }}
>
  <datalist id="sub-platforms">
    {#each suggestions.platforms as v}<option value={v}></option>{/each}
  </datalist>
  <datalist id="sub-categories">
    {#each suggestions.categories as v}<option value={v}></option>{/each}
  </datalist>
  <datalist id="sub-payment-methods">
    {#each suggestions.payment_methods ?? [] as v}<option value={v}></option>{/each}
  </datalist>

  <div class="grid">
    <label class="field span">
      <span>{$t('subscriptions.form.name')}</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={s.name} autofocus placeholder={$t('subscriptions.form.namePlaceholder')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.platform')}</span>
      <input bind:value={s.platform} list="sub-platforms" autocomplete="off" placeholder={$t('subscriptions.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.category')}</span>
      <input bind:value={s.category} list="sub-categories" autocomplete="off" placeholder={$t('subscriptions.form.optional')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.price')}</span>
      <input class="num-in" type="number" step="any" min="0" bind:value={s.price} />
    </label>
    <div class="field">
      <span>{$t('subscriptions.form.currency')}</span>
      <Dropdown
        bind:value={s.currency}
        ariaLabel={$t('subscriptions.form.currency')}
        options={CURRENCIES.map((c) => ({ value: c, label: c }))}
      />
    </div>
    <div class="field span">
      <span>{$t('subscriptions.form.frequency')}</span>
      <Seg
        size="md"
        bind:value={s.frequency}
        ariaLabel={$t('subscriptions.form.frequency')}
        options={FREQUENCIES.map((f) => ({ value: f.id, label: $t(FREQUENCY_KEYS[f.id] ?? f.label) }))}
      />
    </div>
    <label class="field">
      <span>{$t('subscriptions.form.startedOn')}</span>
      <input type="date" bind:value={s.started_on} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.paymentMethod')}</span>
      <input bind:value={s.payment_method} list="sub-payment-methods" autocomplete="off" placeholder={$t('subscriptions.form.paymentPlaceholder')} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.cardExpiry')}</span>
      <input type="month" bind:value={s.card_expiry} />
    </label>
    <label class="field">
      <span>{$t('subscriptions.form.url')}</span>
      <input bind:value={s.url} placeholder={$t('subscriptions.form.optional')} />
    </label>
  </div>

  <div class="toggles">
  <div class="row">
    <label class="check">
      <input type="checkbox" bind:checked={s.trial} />
      <span>{$t('subscriptions.form.freeTrial')}</span>
    </label>
    {#if s.trial}
      <label class="field inline">
        <span>{$t('subscriptions.form.trialEnd')}</span>
        <input type="date" bind:value={s.trial_end} required />
      </label>
    {/if}
  </div>

  <div class="row">
    <label class="check">
      <input type="checkbox" bind:checked={s.remind} />
      <span>{$t('subscriptions.form.remind')}</span>
    </label>
    {#if s.remind}
      <label class="field inline days">
        <span>{$t('subscriptions.form.remindDays')}</span>
        <input class="num-in" type="number" min="0" max="60" step="1" bind:value={s.remind_days} />
      </label>
      <div class="field inline grow">
        <span>{$t('subscriptions.form.channels')}</span>
        <MultiSelect
          bind:value={s.remind_channels}
          options={channelOptions}
          allLabel={$t('subscriptions.form.allChannels')}
          width="100%"
        />
      </div>
      <ChannelButton module="subscriptions" size="sm" onchanged={loadChannels} />
    {/if}
  </div>

  {#if initial?.ended_at || s.ended_at}
    <label class="field inline">
      <span>{$t('subscriptions.form.endedAt')}</span>
      <input type="date" bind:value={s.ended_at} />
    </label>
  {/if}

  <label class="check">
    <input type="checkbox" bind:checked={s.active} />
    <span>{$t('subscriptions.form.active')}</span>
  </label>
  </div>

  <details class="fold" bind:open={advanced}>
    <summary>{$t('subscriptions.form.advanced')}</summary>
    <div class="grid">
      {#if s.frequency !== 'yearly'}
        <label class="field">
          <span>{$t('subscriptions.form.yearlyPrice')}</span>
          <input class="num-in" type="number" step="any" min="0" bind:value={s.yearly_price} placeholder={$t('subscriptions.form.optional')} />
        </label>
      {/if}
      <label class="field">
        <span>{$t('subscriptions.form.myShare')}</span>
        <input class="num-in" type="number" step="any" min="1" max="100" bind:value={s.my_share_pct} />
      </label>
    </div>
  </details>

  {#if error}<p class="error">{error}</p>{/if}

  <div class="actions">
    <button type="button" class="ghost" onclick={oncancel}>{$t('common.cancel')}</button>
    <button type="submit" class="primary">{initial ? $t('common.save') : $t('subscriptions.form.addSubscription')}</button>
  </div>
</form>

<style>
  .sub-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--space-3) var(--space-4);
  }
  .field {
    min-width: 0;
  }
  .field.span {
    grid-column: 1 / -1;
  }
  .num-in {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }
  .toggles {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-3);
  }
  .field.inline {
    width: 160px;
  }
  .field.inline.days {
    width: 96px;
  }
  .field.inline.grow {
    flex: 1;
    min-width: 180px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: 13px;
    min-height: 32px;
  }
  .check input {
    width: 16px;
  }
  .fold {
    border-top: var(--hairline) solid var(--border);
    padding-top: var(--space-3);
  }
  .fold summary {
    cursor: pointer;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--dim);
    margin-bottom: var(--space-3);
  }
  .fold:not([open]) summary {
    margin-bottom: 0;
  }
  .fold summary:hover {
    color: var(--text);
  }
  .error {
    color: var(--red);
    font-size: 12px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding-top: var(--space-3);
    border-top: var(--hairline) solid var(--border);
  }

  @media (max-width: 640px) {
    .grid {
      grid-template-columns: minmax(0, 1fr);
    }
    .check {
      min-height: 44px;
    }
    .actions button {
      min-height: 44px;
    }
    .field.inline {
      width: 100%;
    }
  }
</style>

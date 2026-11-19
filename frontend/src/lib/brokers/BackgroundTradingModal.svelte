<script>
  // Background trading on one account: the signature that lets orders armed on it (a trade on
  // alert, a strategy) be sent with nobody watching. Asked once per account, from the account
  // card in Settings, Brokers, and from any dialog that arms an order on an account without it.
  //
  // What is shown is what is recorded: the terms sections as the language pack renders them,
  // and each statement's sentence as ticked, travel with the signature, so the record proves
  // what the user read, in the language they read it. With `record` set the modal shows a
  // past signature from that record, never from today's language pack, which may have changed.
  //
  // The code field appears when the trading lock is on and the account is not paper; the
  // server checks the code with the signature and records that it did.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { brokersApi } from './api.js';
  import { t, locale } from '$lib/i18n';

  let { open = $bindable(false), account = null, record = null, onsigned } = $props();

  /** The terms sections shown, in the order of Settings, Terms (same shapes). A section the
   *  language pack does not carry is left out rather than shown as raw keys. */
  const TERMS = [
    { id: 'trading', paragraphs: 1, bullets: 4 },
    { id: 'testing', paragraphs: 1, bullets: 4 },
    { id: 'warranty', paragraphs: 2 },
    { id: 'liability', paragraphs: 2 }
  ];
  /** The discharge statements, one checkbox each. The server refuses a signature missing one. */
  const STATEMENTS = ['free', 'tested', 'venue', 'unattended', 'losses', 'paper_first'];

  let info = $state(null);
  let loading = $state(false);
  let error = $state('');
  let ticked = $state({});
  let typed = $state('');
  let code = $state('');
  let signing = $state(false);

  const viewing = $derived(!!record);
  const accountId = $derived(account?.id ?? null);

  async function load(id) {
    loading = true;
    error = '';
    ticked = {};
    typed = '';
    code = '';
    try {
      info = await brokersApi.backgroundTrading(id, true);
    } catch (e) {
      info = null;
      error = e.message;
    } finally {
      loading = false;
    }
  }
  $effect(() => {
    if (open && !record && accountId) load(accountId);
  });

  /** A terms section as the current language pack renders it, or null when it has none. */
  function section(s, tr) {
    const key = (k) => `settings.terms.${s.id}.${k}`;
    const title = tr(key('title'));
    if (title === key('title')) return null;
    const lines = [
      ...Array.from({ length: s.paragraphs ?? 0 }, (_, i) => key(`p${i + 1}`)),
      ...Array.from({ length: s.bullets ?? 0 }, (_, i) => key(`b${i + 1}`))
    ]
      .map((k) => [k, tr(k)])
      .filter(([k, v]) => v !== k)
      .map(([, v]) => v);
    return { key: s.id, title, lines };
  }

  const terms = $derived(record ? (record.terms ?? []) : TERMS.map((s) => section(s, $t)).filter(Boolean));
  const statements = $derived(
    record ? (record.statements ?? []) : STATEMENTS.map((key) => ({ key, text: $t(`brokers.background.statement.${key}`) }))
  );

  const environment = $derived(record?.environment ?? info?.environment ?? null);
  const name = $derived(record?.account_name ?? account?.name ?? '');
  const brokerLabel = $derived(account?.capability?.label ?? record?.broker ?? account?.broker ?? '');
  const codeRequired = $derived(!!info?.code_required);
  const allTicked = $derived(STATEMENTS.every((k) => ticked[k]));
  const nameOk = $derived(typed.trim() !== '' && typed.trim() === (account?.name ?? '').trim());
  const ready = $derived(
    !!info && !!environment && allTicked && nameOk && (!codeRequired || code.trim().length > 0) && !signing
  );

  async function sign() {
    if (!ready || !account) return;
    signing = true;
    error = '';
    try {
      const r = await brokersApi.signBackgroundTrading(account.id, {
        statements: statements.map((s) => ({ key: s.key, text: s.text, accepted: !!ticked[s.key] })),
        terms,
        terms_version: $t('settings.terms.updated'),
        locale: $locale,
        typed: typed.trim(),
        code: code.trim()
      });
      open = false;
      onsigned?.(r);
    } catch (e) {
      error = e.message;
      code = '';
    } finally {
      signing = false;
    }
  }

  const when = (iso) => (iso ? new Date(iso).toLocaleString() : '');
</script>

<Modal
  bind:open
  size="lg"
  title={viewing
    ? $t('brokers.background.signedTitle', { name, time: when(record?.signed_at) })
    : $t('brokers.background.modalTitle', { name })}
>
  {#if !viewing && loading}
    <Skeleton height="320px" />
  {:else}
    <dl class="facts">
      <div><dt>{$t('brokers.background.account')}</dt><dd>{name}</dd></div>
      <div><dt>{$t('brokers.background.broker')}</dt><dd>{brokerLabel}</dd></div>
      <div>
        <dt>{$t('brokers.background.environment')}</dt>
        <dd class:live={environment === 'live'}>
          {environment ? $t(`brokers.background.env.${environment}`) : '–'}
        </dd>
      </div>
      {#if viewing}
        <div><dt>{$t('brokers.background.language')}</dt><dd class="mono">{record.locale}</dd></div>
        <div><dt>{$t('brokers.background.termsVersion')}</dt><dd>{record.terms_version}</dd></div>
        <div>
          <dt>{$t('brokers.background.secondFactor')}</dt>
          <dd>{record.second_factor ? $t('brokers.background.factorYes') : $t('brokers.background.factorNo')}</dd>
        </div>
        <div><dt>{$t('brokers.background.typed')}</dt><dd>{record.typed}</dd></div>
      {/if}
    </dl>

    {#if !viewing}
      <p class="intro">{$t('brokers.background.intro')}</p>
      {#if info?.environment_error}
        <p class="warn"><Icon name="alert-triangle" size={12} /> <span>{info.environment_error}</span></p>
      {/if}
    {/if}

    <section class="terms" aria-label={$t('brokers.background.termsHeading')}>
      <h4>{$t('brokers.background.termsHeading')}</h4>
      {#each terms as s (s.key)}
        <div class="clause">
          <h5>{s.title}</h5>
          {#each s.lines as line, i (i)}
            <p>{line}</p>
          {/each}
        </div>
      {/each}
    </section>

    <section class="statements">
      <h4>{$t('brokers.background.statementsHeading')}</h4>
      {#each statements as s (s.key)}
        {#if viewing}
          <p class="statement signed"><Icon name="check" size={12} /> <span>{s.text}</span></p>
        {:else}
          <label class="statement">
            <input type="checkbox" bind:checked={ticked[s.key]} />
            <span>{s.text}</span>
          </label>
        {/if}
      {/each}
    </section>

    {#if !viewing}
      <form
        class="sign"
        onsubmit={(e) => {
          e.preventDefault();
          sign();
        }}
      >
        <label class="fld">
          <span class="lbl">{$t('brokers.background.typeName', { name: account?.name ?? '' })}</span>
          <input type="text" bind:value={typed} autocomplete="off" spellcheck="false" />
        </label>
        {#if codeRequired}
          <label class="fld">
            <span class="lbl">{$t('brokers.background.code')}</span>
            <input type="text" bind:value={code} inputmode="numeric" autocomplete="one-time-code" maxlength="10" />
            <span class="help">{$t('brokers.background.codeHint')}</span>
          </label>
        {/if}
        <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
      </form>
    {/if}
  {/if}
  <ErrorText {error} />

  {#snippet footer()}
    {#if viewing}
      <Button variant="ghost" onclick={() => (open = false)}>{$t('common.close')}</Button>
    {:else}
      <Button variant="ghost" onclick={() => (open = false)}>{$t('common.cancel')}</Button>
      <Button variant="danger" icon="lock" onclick={sign} loading={signing} disabled={!ready}>
        {$t('brokers.background.sign')}
      </Button>
    {/if}
  {/snippet}
</Modal>

<style>
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    margin: 0 0 var(--space-3);
  }
  .facts dt {
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .facts dd {
    margin: 0;
    font-size: var(--text-sm);
  }
  .facts dd.live {
    color: var(--red);
  }
  .mono {
    font-family: var(--mono);
  }
  .intro {
    margin: 0 0 var(--space-3);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    margin: 0 0 var(--space-3);
    font-size: var(--text-sm);
    color: var(--amber);
  }
  .warn :global(svg) {
    flex: none;
    margin-top: 3px;
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .terms {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-top: 1px solid var(--border);
  }
  .clause h5 {
    margin: 0 0 var(--space-1);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .clause p {
    margin: 0 0 var(--space-1);
    color: var(--muted);
    font-size: var(--text-sm);
    line-height: 1.55;
  }
  .statements {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) 0;
    border-top: 1px solid var(--border);
  }
  .statement {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin: 0;
    font-size: var(--text-sm);
    line-height: 1.5;
  }
  label.statement {
    cursor: pointer;
  }
  .statement input {
    flex: none;
    margin-top: 2px;
  }
  .statement.signed :global(svg) {
    flex: none;
    margin-top: 4px;
    color: var(--green);
  }
  .sign {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--space-3) var(--space-4);
    padding-top: var(--space-3);
    border-top: 1px solid var(--border);
  }
  .fld {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
  }
  .lbl {
    font-size: var(--text-sm);
  }
  .help {
    font-size: var(--text-xs);
    color: var(--muted);
  }
</style>

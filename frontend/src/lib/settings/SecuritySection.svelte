<script>
  // Settings → Security: the second factor, the sessions this account has open, and the
  // ceiling on how long one request may run.
  //
  // Three unrelated controls share a screen because they answer the same question: what an
  // instance exposed on the internet gives away if something goes wrong. The second factor
  // is what a stolen password cannot get past, the session list is how a stolen cookie is
  // taken back, and the timeout is what stops one request holding a connection forever.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import QrCode from '$lib/ui/QrCode.svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import { t } from '$lib/i18n';

  // ── Second factor ──────────────────────────────────────────────────────────
  let totpEnabled = $state(false);
  let enrolling = $state(null); // { secret, uri } while the QR code is on screen
  let confirmCode = $state('');
  let totpBusy = $state(false);
  let totpError = $state('');

  // ── Sessions ───────────────────────────────────────────────────────────────
  let sessions = $state([]);
  let sessionsError = $state('');

  // ── Request timeout ────────────────────────────────────────────────────────
  let timeout = $state({ seconds: 120, default: 120, min: 10, max: 3600 });
  let timeoutInput = $state(120);
  let timeoutSaved = $state(false);
  let timeoutError = $state('');

  let loading = $state(true);

  onMount(reload);

  async function reload() {
    loading = true;
    try {
      const [me, s, tmo] = await Promise.all([
        settingsApi.me(),
        settingsApi.listSessions().catch((e) => {
          sessionsError = e.message;
          return { sessions: [] };
        }),
        settingsApi.getRequestTimeout()
      ]);
      totpEnabled = !!me?.totp_enabled;
      sessions = s?.sessions ?? [];
      timeout = tmo;
      timeoutInput = tmo.seconds;
    } catch (e) {
      totpError = e.message;
    } finally {
      loading = false;
    }
  }

  async function startEnroll() {
    totpError = '';
    totpBusy = true;
    try {
      enrolling = await settingsApi.totpEnroll();
      confirmCode = '';
    } catch (e) {
      totpError = e.message;
    } finally {
      totpBusy = false;
    }
  }

  async function confirmEnroll() {
    totpError = '';
    totpBusy = true;
    try {
      await settingsApi.totpConfirm(confirmCode.trim());
      enrolling = null;
      confirmCode = '';
      await reload();
    } catch (e) {
      totpError = e.message;
    } finally {
      totpBusy = false;
    }
  }

  async function disableTotp() {
    totpError = '';
    totpBusy = true;
    try {
      await settingsApi.totpDisable();
      await reload();
    } catch (e) {
      totpError = e.message;
    } finally {
      totpBusy = false;
    }
  }

  async function revoke(id) {
    sessionsError = '';
    try {
      await settingsApi.revokeSession(id);
      await reload();
    } catch (e) {
      sessionsError = e.message;
    }
  }

  async function revokeOthers() {
    sessionsError = '';
    try {
      await settingsApi.revokeOtherSessions();
      await reload();
    } catch (e) {
      sessionsError = e.message;
    }
  }

  async function saveTimeout() {
    timeoutError = '';
    timeoutSaved = false;
    try {
      const r = await settingsApi.setRequestTimeout(Number(timeoutInput));
      timeout = { ...timeout, seconds: r.seconds };
      timeoutSaved = true;
      setTimeout(() => (timeoutSaved = false), 2500);
    } catch (e) {
      timeoutError = e.message;
    }
  }

  const otherSessions = $derived(sessions.filter((s) => !s.current).length);

  /** The stored strings are RFC 3339; show them in the viewer's own locale and zone. */
  function when(value) {
    if (!value) return '—';
    const d = new Date(value);
    return Number.isNaN(d.getTime()) ? '—' : d.toLocaleString();
  }

  /** A user-agent string is long and mostly noise; name the browser and leave it there. */
  function browser(ua) {
    if (!ua) return $t('security.sessions.unknownBrowser');
    const m =
      /(Firefox|Edg|OPR|Chrome|Safari)\/[\d.]+/.exec(ua) ?? /^([A-Za-z-]+)\//.exec(ua);
    const name = m ? m[1].replace('Edg', 'Edge').replace('OPR', 'Opera') : ua.slice(0, 40);
    const os = /(Windows|Macintosh|Mac OS X|Linux|Android|iPhone|iPad)/.exec(ua);
    return os ? `${name} · ${os[1]}` : name;
  }
</script>

<div class="section">
  <!-- ── Second factor ─────────────────────────────────────────────────────── -->
  <header>
    <h2>{$t('security.totp.title')}</h2>
    <p class="lead">{$t('security.totp.lead')}</p>
  </header>

  {#if loading}
    <Skeleton rows={3} />
  {:else}
    <div class="card">
      <div class="row">
        <span class="state" class:on={totpEnabled}>
          <Icon name={totpEnabled ? 'check' : 'alert-triangle'} size={14} />
          {totpEnabled ? $t('security.totp.on') : $t('security.totp.off')}
        </span>
        {#if totpEnabled}
          <button class="ghost danger" onclick={disableTotp} disabled={totpBusy}>
            {$t('security.totp.disable')}
          </button>
        {:else if !enrolling}
          <button class="primary" onclick={startEnroll} disabled={totpBusy}>
            {$t('security.totp.enable')}
          </button>
        {/if}
      </div>

      {#if enrolling}
        <div class="enroll">
          <p>{$t('security.totp.step1')}</p>
          <!-- Three ways into the same enrolment: scan the QR, paste the URI, or type the
               secret by hand when the phone cannot reach this screen. -->
          <QrCode value={enrolling.uri} alt={$t('security.totp.qrAlt')} />
          <code class="uri">{enrolling.uri}</code>
          <p class="muted small">{$t('security.totp.secretLabel')}</p>
          <code class="secret">{enrolling.secret}</code>

          <p>{$t('security.totp.step2')}</p>
          <div class="confirm">
            <input
              bind:value={confirmCode}
              inputmode="numeric"
              maxlength="6"
              placeholder="123456"
              autocomplete="one-time-code"
            />
            <button class="primary" onclick={confirmEnroll} disabled={totpBusy || confirmCode.trim().length !== 6}>
              {$t('security.totp.confirm')}
            </button>
            <button class="ghost" onclick={() => (enrolling = null)}>{$t('common.cancel')}</button>
          </div>
          <p class="muted small">{$t('security.totp.recovery')}</p>
        </div>
      {/if}

      <ErrorText error={totpError} compact copyable />
    </div>
  {/if}

  <!-- ── Sessions ──────────────────────────────────────────────────────────── -->
  <header>
    <h2>{$t('security.sessions.title')}</h2>
    <p class="lead">{$t('security.sessions.lead')}</p>
  </header>

  <div class="card">
    {#if loading}
      <Skeleton rows={2} />
    {:else}
      <table>
        <thead>
          <tr>
            <th>{$t('security.sessions.device')}</th>
            <th>{$t('security.sessions.source')}</th>
            <th>{$t('security.sessions.lastSeen')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each sessions as s (s.id)}
            <tr>
              <td>
                {browser(s.user_agent)}
                {#if s.current}<span class="badge">{$t('security.sessions.current')}</span>{/if}
              </td>
              <td class="mono">{s.ip || '—'}</td>
              <td>{when(s.last_seen_at)}</td>
              <td class="right">
                {#if !s.current}
                  <button class="ghost danger small" onclick={() => revoke(s.id)}>
                    {$t('security.sessions.revoke')}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>

      {#if otherSessions > 0}
        <div class="row end">
          <button class="ghost danger" onclick={revokeOthers}>
            {$t('security.sessions.revokeOthers')}
          </button>
        </div>
      {/if}
    {/if}
    <ErrorText error={sessionsError} compact copyable />
  </div>

  <!-- ── Request timeout ───────────────────────────────────────────────────── -->
  <header>
    <h2>{$t('security.timeout.title')}</h2>
    <p class="lead">{$t('security.timeout.lead')}</p>
  </header>

  <div class="card">
    <div class="row">
      <input
        type="number"
        bind:value={timeoutInput}
        min={timeout.min}
        max={timeout.max}
        step="10"
      />
      <span class="muted">{$t('security.timeout.unit')}</span>
      <button class="primary" onclick={saveTimeout} disabled={Number(timeoutInput) === timeout.seconds}>
        {$t('common.save')}
      </button>
      {#if timeoutSaved}<span class="ok"><Icon name="check" size={14} /> {$t('common.saved')}</span>{/if}
    </div>
    <p class="muted small">{$t('security.timeout.note')}</p>
    <ErrorText error={timeoutError} compact copyable />
  </div>
</div>

<style>
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  header h2 {
    margin: 0;
    font-size: 15px;
  }
  .lead {
    margin: var(--space-1) 0 0;
    color: var(--muted);
    font-size: 13px;
  }
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .row.end {
    justify-content: flex-end;
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    font-size: 13px;
    color: var(--amber);
    margin-right: auto;
  }
  .state.on {
    color: var(--green);
  }
  .ok {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--green);
    font-size: 12px;
  }
  .enroll {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
  }
  .enroll p {
    margin: 0;
    font-size: 13px;
  }
  .uri,
  .secret {
    display: block;
    overflow-wrap: anywhere;
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: var(--space-2);
    font-size: 12px;
  }
  .secret {
    letter-spacing: 0.08em;
  }
  .confirm {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .confirm input,
  .row input[type='number'] {
    width: 9rem;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: var(--muted);
    font-size: 12px;
    padding-bottom: var(--space-2);
  }
  td {
    padding: var(--space-2) 0;
    border-top: 1px solid var(--border);
  }
  td.right {
    text-align: right;
  }
  .mono {
    font-variant-numeric: tabular-nums;
  }
  .badge {
    margin-left: var(--space-1);
    padding: 1px 6px;
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--muted);
    font-size: 11px;
  }
  .muted {
    color: var(--muted);
  }
  .small {
    font-size: 12px;
    margin: 0;
  }
  .danger {
    color: var(--red);
  }
  button.small {
    font-size: 12px;
  }
</style>

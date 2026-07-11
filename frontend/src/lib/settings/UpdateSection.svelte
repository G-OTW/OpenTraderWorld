<script>
  // Update the app. Pulling images and rebuilding containers needs Docker on the host,
  // which the app intentionally cannot do to itself (it has no shell or Docker access — a
  // deliberate security boundary). So we show the current version and guide the operator
  // through the host commands.
  import { onMount } from 'svelte';
  import { settingsApi } from '$lib/settings/api.js';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { t } from '$lib/i18n';

  let version = $state('');
  let loading = $state(true);
  // null while checking; then { current, latest, update_available } — latest stays null
  // when GitHub couldn't be reached (offline box), which renders as "couldn't check".
  let check = $state(null);

  onMount(async () => {
    try {
      version = await settingsApi.version();
    } finally {
      loading = false;
    }
    try {
      check = await settingsApi.updateCheck();
    } catch {
      check = { latest: null, update_available: false };
    }
  });

  // Releases are force-pushed as fresh snapshots, so `git pull` fails with "divergent
  // branches" — the checkout must be reset to the release instead (docs/guide/updating.md).
  const pull =
    'cd /path/to/OpenTraderWorld\n' +
    'git fetch origin\n' +
    'git reset --hard origin/master\n' +
    'docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \\\n' +
    '  --env-file deploy/.env --env-file deploy/network.env \\\n' +
    '  pull\n' +
    'docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.images.yml \\\n' +
    '  --env-file deploy/.env --env-file deploy/network.env \\\n' +
    '  up -d';
</script>

<div class="section">
  <h2>{$t('settings.update.title')}</h2>
  <p class="version">
    {$t('settings.update.current')}
    <strong>{loading ? '…' : `v${version}`}</strong>
  </p>

  {#if !check}
    <p class="check muted-check">{$t('settings.update.checking')}</p>
  {:else if check.update_available}
    <p class="check avail">
      <Icon name="arrow-up" size={14} />
      {$t('settings.update.available', { version: check.latest })}
    </p>
  {:else if check.latest}
    <p class="check ok">
      <Icon name="check-circle" size={14} />
      {$t('settings.update.upToDate')}
    </p>
  {:else}
    <p class="check muted-check">{$t('settings.update.checkFailed')}</p>
  {/if}

  <p class="muted">{$t('settings.update.intro')}</p>
  <CommandBlock command={pull} />

  <div class="note">
    <strong>{$t('settings.update.beforeTitle')}</strong>
    <ul>
      <li>{$t('settings.update.before1')}</li>
      <li>{$t('settings.update.before2')}</li>
      <li>{$t('settings.update.before3')}</li>
    </ul>
  </div>
</div>

<style>
  .section {
    max-width: 680px;
  }
  h2 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-md);
    color: var(--text);
  }
  .version {
    color: var(--text);
    font-size: var(--text-base);
    margin: 0 0 var(--space-2);
  }
  .muted {
    color: var(--muted);
    font-size: var(--text-base);
    line-height: 1.5;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--text-base);
    margin: 0 0 var(--space-3);
  }
  .check.ok {
    color: var(--green-ink);
  }
  .check.avail {
    color: var(--amber-ink);
    font-weight: var(--fw-semibold);
  }
  .check.muted-check {
    color: var(--muted);
  }
  .note {
    margin-top: var(--space-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .note ul {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    color: var(--muted);
  }
  .note li {
    margin: 6px 0;
  }
</style>

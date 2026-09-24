<script>
  // Backup and restore, split two ways: Full (the whole database, taken on the host with
  // pg_dump: what a dead server needs) and Partial (a per-module zip, out and back in,
  // with the app up: what moving house needs). Each scope has a Backup and a Restore tab.
  //
  // The full side is guided rather than executed: core runs distroless (no shell, no
  // pg_dump) and is deliberately kept without host or Docker access, so it hands over
  // copy-paste commands plus the security notes that matter (encryption at rest, secret-key
  // handling, session tokens).
  import { onMount } from 'svelte';
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import Tabs from '$lib/ui/Tabs.svelte';
  import FileDrop from '$lib/import/FileDrop.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { settingsApi, fmtBytes } from '$lib/settings/api.js';
  import { promptReauth } from '$lib/reauth.svelte.js';
  import { fmtNum } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let scope = $state('full'); // full | partial
  let action = $state('backup'); // backup | restore

  const scopeTabs = $derived([
    { id: 'full', label: $t('settings.backup.tabFull') },
    { id: 'partial', label: $t('settings.backup.tabPartial') }
  ]);
  const actionTabs = $derived([
    { id: 'backup', label: $t('settings.backup.tabBackup') },
    { id: 'restore', label: $t('settings.backup.restoreTitle') }
  ]);

  // ── Full ───────────────────────────────────────────────────────────────────
  // Scheduled-backup status, reported by the host-side backup script through a small status
  // file. `configured: false` (no script, or no backup yet) is the normal case for a laptop
  // install: the card stays hidden and this tab is exactly what it has always been.
  // Nothing here is a setting: there is no assisted mode to switch on, the presence of the
  // report is the signal, so a hand-run backup on a laptop lights it up just the same.
  let status = $state(null);

  onMount(async () => {
    try {
      status = await settingsApi.backupStatus();
    } catch {
      status = null; // A failed probe must never take the manual instructions down with it.
    }
  });

  const scheduled = $derived(status?.configured ? status : null);

  // Relative age in words. The script reports an absolute timestamp and core computes the
  // age, so a stopped scheduler shows an ageing value instead of a frozen "ok".
  const age = $derived.by(() => {
    const s = scheduled?.age_seconds;
    if (s == null) return '';
    if (s < 90) return $t('settings.backup.ageJustNow');
    if (s < 5400) return $t('settings.backup.ageMinutes', { n: Math.round(s / 60) });
    if (s < 172800) return $t('settings.backup.ageHours', { n: Math.round(s / 3600) });
    return $t('settings.backup.ageDays', { n: Math.round(s / 86400) });
  });

  const failed = $derived(!!scheduled && scheduled.result !== 'ok');
  const tone = $derived(!scheduled ? '' : failed ? 'bad' : scheduled.stale ? 'warn' : 'good');

  const headline = $derived.by(() => {
    if (!scheduled) return '';
    if (failed) return $t('settings.backup.scheduledFailedTitle', { age });
    if (scheduled.stale) return $t('settings.backup.scheduledStaleTitle', { age });
    return $t('settings.backup.scheduledOkTitle', { age });
  });

  function humanSize(bytes) {
    if (!Number.isFinite(bytes) || bytes <= 0) return '';
    const units = ['B', 'kB', 'MB', 'GB'];
    let v = bytes;
    let i = 0;
    while (v >= 1024 && i < units.length - 1) {
      v /= 1024;
      i += 1;
    }
    return `${v < 10 && i > 0 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
  }

  const plain =
    'docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    "  pg_dump -U otw -d opentraderworld --no-owner > otw-backup-$(date +%F).sql";

  // GPG_TTY + loopback: inside a pipeline gpg's stdin is the dump, so without them the
  // passphrase prompt fails with "Inappropriate ioctl for device".
  const encrypted =
    'export GPG_TTY=$(tty)\n' +
    'docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    '  pg_dump -U otw -d opentraderworld --no-owner \\\n' +
    '  | gpg --symmetric --cipher-algo AES256 --pinentry-mode loopback \\\n' +
    '  > otw-backup-$(date +%F).sql.gpg';

  // Core must be stopped and the database empty: core runs migrations on boot, so
  // restoring over an already-migrated schema collides (duplicate tables and
  // _sqlx_migrations rows). The dump carries the schema; an empty database is all it needs.
  const restorePlain =
    'docker compose -f deploy/docker-compose.yml stop core\n' +
    'docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    '  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"\n' +
    'docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    '  psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q < otw-backup-YYYY-MM-DD.sql\n' +
    'docker compose -f deploy/docker-compose.yml start core';

  const restoreEncrypted =
    'export GPG_TTY=$(tty)\n' +
    'docker compose -f deploy/docker-compose.yml stop core\n' +
    'docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    '  psql -U otw -d postgres -c "CREATE DATABASE opentraderworld OWNER otw"\n' +
    'gpg -d otw-backup-YYYY-MM-DD.sql.gpg \\\n' +
    '  | docker compose -f deploy/docker-compose.yml exec -T postgres \\\n' +
    '    psql -U otw -d opentraderworld -v ON_ERROR_STOP=1 -q\n' +
    'docker compose -f deploy/docker-compose.yml start core';

  // ── Partial ────────────────────────────────────────────────────────────────
  // Historical bars are the one thing left unticked by default: they are the biggest table
  // by far and can be downloaded again from the provider, so carrying them by hand is
  // usually waste. Everything else starts selected.
  const BULKY = ['histdata'];

  let usage = $state(null);
  let loadingUsage = $state(false);
  let usageError = $state('');
  let picked = $state({});
  let withCredentials = $state(false);
  let mode = $state('merge');
  let file = $state(null);
  let loadingIn = $state(false);
  let result = $state(null);
  let transferError = $state('');
  let replaceConfirm = $state('');

  const modules = $derived(
    usage ? [...usage.modules].sort((a, b) => b.size_bytes - a.size_bytes) : []
  );
  const chosen = $derived(modules.filter((m) => picked[m.id]).map((m) => m.id));
  const replacing = $derived(mode === 'replace');
  // Replace wipes before loading, so it asks for the word back, like the per-module wipe does.
  const replaceArmed = $derived(!replacing || replaceConfirm.trim().toUpperCase() === 'REPLACE');

  // The module list is only needed once the partial side is opened, and it costs a
  // per-table size scan, so it is fetched on first view rather than on mount.
  $effect(() => {
    if (scope === 'partial' && !usage && !loadingUsage) loadUsage();
  });

  // Seed the selection from the module list once it arrives, without clobbering a choice
  // the user already made.
  $effect(() => {
    for (const m of modules) {
      if (!(m.id in picked)) picked[m.id] = !BULKY.includes(m.id);
    }
  });

  async function loadUsage() {
    loadingUsage = true;
    usageError = '';
    try {
      usage = await settingsApi.dataUsage();
    } catch (e) {
      usageError = e.message;
    } finally {
      loadingUsage = false;
    }
  }

  async function download() {
    // A bundle with the sealed credential stores needs a recent password. The transfer is a
    // plain navigation (see below), which cannot carry a retry, so the grant is taken first:
    // a refusal here means the prompt was cancelled and nothing should be downloaded.
    if (withCredentials) {
      const { valid } = await settingsApi.reauthStatus().catch(() => ({ valid: false }));
      if (!valid && !(await promptReauth())) return;
    }
    // Let the browser do the transfer: a plain navigation streams to disk instead of
    // holding the whole zip in a blob in memory.
    window.location.href = settingsApi.exportUrl({
      modules: chosen,
      credentials: withCredentials
    });
  }

  // The selection, added up, so the download is a known quantity before it starts.
  const selected = $derived.by(() => {
    const picks = modules.filter((m) => picked[m.id]);
    return {
      count: picks.length,
      rows: picks.reduce((a, m) => a + m.rows, 0),
      bytes: picks.reduce((a, m) => a + m.size_bytes, 0),
      tables: picks.flatMap((m) => m.tables.map((t) => ({ ...t, module: m.name })))
    };
  });

  // Reading a file says what it holds and what it would meet here, with nothing written.
  // The counts in it are the file's own (its manifest records them), so this costs one
  // upload and a count per table, not a load.
  let preview = $state(null);
  let previewing = $state(false);

  const previewTotals = $derived.by(() => {
    const mods = preview?.modules ?? [];
    return {
      incoming: mods.reduce((a, m) => a + m.incoming_rows, 0),
      here: mods.reduce((a, m) => a + m.here_rows, 0),
      exact: mods.every((m) => m.here_exact)
    };
  });

  // An estimated count is marked rather than rounded away: the number is still the useful
  // one, it just is not a promise.
  const approx = (n, exact) => (exact ? fmtNum(n, 0) : `~${fmtNum(n, 0)}`);

  async function pickFile(picked) {
    file = picked ?? null;
    result = null;
    transferError = '';
    preview = null;
    if (!file) return;
    previewing = true;
    try {
      preview = await settingsApi.previewData({ file });
    } catch (err) {
      // A file the load would refuse (damaged, or from a newer version) is refused here
      // too, which is the point: it costs the user nothing at this stage.
      transferError = err.message;
      file = null;
    } finally {
      previewing = false;
    }
  }

  async function load() {
    if (!file || !replaceArmed) return;
    loadingIn = true;
    transferError = '';
    result = null;
    try {
      // No module list: load everything the file carries. Narrowing it is the export's job.
      result = await settingsApi.importData({ file, mode });
      replaceConfirm = '';
      // The "here now" column is stale the moment the load succeeds.
      preview = null;
      await loadUsage();
    } catch (e) {
      transferError = e.message;
    } finally {
      loadingIn = false;
    }
  }
</script>

<div class="section">
  <h2>{$t('settings.backup.title')}</h2>

  <Tabs tabs={scopeTabs} bind:value={scope} ariaLabel={$t('settings.backup.scopeLabel')} />
  <div class="sub-tabs">
    <Tabs tabs={actionTabs} bind:value={action} ariaLabel={$t('settings.backup.actionLabel')} />
  </div>

  {#if scope === 'full' && action === 'backup'}
    {#if scheduled}
      <div class="status {tone}">
        <strong>{headline}</strong>
        <ul>
          {#if scheduled.destination}
            <li>{$t('settings.backup.scheduledDest', { destination: scheduled.destination })}</li>
          {:else}
            <li>{$t('settings.backup.scheduledDestNone')}</li>
          {/if}
          <li>
            {scheduled.verified
              ? $t('settings.backup.scheduledVerified')
              : $t('settings.backup.scheduledUnverified')}
          </li>
          {#if scheduled.schedule}
            <li>{$t('settings.backup.scheduledNext', { schedule: scheduled.schedule })}</li>
          {/if}
          {#if humanSize(scheduled.size_bytes)}
            <li>{$t('settings.backup.scheduledSize', { size: humanSize(scheduled.size_bytes) })}</li>
          {/if}
          {#if scheduled.error}
            <li>{scheduled.error}</li>
          {/if}
          {#if scheduled.stale || failed}
            <li>{$t('settings.backup.scheduledStaleHelp')}</li>
          {/if}
        </ul>
      </div>
      <h3 class="first">{$t('settings.backup.manualTitle')}</h3>
    {/if}

    <p class="muted">{$t('settings.backup.intro')}</p>
    <CommandBlock command={plain} />

    <h3>{$t('settings.backup.encryptTitle')}</h3>
    <!-- Contains inline <code> markup; translators keep the tags. -->
    <p class="muted">{@html $t('settings.backup.encryptBody')}</p>
    <CommandBlock command={encrypted} />

    <div class="note">
      <strong>{$t('settings.backup.securityTitle')}</strong>
      <ul>
        <li>{@html $t('settings.backup.security1')}</li>
        <li>{@html $t('settings.backup.security2')}</li>
        <li>{@html $t('settings.backup.security3')}</li>
        <li>{$t('settings.backup.security4')}</li>
      </ul>
    </div>
  {:else if scope === 'full' && action === 'restore'}
    <p class="muted">{$t('settings.backup.restoreBody')}</p>
    <CommandBlock command={restorePlain} />
    <p class="muted">{$t('settings.backup.restoreEncrypted')}</p>
    <CommandBlock command={restoreEncrypted} />
  {:else if scope === 'partial' && action === 'backup'}
    <p class="muted">{$t('settings.backup.selIntro')}</p>

    <ErrorText error={usageError} />

    {#if loadingUsage && !usage}
      <Skeleton height="120px" />
    {:else}
      <div class="picker">
        {#each modules as m (m.id)}
          <label class="pick">
            <input type="checkbox" bind:checked={picked[m.id]} />
            <span>{m.name}</span>
            <span class="muted small nums">
              {$t('settings.backup.selRows', { n: fmtNum(m.rows, 0) })} · {fmtBytes(m.size_bytes)}
            </span>
          </label>
        {/each}
      </div>

      <p class="total">
        {$t('settings.backup.selSelected', {
          modules: selected.count,
          rows: fmtNum(selected.rows, 0),
          size: fmtBytes(selected.bytes)
        })}
      </p>

      {#if selected.tables.length}
        <details class="detail">
          <summary>{$t('settings.backup.selTablesDetail')}</summary>
          <div class="scroll">
            <table class="tbl">
            <thead>
              <tr>
                <th>{$t('settings.data.colModule')}</th>
                <th>{$t('settings.data.colTable')}</th>
                <th class="num">{$t('settings.data.colRows')}</th>
                <th class="num">{$t('settings.data.colSize')}</th>
              </tr>
            </thead>
            <tbody>
              {#each selected.tables as tbl (tbl.module + tbl.name)}
                <tr>
                  <td>{tbl.module}</td>
                  <td class="mono">{tbl.name}</td>
                  <td class="num">{fmtNum(tbl.rows, 0)}</td>
                  <td class="num">{fmtBytes(tbl.size_bytes)}</td>
                </tr>
              {/each}
              </tbody>
            </table>
          </div>
        </details>
      {/if}

      <label class="pick wide">
        <input type="checkbox" bind:checked={withCredentials} />
        <span>{$t('settings.backup.selCredentials')}</span>
      </label>

      <div class="actions left">
        <button class="primary" disabled={!chosen.length} onclick={download}>
          {$t('settings.backup.selDownload')}
        </button>
      </div>
    {/if}
  {:else}
    <p class="muted">{$t('settings.backup.selLoadIntro')}</p>

    <div class="loader">
      <FileDrop
        accept=".zip,application/zip"
        title={$t('settings.backup.selDrop')}
        hint={$t('settings.backup.selDropHint')}
        browseLabel={$t('settings.backup.selBrowse')}
        busyLabel={$t('settings.backup.previewing')}
        busy={previewing}
        onpick={pickFile}
      />

      {#if file}
        <p class="picked">
          <Icon name="file-text" size={14} />
          <span class="mono">{file.name}</span>
          <span class="muted small">{fmtBytes(file.size)}</span>
        </p>
      {/if}

      {#if preview}
        <!-- What the file holds against what is here, before anything is written. -->
        <h4>{$t('settings.backup.previewTitle')}</h4>
        <p class="muted small">
          {$t('settings.backup.previewFrom', {
            version: preview.app_version,
            date: preview.created_at.slice(0, 10)
          })}
          {#if preview.includes_credentials}{$t('settings.backup.previewCreds')}{/if}
        </p>

        <div class="scroll">
          <table class="tbl">
            <thead>
              <tr>
                <th>{$t('settings.data.colModule')}</th>
                <th class="num">{$t('settings.backup.colInFile')}</th>
                <th class="num">{$t('settings.backup.colHereNow')}</th>
              </tr>
            </thead>
            <tbody>
              {#each preview.modules as m (m.id)}
                <tr class="mod">
                  <td>{m.name}</td>
                  <td class="num">{fmtNum(m.incoming_rows, 0)}</td>
                  <td class="num">{approx(m.here_rows, m.here_exact)}</td>
                </tr>
                {#each m.tables as tbl (tbl.name)}
                  <tr class="sub">
                    <td class="mono">{tbl.name}</td>
                    <td class="num">{fmtNum(tbl.incoming_rows, 0)}</td>
                    <td class="num">{approx(tbl.here_rows, tbl.here_exact)}</td>
                  </tr>
                {/each}
              {/each}
            </tbody>
          </table>
        </div>

        {#if !previewTotals.exact}
          <p class="muted small">{$t('settings.backup.previewEstimate')}</p>
        {/if}

        {#if preview.unknown_modules.length}
          <p class="muted small">
            {$t('settings.backup.previewUnknown', {
              modules: preview.unknown_modules.join(', ')
            })}
          </p>
        {/if}
      {/if}

      <label class="pick wide">
        <input type="radio" value="merge" bind:group={mode} />
        <span>{$t('settings.backup.selMerge')}</span>
      </label>
      <label class="pick wide">
        <input type="radio" value="replace" bind:group={mode} />
        <span>{$t('settings.backup.selReplace')}</span>
      </label>

      {#if preview}
        <p class="outcome">
          {#if replacing}
            {$t('settings.backup.previewReplace', {
              here: approx(previewTotals.here, previewTotals.exact),
              rows: fmtNum(previewTotals.incoming, 0)
            })}
          {:else}
            {$t('settings.backup.previewMerge', { rows: fmtNum(previewTotals.incoming, 0) })}
          {/if}
        </p>
      {/if}

      {#if replacing}
        <div class="warn">
          <p>{$t('settings.backup.selReplaceWarn')}</p>
          <input bind:value={replaceConfirm} placeholder="REPLACE" />
        </div>
      {/if}

      <div class="actions left">
        <button
          class="primary"
          class:danger={replacing}
          disabled={!file || loadingIn || !replaceArmed}
          onclick={load}
        >
          {loadingIn ? $t('settings.backup.selLoading') : $t('settings.backup.selLoadButton')}
        </button>
      </div>

      <ErrorText error={transferError} />

      {#if result}
        <div class="done">
          <strong>{$t('settings.backup.selLoadDone')}</strong>
          <ul>
            {#each result.modules as m (m.id)}
              <li>{m.name}: {fmtNum(m.rows, 0)} {$t('settings.backup.selLoadRows')}</li>
            {/each}
            {#if result.safety_file}
              <li>{$t('settings.backup.selSafety', { file: result.safety_file })}</li>
            {/if}
          </ul>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .section {
    max-width: 680px;
  }
  h2 {
    margin: 0 0 var(--space-3);
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  h3 {
    margin: var(--space-6) 0 var(--space-1);
    font-size: 13.5px;
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
    color: var(--text);
  }
  /* Second level reads as a filter under the first, not as a second header. */
  .sub-tabs {
    margin: var(--space-2) 0 var(--space-4);
    padding-left: var(--space-2);
    font-size: var(--text-sm);
  }
  .muted {
    color: var(--dim);
    font-size: var(--text-base);
    line-height: 1.5;
  }
  .small {
    font-size: 11.5px;
  }
  code {
    font-family: var(--mono);
    font-size: 0.82em;
    background: var(--surface-2);
    padding: 1px 4px;
    border-radius: 0;
  }
  h3.first {
    margin-top: var(--space-6);
  }
  .status {
    margin: var(--space-3) 0 var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: 3px solid var(--green);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .status.warn {
    border-left-color: var(--amber);
  }
  .status.bad {
    border-left-color: var(--red);
  }
  .status ul {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    color: var(--muted);
  }
  .status li {
    margin: 6px 0;
    line-height: 1.45;
  }
  .note {
    margin-top: var(--space-4);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: 3px solid var(--amber);
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
    line-height: 1.45;
  }
  .picker {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: var(--space-1) var(--space-3);
    margin-top: var(--space-2);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 3px 0;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .pick span:last-child:not(:nth-child(2)) {
    margin-left: auto;
  }
  .pick.wide {
    margin-top: var(--space-2);
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .actions.left {
    justify-content: flex-start;
    margin-top: var(--space-3);
  }
  .loader {
    margin-top: var(--space-2);
  }
  .picked {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: var(--space-3) 0 0;
    font-size: var(--text-sm);
    color: var(--text);
  }
  h4 {
    margin: var(--space-4) 0 var(--space-1);
    font-size: var(--text-sm);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: var(--fw-medium);
    color: var(--muted);
  }
  .nums {
    white-space: nowrap;
  }
  .total {
    margin: var(--space-3) 0 0;
    font-size: var(--text-sm);
    color: var(--muted);
  }
  .detail {
    margin-top: var(--space-2);
    font-size: var(--text-sm);
  }
  .detail summary {
    cursor: pointer;
    color: var(--muted);
    padding: var(--space-1) 0;
  }
  .detail summary:hover {
    color: var(--text);
  }
  /* The list is as long as the selection, so it gets a window, with the header pinned. */
  .scroll {
    max-height: 320px;
    overflow: auto;
  }
  .scroll :global(thead th) {
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--surface);
  }
  .mono {
    font-family: var(--mono);
    font-size: 0.88em;
    color: var(--muted);
  }
  /* A module line is the total of the table lines under it. */
  tr.mod td {
    font-weight: var(--fw-medium);
  }
  tr.sub td:first-child {
    padding-left: var(--space-4);
  }
  tr.sub td {
    color: var(--muted);
  }
  .outcome {
    margin: var(--space-3) 0 0;
    font-size: var(--text-sm);
    color: var(--text);
  }
  .warn {
    margin-top: var(--space-3);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: 3px solid var(--red);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .warn p {
    margin: 0 0 var(--space-2);
  }
  .done {
    margin-top: var(--space-3);
    background: var(--surface);
    border: var(--hairline) solid var(--border);
    border-left: 3px solid var(--green);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text);
  }
  .done ul {
    margin: var(--space-2) 0 0;
    padding-left: var(--space-4);
    color: var(--muted);
  }
</style>

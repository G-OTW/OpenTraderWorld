<script>
  // Backup the whole database. The core runs distroless (no shell / no pg_dump) and is
  // deliberately kept without host or Docker access, so a full backup is an operator
  // action run on the host. We guide it here with copy-paste commands and the security
  // notes that matter (encryption at rest, secret-key handling, session tokens).
  import CommandBlock from '$lib/settings/CommandBlock.svelte';
  import { t } from '$lib/i18n';

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
</script>

<div class="section">
  <h2>{$t('settings.backup.title')}</h2>
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

  <h3>{$t('settings.backup.restoreTitle')}</h3>
  <p class="muted">{$t('settings.backup.restoreBody')}</p>
  <CommandBlock command={restorePlain} />
  <p class="muted">{$t('settings.backup.restoreEncrypted')}</p>
  <CommandBlock command={restoreEncrypted} />
</div>

<style>
  .section {
    max-width: 680px;
  }
  h2 {
    margin: 0 0 var(--space-2);
    font-size: 1.1rem;
    color: var(--text);
  }
  h3 {
    margin: var(--space-6) 0 var(--space-1);
    font-size: 0.95rem;
    color: var(--text);
  }
  .muted {
    color: var(--muted);
    font-size: 0.86rem;
    line-height: 1.5;
  }
  code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.82em;
    background: var(--surface-2);
    padding: 1px 4px;
    border-radius: 4px;
  }
  .note {
    margin-top: var(--space-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-left: 3px solid var(--amber);
    border-radius: var(--radius);
    padding: var(--space-3);
    font-size: 0.84rem;
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
</style>

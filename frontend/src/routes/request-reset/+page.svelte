<script>
  /**
   * Password recovery instructions.
   *
   * There is no reset form here on purpose: OTW is a single-user app on the user's own
   * server, with no mailbox to send a link to and no second account to vouch for the first.
   * Any unauthenticated endpoint that could change a password would be a way in, so the
   * reset lives on the host shell instead (`otw-core reset-password`) and this page is only
   * the manual for it.
   */
  import '$lib/ui/auth-card.css';
  import { t } from '$lib/i18n';
  import { copyLog } from '$lib/ui/copyLog.js';

  // The compose project is named `opentraderworld`, so the core container is this by
  // default; the notes below cover a renamed one.
  const CONTAINER = 'opentraderworld-core-1';
  const BIN = '/app/otw-core';

  const reset = `docker exec -it ${CONTAINER} ${BIN} reset-password USERNAME`;
  const list = `docker exec ${CONTAINER} ${BIN} list-users`;
  const stdin = `printf '%s' 'my-new-password' | docker exec -i ${CONTAINER} ${BIN} reset-password USERNAME --stdin`;
</script>

<div class="auth-card wide">
  <h1>{$t('resetHelp.title')}</h1>
  <p class="sub">{$t('resetHelp.intro')}</p>

  <ol>
    <li>{$t('resetHelp.step.shell')}</li>
    <li>
      {$t('resetHelp.step.run')}
      <code use:copyLog={reset} title={$t('resetHelp.copy')}>{reset}</code>
    </li>
    <li>{$t('resetHelp.step.signIn')}</li>
  </ol>

  <h2>{$t('resetHelp.more')}</h2>
  <dl>
    <dt>{$t('resetHelp.noUsername')}</dt>
    <dd><code use:copyLog={list} title={$t('resetHelp.copy')}>{list}</code></dd>

    <dt>{$t('resetHelp.ownPassword')}</dt>
    <dd><code use:copyLog={stdin} title={$t('resetHelp.copy')}>{stdin}</code></dd>

    <dt>{$t('resetHelp.renamed')}</dt>
    <dd><code use:copyLog={'docker ps'} title={$t('resetHelp.copy')}>docker ps</code></dd>
  </dl>

  <ul class="notes">
    <li>{$t('resetHelp.note.sessions')}</li>
    <li>{$t('resetHelp.note.data')}</li>
    <li>{$t('resetHelp.note.bare')}</li>
  </ul>

  <a class="link" href="/login">{$t('resetHelp.back')}</a>
</div>

<style>
  /* The card is sized for a login form; this one carries commands, which must not wrap
     mid-flag. Everything else (border, type scale, links) stays the shared auth card. */
  .wide {
    max-width: 620px;
  }

  ol {
    margin-top: var(--space-6);
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  ol li {
    color: var(--text);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
  }

  h2 {
    margin-top: var(--space-8);
    color: var(--dim);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }

  dl {
    margin-top: var(--space-4);
  }
  dt {
    color: var(--text);
    font-size: var(--text-sm);
    line-height: var(--lh-base);
    margin-top: var(--space-4);
  }
  dd {
    margin: 0;
  }

  /* Click-to-copy: the command is long and typed nowhere else, so the whole block is the
     target (`copyLog` adds the cursor + a data-copied flash). */
  code {
    display: block;
    margin-top: var(--space-2);
    padding: var(--space-3);
    background: var(--surface-2);
    border: var(--hairline) solid var(--border);
    font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
    font-size: var(--text-xs);
    line-height: var(--lh-base);
    color: var(--text);
    overflow-x: auto;
    white-space: pre;
  }
  code[data-copied] {
    border-color: var(--green);
  }

  .notes {
    margin-top: var(--space-6);
    padding-left: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .notes li {
    color: var(--dim);
    font-size: var(--text-xs);
    line-height: var(--lh-base);
  }
</style>

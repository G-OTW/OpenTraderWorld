<script>
  // One message, rendered as safely as a mail body can be rendered.
  //
  // The body goes into a sandboxed iframe with no `allow-same-origin` and no
  // `allow-scripts`, plus a restrictive CSP inside the document: even if something
  // survived the server-side sanitiser, it has no origin, no script engine and no
  // network. Remote images stay parked until the reader asks for them — that request
  // is the sender's read receipt, and it should be the user's decision, not a default.
  import { onMount } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { mailboxApi } from './api.js';
  import { fmtDateTime, fmtBytes } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    /** Row from the list (id + list metadata); null closes the reader. */
    row = null,
    /** Called after an action that changes the list (read/star/archive/delete). */
    onchange = () => {},
    /** Called when the reader is dismissed (mobile back / close button). */
    onclose = null
  } = $props();

  let loading = $state(false);
  let error = $state('');
  let data = $state(null); // { message, attachments, unsubscribe_url }
  let images = $state(false);
  let busy = $state('');
  let notice = $state('');
  let remindOpen = $state(false);

  // Reload whenever the selected row changes; images reset to off per message.
  $effect(() => {
    const id = row?.id;
    if (!id) {
      data = null;
      return;
    }
    images = false;
    notice = '';
    load(id, false);
  });

  async function load(id, withImages) {
    loading = true;
    error = '';
    try {
      data = await mailboxApi.getMessage(id, withImages);
      // Opening a message is what marks it read — no timer, no separate button.
      if (data?.message && !data.message.read) {
        await mailboxApi.updateMessage(id, { read: true });
        data.message.read = true;
        onchange();
      }
    } catch (e) {
      error = e.message;
      data = null;
    } finally {
      loading = false;
    }
  }

  const msg = $derived(data?.message ?? null);

  async function act(patch, key) {
    if (!msg) return;
    busy = key;
    try {
      await mailboxApi.updateMessage(msg.id, patch);
      Object.assign(msg, patch);
      onchange();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function showImages() {
    images = true;
    await load(msg.id, true);
  }

  async function unsubscribe() {
    busy = 'unsub';
    notice = '';
    try {
      const r = await mailboxApi.unsubscribe(msg.id);
      if (r.ok) {
        notice = $t('mailbox.unsubDone');
        onchange();
      } else if (r.open_url) {
        window.open(r.open_url, '_blank', 'noopener');
        notice = $t('mailbox.unsubOpened');
      }
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  // "Remind me" presets: the three answers to "not now" that people actually give.
  const REMIND_CHOICES = [
    { id: 'tonight', labelKey: 'mailbox.remind.tonight', hours: 0, at: '20:00', dayOffset: 0 },
    { id: 'tomorrow', labelKey: 'mailbox.remind.tomorrow', at: '09:00', dayOffset: 1 },
    { id: 'weekend', labelKey: 'mailbox.remind.weekend', at: '10:00', dayOffset: null }
  ];

  function dateFor(choice) {
    const d = new Date();
    if (choice.dayOffset === null) {
      // Next Saturday (today counts only if it is not already Saturday).
      const delta = (6 - d.getDay() + 7) % 7 || 7;
      d.setDate(d.getDate() + delta);
    } else {
      d.setDate(d.getDate() + choice.dayOffset);
    }
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  async function remind(choice) {
    busy = 'remind';
    remindOpen = false;
    try {
      await mailboxApi.remind(msg.id, {
        date: dateFor(choice),
        time: choice.at,
        tz_offset_minutes: -new Date().getTimezoneOffset()
      });
      notice = $t('mailbox.remindDone');
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  async function remove() {
    busy = 'delete';
    try {
      await mailboxApi.deleteMessage(msg.id);
      data = null;
      onchange();
      onclose?.();
    } catch (e) {
      error = e.message;
    } finally {
      busy = '';
    }
  }

  // The document handed to the iframe: reader typography + a CSP that pins the body
  // to "text and, if asked, pictures". `srcdoc` + no allow-same-origin means the frame
  // is a unique opaque origin, so it cannot read this app at all.
  const READER_CSS = `
    :root { color-scheme: light dark; }
    body { margin: 0; padding: 20px 22px 40px;
      font: 15px/1.65 ui-sans-serif, -apple-system, "Segoe UI", Roboto, sans-serif;
      color: #1c1e21; background: #fff; word-break: break-word; }
    @media (prefers-color-scheme: dark) {
      body { color: #d8dade; background: #17181b; }
      a { color: #7aa7ff; }
    }
    img, video, table { max-width: 100% !important; height: auto; }
    table { border-collapse: collapse; }
    td, th { padding: 4px 6px; }
    a { color: #1f5fd0; text-decoration: underline; }
    blockquote { margin: 0 0 0 12px; padding-left: 12px; border-left: 2px solid #8884; }
    pre { white-space: pre-wrap; }
    h1, h2, h3 { line-height: 1.25; }
    hr { border: 0; border-top: 1px solid #8883; margin: 20px 0; }
  `;

  const srcdoc = $derived.by(() => {
    if (!msg) return '';
    const csp = `default-src 'none'; img-src ${images ? "https: data:" : "'none'"}; style-src 'unsafe-inline'; font-src data:; form-action 'none'; base-uri 'none'`;
    const body = msg.body_html
      ? msg.body_html
      : `<pre>${escapeHtml(msg.body_text ?? '')}</pre>`;
    return `<!doctype html><html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${csp}">
<style>${READER_CSS}</style></head><body>${body}</body></html>`;
  });

  function escapeHtml(s) {
    return s
      .replaceAll('&', '&amp;')
      .replaceAll('<', '&lt;')
      .replaceAll('>', '&gt;');
  }

  // Close the remind menu on any outside click.
  onMount(() => {
    const close = () => (remindOpen = false);
    window.addEventListener('click', close);
    return () => window.removeEventListener('click', close);
  });
</script>

{#if !row}
  <div class="placeholder">
    <Icon name="mail" size={26} strokeWidth={1.4} />
    <p>{$t('mailbox.readerEmpty')}</p>
  </div>
{:else if loading && !msg}
  <div class="pad"><Skeleton rows={6} /></div>
{:else if error && !msg}
  <div class="pad"><ErrorText {error} /></div>
{:else if msg}
  <article class="reader">
    <header>
      <div class="top">
        {#if onclose}
          <button class="icon back" onclick={onclose} aria-label={$t('common.back')}>
            <Icon name="arrow-left" size={16} />
          </button>
        {/if}
        <div class="who">
          <h2>{msg.subject || $t('mailbox.noSubject')}</h2>
          <p>
            <strong>{msg.sender_name}</strong>
            <span class="addr">{msg.sender_addr}</span>
            <span class="dot">·</span>
            <time>{fmtDateTime(msg.received_at)}</time>
          </p>
        </div>
        <Badge tone="neutral">{$t(`mailbox.cat.${msg.sender_category}`)}</Badge>
      </div>

      <div class="actions">
        <Button
          size="sm"
          variant="ghost"
          icon="star"
          loading={busy === 'star'}
          onclick={() => act({ starred: !msg.starred }, 'star')}
        >
          {msg.starred ? $t('mailbox.unstar') : $t('mailbox.star')}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="eye-off"
          loading={busy === 'read'}
          onclick={() => act({ read: false }, 'read')}
        >
          {$t('mailbox.markUnread')}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon="inbox"
          loading={busy === 'archive'}
          onclick={() => act({ archived: !msg.archived }, 'archive')}
        >
          {msg.archived ? $t('mailbox.unarchive') : $t('mailbox.archive')}
        </Button>

        <div class="menu-wrap">
          <Button
            size="sm"
            variant="ghost"
            icon="bell"
            loading={busy === 'remind'}
            onclick={(e) => {
              e.stopPropagation();
              remindOpen = !remindOpen;
            }}
          >
            {$t('mailbox.remindMe')}
          </Button>
          {#if remindOpen}
            <!-- The window listener closes the menu; each item closes it as it acts, so
                 no click-catcher wrapper (and no keyboard trap) is needed here. -->
            <div class="menu" role="menu" tabindex="-1">
              {#each REMIND_CHOICES as c (c.id)}
                <button role="menuitem" onclick={() => remind(c)}>{$t(c.labelKey)}</button>
              {/each}
            </div>
          {/if}
        </div>

        {#if data?.unsubscribe_url}
          <Button
            size="sm"
            variant="ghost"
            icon="log-out"
            loading={busy === 'unsub'}
            onclick={unsubscribe}
          >
            {$t('mailbox.unsubscribe')}
          </Button>
        {/if}
        <span class="spacer"></span>
        <Button
          size="sm"
          variant="danger"
          icon="trash"
          loading={busy === 'delete'}
          onclick={remove}
        >
          {$t('common.delete')}
        </Button>
      </div>

      {#if notice}<p class="notice">{notice}</p>{/if}
      {#if error}<ErrorText {error} />{/if}

      {#if msg.has_remote_images && !images}
        <div class="imgbar">
          <Icon name="eye-off" size={13} />
          <span>{$t('mailbox.imagesBlocked')}</span>
          <button class="link" onclick={showImages}>{$t('mailbox.loadImages')}</button>
        </div>
      {/if}

      {#if data?.attachments?.length}
        <ul class="attachments">
          {#each data.attachments as a (a.id)}
            <li>
              <a href={mailboxApi.attachmentUrl(a.id)} download={a.filename}>
                <Icon name="file-text" size={13} />
                <span class="name">{a.filename}</span>
                <span class="size">{fmtBytes(a.size_bytes)}</span>
              </a>
            </li>
          {/each}
        </ul>
      {/if}
    </header>

    <iframe
      class="body"
      title={msg.subject || $t('mailbox.noSubject')}
      sandbox="allow-popups allow-popups-to-escape-sandbox"
      referrerpolicy="no-referrer"
      srcdoc={srcdoc}
    ></iframe>
  </article>
{/if}

<style>
  .placeholder {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    color: var(--muted);
  }
  .placeholder p {
    margin: 0;
    font-size: var(--text-sm);
  }
  .pad {
    padding: var(--space-4);
  }

  .reader {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  header {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  .top {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .who {
    min-width: 0;
    flex: 1;
  }
  h2 {
    margin: 0;
    font-size: var(--text-md);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .who p {
    margin: var(--space-1) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    align-items: baseline;
  }
  .who strong {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .addr {
    overflow-wrap: anywhere;
  }
  .dot {
    opacity: 0.6;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-3);
  }
  .spacer {
    flex: 1;
  }

  .menu-wrap {
    position: relative;
  }
  .menu {
    position: absolute;
    z-index: 20;
    top: calc(100% + 4px);
    left: 0;
    min-width: 170px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
    padding: var(--space-1);
    display: flex;
    flex-direction: column;
  }
  .menu button {
    background: none;
    border: 0;
    text-align: left;
    padding: var(--space-2);
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--surface-2);
  }

  .notice {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--green);
  }

  .imgbar {
    margin-top: var(--space-2);
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    background: var(--surface-2);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .link {
    background: none;
    border: 0;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font-size: inherit;
    text-decoration: underline;
  }

  .attachments {
    list-style: none;
    margin: var(--space-2) 0 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .attachments a {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    color: var(--text);
    text-decoration: none;
    max-width: 260px;
  }
  .attachments a:hover {
    background: var(--surface-2);
  }
  .attachments .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .attachments .size {
    color: var(--muted);
    flex: none;
  }

  .body {
    flex: 1;
    min-height: 0;
    width: 100%;
    border: 0;
    background: var(--surface);
  }

  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: var(--space-1);
    border-radius: var(--radius);
  }
  .icon:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .back {
    flex: none;
  }
</style>

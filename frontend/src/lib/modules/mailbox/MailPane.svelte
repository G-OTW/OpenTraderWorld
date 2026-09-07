<script>
  // The reading screen: categories + senders on the left, messages in the middle,
  // the message itself on the right. Three panes on a desktop, one at a time on a
  // phone — the same information, never a different feature set.
  import { untrack } from 'svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import MessageReader from './MessageReader.svelte';
  import { mailboxApi, CATEGORIES } from './api.js';
  import { relativeTime } from '$lib/format.js';
  import { t } from '$lib/i18n';

  let {
    /** Connected accounts, for the mailbox filter (only shown when there are several). */
    accounts = [],
    /** Bumped by the page after a manual fetch so the panes reload. */
    refreshKey = 0,
    /** Called when counts changed, so the page header can refresh. */
    oncounts = () => {}
  } = $props();

  let senders = $state([]);
  let messages = $state([]);
  let loadingSenders = $state(true);
  let loadingList = $state(true);
  let error = $state('');

  // Filters
  let category = $state('all');
  let senderId = $state(null);
  let accountId = $state('');
  let search = $state('');
  let unreadOnly = $state(false);
  let starredOnly = $state(false);

  let selected = $state(null); // the selected message row
  let mobileReader = $state(false);

  let confirmSender = $state(null);

  // Debounce the search box: one request per pause, not per keystroke.
  let searchTimer = null;
  function onSearch(v) {
    search = v;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => loadMessages(), 250);
  }

  // Filters listed explicitly, then the load runs untracked: `search` is read inside
  // loadMessages and would otherwise become a dependency, firing a request per keystroke
  // and defeating the debounce above.
  $effect(() => {
    void [category, senderId, accountId, unreadOnly, starredOnly, refreshKey];
    untrack(() => loadMessages());
  });

  $effect(() => {
    void refreshKey;
    untrack(() => loadSenders());
  });

  async function loadSenders() {
    loadingSenders = true;
    try {
      senders = await mailboxApi.listSenders();
    } catch (e) {
      error = e.message;
    } finally {
      loadingSenders = false;
    }
  }

  async function loadMessages() {
    loadingList = true;
    error = '';
    try {
      messages = await mailboxApi.listMessages({
        category,
        sender_id: senderId ?? '',
        account_id: accountId,
        q: search,
        unread: unreadOnly ? true : '',
        starred: starredOnly ? true : '',
        limit: 200
      });
      // Keep the open message only while it is still in the visible list.
      if (selected && !messages.some((m) => m.id === selected.id)) selected = null;
    } catch (e) {
      error = e.message;
      messages = [];
    } finally {
      loadingList = false;
    }
  }

  async function afterChange() {
    await Promise.all([loadMessages(), loadSenders()]);
    oncounts();
  }

  const kept = $derived(senders.filter((s) => s.status === 'kept'));
  const pending = $derived(senders.filter((s) => s.status === 'pending'));

  // Senders of the active category, grouped by domain — the store's own shelves.
  const grouped = $derived.by(() => {
    const list = kept.filter((s) => category === 'all' || s.category === category);
    const map = new Map();
    for (const s of list) {
      if (!map.has(s.domain)) map.set(s.domain, []);
      map.get(s.domain).push(s);
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  });

  function unreadFor(cat) {
    return kept
      .filter((s) => cat === 'all' || s.category === cat)
      .reduce((n, s) => n + (s.unread_count ?? 0), 0);
  }

  async function keepSender(s, cat) {
    await mailboxApi.updateSender(s.id, { status: 'kept', category: cat });
    await afterChange();
  }

  async function ignoreSender(s) {
    await mailboxApi.updateSender(s.id, { status: 'ignored' });
    await loadSenders();
  }

  async function setCategory(s, value) {
    await mailboxApi.updateSender(s.id, { category: value });
    await afterChange();
  }

  async function deleteSender() {
    const s = confirmSender;
    confirmSender = null;
    if (!s) return;
    await mailboxApi.deleteSender(s.id);
    if (senderId === s.id) senderId = null;
    await afterChange();
  }

  async function markAllRead() {
    await mailboxApi.readAll(senderId);
    await afterChange();
  }

  async function toggleStar(m, e) {
    e.stopPropagation();
    await mailboxApi.updateMessage(m.id, { starred: !m.starred });
    m.starred = !m.starred;
  }

  function open(m) {
    selected = m;
    mobileReader = true;
  }

  // Keyboard: move through the list with ↑/↓ (or j/k), open with Enter. Ignored while
  // typing — a mail list that hijacks the search box is worse than no shortcuts.
  function onKeydown(e) {
    const tag = document.activeElement?.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
    const idx = messages.findIndex((m) => m.id === selected?.id);
    if (e.key === 'ArrowDown' || e.key === 'j') {
      e.preventDefault();
      const next = messages[Math.min(idx + 1, messages.length - 1)] ?? messages[0];
      if (next) open(next);
    } else if (e.key === 'ArrowUp' || e.key === 'k') {
      e.preventDefault();
      const prev = messages[Math.max(idx - 1, 0)];
      if (prev) open(prev);
    } else if (e.key === 'Escape') {
      mobileReader = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="mail" class:reading={mobileReader}>
  <!-- ── Sidebar: categories, mailbox filter, senders ───────────────────── -->
  <aside class="side">
    <!-- Categories as a folder list, not wrapping pills: five labels of unequal width
         never line up on a 232px column, and a mail client already has a word for
         "the thing you click on the left". -->
    <nav class="cats" aria-label={$t('mailbox.categories')}>
      {#each ['all', ...CATEGORIES] as c (c)}
        <button
          class="cat"
          class:active={category === c}
          aria-current={category === c ? 'true' : undefined}
          onclick={() => {
            category = c;
            senderId = null;
          }}
        >
          <span class="nm">{c === 'all' ? $t('mailbox.cat.all') : $t(`mailbox.cat.${c}`)}</span>
          {#if unreadFor(c) > 0}<span class="count">{unreadFor(c)}</span>{/if}
        </button>
      {/each}
    </nav>

    {#if accounts.length > 1}
      <div class="accountpick">
        <Dropdown
          bind:value={accountId}
          ariaLabel={$t('mailbox.filterByMailbox')}
          options={[
            { value: '', label: $t('mailbox.allMailboxes') },
            ...accounts.map((a) => ({ value: a.id, label: a.name || a.email || a.username }))
          ]}
        />
      </div>
    {/if}

    {#if pending.length}
      <section class="pending">
        <h3>
          <Icon name="alert-triangle" size={12} />
          {$t('mailbox.pendingTitle')}
        </h3>
        <p>{$t('mailbox.pendingHint')}</p>
        {#each pending as s (s.id)}
          <div class="pendrow">
            <div class="who">
              <strong>{s.name || s.from_addr}</strong>
              <span>{s.last_subject}</span>
            </div>
            <div class="pendactions">
              <button onclick={() => keepSender(s, 'broker')}>{$t('mailbox.cat.broker')}</button>
              <button onclick={() => keepSender(s, 'news')}>{$t('mailbox.cat.news')}</button>
              <button onclick={() => keepSender(s, 'other')}>{$t('mailbox.cat.other')}</button>
              <button class="mute" onclick={() => ignoreSender(s)}>{$t('mailbox.ignore')}</button>
            </div>
          </div>
        {/each}
      </section>
    {/if}

    <div class="senders">
      <button class="sender all" class:active={!senderId} onclick={() => (senderId = null)}>
        <span class="nm">{$t('mailbox.allSenders')}</span>
      </button>
      {#if loadingSenders}
        <div class="pad"><Skeleton rows={4} /></div>
      {:else}
        {#each grouped as [domain, list] (domain)}
          <p class="domain">{domain}</p>
          {#each list as s (s.id)}
            <button
              class="sender"
              class:active={senderId === s.id}
              onclick={() => (senderId = s.id)}
            >
              <span class="nm">{s.name || s.from_addr}</span>
              {#if s.unread_count > 0}<span class="count">{s.unread_count}</span>{/if}
            </button>
          {/each}
        {/each}
        {#if !grouped.length}
          <p class="none">{$t('mailbox.noSenders')}</p>
        {/if}
      {/if}
    </div>
  </aside>

  <!-- ── List ───────────────────────────────────────────────────────────── -->
  <section class="list">
    <!-- One control height across the bar: the input carries its own border from the
         control layer (no wrapper box around a box, no inset icon) and the chips
         match it. -->
    <div class="listbar">
      <div class="search">
        <input
          type="search"
          placeholder={$t('mailbox.searchMail')}
          value={search}
          oninput={(e) => onSearch(e.currentTarget.value)}
        />
      </div>
      <button
        class="chip"
        class:active={unreadOnly}
        onclick={() => (unreadOnly = !unreadOnly)}
        aria-pressed={unreadOnly}
      >
        {$t('mailbox.unread')}
      </button>
      <button
        class="chip sq"
        class:active={starredOnly}
        onclick={() => (starredOnly = !starredOnly)}
        aria-pressed={starredOnly}
        title={$t('mailbox.star')}
      >
        <Icon name="star" size={13} />
      </button>
      <button class="chip sq" onclick={markAllRead} title={$t('mailbox.markAllRead')}>
        <Icon name="check" size={13} />
      </button>
    </div>

    {#if senderId}
      {@const s = senders.find((x) => x.id === senderId)}
      {#if s}
        <div class="senderhead">
          <div>
            <strong>{s.name || s.from_addr}</strong>
            <span class="addr">{s.from_addr}</span>
          </div>
          <div class="senderacts">
            <Dropdown
              value={s.category}
              onpick={(v) => setCategory(s, v)}
              ariaLabel={$t('mailbox.category')}
              options={CATEGORIES.map((c) => ({ value: c, label: $t(`mailbox.cat.${c}`) }))}
            />
            <button
              class="icon"
              onclick={() => (confirmSender = s)}
              title={$t('mailbox.deleteSender')}
            >
              <Icon name="trash" size={13} />
            </button>
          </div>
        </div>
      {/if}
    {/if}

    {#if error}<div class="pad"><ErrorText {error} /></div>{/if}

    {#if loadingList}
      <div class="pad"><Skeleton rows={6} /></div>
    {:else if !messages.length}
      <div class="pad">
        <EmptyState
          icon="inbox"
          compact
          title={$t('mailbox.noMailTitle')}
          description={$t('mailbox.noMailDesc')}
        />
      </div>
    {:else}
      <ul>
        {#each messages as m (m.id)}
          <li>
            <button
              class="row"
              class:unread={!m.read}
              class:active={selected?.id === m.id}
              onclick={() => open(m)}
            >
              <span class="meta">
                <span class="from">{m.sender_name}</span>
                <span class="when">{relativeTime(m.received_at)}</span>
              </span>
              <span class="subject">{m.subject || $t('mailbox.noSubject')}</span>
              <span class="snippet">{m.snippet}</span>
              <span class="icons">
                {#if m.attachment_count > 0}<Icon name="file-text" size={12} />{/if}
              </span>
            </button>
            <button
              class="starbtn"
              class:on={m.starred}
              onclick={(e) => toggleStar(m, e)}
              aria-label={$t('mailbox.star')}
            >
              <Icon name="star" size={13} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- ── Reader ─────────────────────────────────────────────────────────── -->
  <section class="reader">
    <MessageReader
      row={selected}
      onchange={afterChange}
      onclose={() => {
        mobileReader = false;
        selected = null;
      }}
    />
  </section>
</div>

<ConfirmModal
  open={!!confirmSender}
  title={$t('mailbox.deleteSender')}
  message={$t('mailbox.deleteSenderMsg', { name: confirmSender?.name || confirmSender?.from_addr })}
  confirmLabel={$t('common.delete')}
  danger
  onconfirm={deleteSender}
  oncancel={() => (confirmSender = null)}
/>

<style>
  .mail {
    display: grid;
    grid-template-columns: 232px 340px 1fr;
    gap: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    overflow: hidden;
    background: var(--surface);
    /* Fill what the page leaves below the header: three panes that scroll independently. */
    flex: 1;
    min-height: 460px;
  }

  .side,
  .list {
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
  }
  .reader {
    min-height: 0;
    overflow: hidden;
  }

  /* ── Categories ── */
  .cats {
    display: flex;
    flex-direction: column;
    padding: var(--space-1);
    border-bottom: var(--hairline) solid var(--border);
    flex: none;
  }
  .cat {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    width: 100%;
    height: 28px;
    padding: 0 var(--space-2);
    border: 0;
    background: none;
    color: var(--muted);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .cat:hover {
    background: var(--surface-2);
    color: var(--text);
  }
  .cat.active {
    background: var(--surface-2);
    color: var(--text);
    font-weight: var(--fw-medium);
    box-shadow: inset 2px 0 0 var(--text);
  }
  /* Unread counts: a right-aligned tabular number, not a coloured bubble — the
     accent budget goes to nav/status/focus, and the digits have to line up. */
  .count {
    flex: none;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .cat.active .count,
  .sender.active .count {
    color: var(--text);
  }

  .accountpick {
    margin: var(--space-2);
    flex: none;
  }

  /* ── Pending senders ── */
  .pending {
    padding: var(--space-2);
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
    flex: none;
  }
  .pending h3 {
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--amber);
  }
  .pending p {
    margin: var(--space-1) 0 var(--space-2);
    font-size: 11px;
    color: var(--muted);
    line-height: 1.4;
  }
  .pendrow {
    padding: var(--space-2) 0;
    border-top: 1px solid var(--border);
  }
  .pendrow .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .pendrow strong {
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pendrow .who span {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pendactions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: var(--space-1);
  }
  .pendactions button {
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    border-radius: var(--radius);
    padding: 1px 6px;
    font-size: 11px;
    cursor: pointer;
  }
  .pendactions button:hover {
    border-color: var(--accent);
  }
  .pendactions .mute {
    color: var(--muted);
  }

  /* ── Sender list ── */
  .senders {
    padding: var(--space-2) var(--space-1) var(--space-4);
  }
  .domain {
    margin: var(--space-3) 0 var(--space-1);
    padding: 0 var(--space-2);
    font-size: 10px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .sender {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: 5px var(--space-2);
    border: 0;
    background: none;
    border-radius: var(--radius);
    color: var(--text);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }
  .sender:hover {
    background: var(--surface-2);
  }
  .sender.active {
    background: var(--surface-2);
    font-weight: var(--fw-medium);
  }
  .sender .nm {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .none {
    padding: var(--space-3) var(--space-2);
    font-size: var(--text-xs);
    color: var(--muted);
  }

  /* ── List ── */
  .listbar {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2);
    border-bottom: var(--hairline) solid var(--border);
    flex: none;
  }
  .search {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
  }
  .search input {
    width: 100%;
  }
  .search input::-webkit-search-cancel-button {
    appearance: none;
  }
  /* .chip comes from the control layer; only the sizing is local so the whole bar
     is one 32px line. */
  .listbar .chip {
    flex: none;
    height: var(--control-h);
    padding: 0 var(--space-3);
  }
  .listbar .chip.sq {
    width: var(--control-h);
    padding: 0;
    justify-content: center;
  }

  .senderhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
    padding: var(--space-2);
    border-bottom: 1px solid var(--border);
    flex: none;
  }
  .senderhead strong {
    display: block;
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .senderhead .addr {
    font-size: 11px;
    color: var(--muted);
  }
  .senderacts {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }
  .senderacts :global(.dd) {
    min-width: 110px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    position: relative;
    border-bottom: 1px solid var(--border);
  }
  .row {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: 0;
    padding: var(--space-2) 30px var(--space-2) var(--space-3);
    cursor: pointer;
    color: var(--text);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .row.active {
    background: var(--surface-2);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    font-size: 11px;
    color: var(--muted);
  }
  .from {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .when {
    flex: none;
  }
  .subject {
    display: block;
    font-size: var(--text-sm);
    line-height: 1.35;
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.unread .subject {
    font-weight: var(--fw-medium);
  }
  .row.unread .from {
    color: var(--text);
  }
  .snippet {
    display: block;
    font-size: var(--text-xs);
    color: var(--muted);
    margin-top: 2px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icons {
    position: absolute;
    right: 30px;
    bottom: var(--space-2);
    color: var(--muted);
  }
  .starbtn {
    position: absolute;
    top: var(--space-2);
    right: var(--space-1);
    background: none;
    border: 0;
    color: var(--border);
    cursor: pointer;
    padding: 2px;
  }
  .starbtn.on {
    color: var(--amber);
  }
  .starbtn:hover {
    color: var(--amber);
  }

  .pad {
    padding: var(--space-3);
  }
  .icon {
    background: none;
    border: 0;
    color: var(--muted);
    cursor: pointer;
    padding: 2px;
    border-radius: var(--radius);
  }
  .icon:hover {
    color: var(--red);
  }

  /* ── Responsive: three panes → two → one ── */
  @media (max-width: 1200px) {
    .mail {
      grid-template-columns: 200px 300px 1fr;
    }
  }
  @media (max-width: 980px) {
    .mail {
      grid-template-columns: 1fr 1fr;
      height: auto;
      min-height: 0;
    }
    .side {
      grid-column: 1 / -1;
      border-right: 0;
      border-bottom: 1px solid var(--border);
      max-height: 320px;
    }
    .list,
    .reader {
      height: 62vh;
    }
  }
  @media (max-width: 700px) {
    .mail {
      grid-template-columns: 1fr;
    }
    .list {
      border-right: 0;
    }
    .reader {
      display: none;
      height: 78vh;
    }
    /* Opening a message swaps the list out for it — no cramped split on a phone. */
    .mail.reading .list,
    .mail.reading .side {
      display: none;
    }
    .mail.reading .reader {
      display: block;
    }
  }
</style>

<script>
  // Settings → Voice → Voice commands.
  //
  // A command is a phrase (plus other ways of saying it) and the steps it runs, in order.
  // Commands chain when spoken together: "turtle and open settings" runs the turtle
  // command, then the built-in open. Every plan stops on a confirmation unless all of it
  // is commands marked to skip it, and that mark is off on every new command.
  import { onMount } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Dropdown from '$lib/ui/Dropdown.svelte';
  import TagInput from '$lib/ui/TagInput.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import EmptyState from '$lib/ui/EmptyState.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';
  import { t } from '$lib/i18n';
  import { voiceApi } from '$lib/voice/api.js';
  import { voice } from '$lib/voice/voice.svelte.js';
  import { speak } from '$lib/voice/audio.js';
  import { shortcutKeys } from '$lib/voice/shortcut.js';
  import { modules, visibleModules } from '$lib/modules/registry';
  import { installedIds } from '$lib/modules/installed.js';
  import { automatorApi } from '$lib/modules/automator/api.js';

  const KINDS = [
    { kind: 'navigate', icon: 'arrow-right' },
    { kind: 'agent', icon: 'brain' },
    { kind: 'automator', icon: 'zap' },
    { kind: 'theme', icon: 'moon' },
    { kind: 'privacy', icon: 'eye-off' },
    { kind: 'say', icon: 'volume-2' }
  ];
  const ICON = Object.fromEntries(KINDS.map((k) => [k.kind, k.icon]));

  let commands = $state([]);
  let workflows = $state([]);
  let loading = $state(true);
  let error = $state('');
  let search = $state('');
  let confirmDelete = $state('');

  // Editor
  let modalOpen = $state(false);
  let editing = $state(null);
  let fPhrase = $state('');
  let fAliases = $state([]);
  let fSteps = $state([]);
  let fBypass = $state(false);
  let fEnabled = $state(true);
  let formError = $state('');
  let saving = $state(false);
  let aliasInput = $state(null);
  let addOpen = $state(false);

  // Pages a step can open: installed modules, the dashboard, settings.
  const pages = $derived([
    { value: '/', label: $t('voice.target.dashboard') },
    ...visibleModules($installedIds)
      .filter((m) => !m.home)
      .map((m) => ({ value: m.base, label: m.name })),
    { value: '/settings', label: $t('nav.settings') }
  ]);

  const shown = $derived.by(() => {
    const q = search.trim().toLowerCase();
    if (!q) return commands;
    return commands.filter((c) =>
      [c.phrase, ...c.aliases].some((p) => p.toLowerCase().includes(q))
    );
  });

  const examples = $derived(
    modules.find((m) => m.id === 'journal')
      ? $t('voice.cmd.exampleChain')
      : ''
  );

  onMount(reload);

  async function reload() {
    loading = true;
    error = '';
    try {
      commands = (await voiceApi.commands()) ?? [];
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
    automatorApi
      .list()
      .then((r) => (workflows = r.workflows ?? []))
      .catch(() => (workflows = []));
  }

  function blankStep(kind) {
    switch (kind) {
      case 'navigate':
        return { kind, target: pages[1]?.value ?? '/', label: pages[1]?.label ?? '' };
      case 'agent':
        return { kind, prompt: '' };
      case 'automator':
        return { kind, workflow_id: workflows[0]?.id ?? '', name: workflows[0]?.name ?? '' };
      case 'theme':
        return { kind, value: 'toggle' };
      case 'privacy':
        return { kind, value: 'toggle' };
      case 'say':
        return { kind, text: '' };
    }
  }

  function openCreate(seed = null) {
    editing = null;
    fPhrase = seed?.phrase ?? '';
    fAliases = seed?.aliases ?? [];
    fSteps = seed?.steps ?? [blankStep('navigate')];
    fBypass = false;
    fEnabled = true;
    formError = '';
    modalOpen = true;
  }

  function openEdit(c) {
    editing = c;
    fPhrase = c.phrase;
    fAliases = [...c.aliases];
    fSteps = structuredClone($state.snapshot(c.steps));
    fBypass = c.bypass_confirm;
    fEnabled = c.enabled;
    formError = '';
    modalOpen = true;
  }

  function addStep(kind) {
    fSteps = [...fSteps, blankStep(kind)];
    addOpen = false;
  }

  function moveStep(i, d) {
    const j = i + d;
    if (j < 0 || j >= fSteps.length) return;
    const next = [...fSteps];
    [next[i], next[j]] = [next[j], next[i]];
    fSteps = next;
  }

  function removeStep(i) {
    fSteps = fSteps.filter((_, k) => k !== i);
  }

  async function save() {
    aliasInput?.flush?.();
    saving = true;
    formError = '';
    try {
      const payload = {
        phrase: fPhrase,
        aliases: fAliases,
        steps: $state.snapshot(fSteps),
        bypass_confirm: fBypass,
        enabled: fEnabled
      };
      if (editing) await voiceApi.updateCommand(editing.id, payload);
      else await voiceApi.addCommand(payload);
      modalOpen = false;
      await reload();
      await voice.load();
    } catch (e) {
      formError = e.message;
    } finally {
      saving = false;
    }
  }

  async function toggle(c) {
    try {
      await voiceApi.updateCommand(c.id, { ...c, enabled: !c.enabled });
      await reload();
      await voice.load();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(c) {
    if (confirmDelete !== c.id) {
      confirmDelete = c.id;
      return;
    }
    confirmDelete = '';
    try {
      await voiceApi.deleteCommand(c.id);
      await reload();
      await voice.load();
    } catch (e) {
      error = e.message;
    }
  }

  function stepText(s) {
    switch (s.kind) {
      case 'navigate':
        return $t('voice.step.navigate', { target: s.label || s.target });
      case 'agent':
        return $t('voice.step.agent', { prompt: s.prompt });
      case 'automator':
        return $t('voice.step.automator', { name: s.name || '…' });
      case 'theme':
        return $t(`voice.step.theme.${s.value}`);
      case 'privacy':
        return $t(`voice.step.privacy.${s.value}`);
      case 'say':
        return $t('voice.step.say', { text: s.text });
      default:
        return s.kind;
    }
  }

  const keys = $derived(shortcutKeys(voice.settings?.shortcut ?? ''));
</script>

<div class="section">
  <header class="head">
    <div class="heading">
      <h2>{$t('voice.cmd.title')}</h2>
      <p class="muted small">{$t('voice.cmd.subtitle')}</p>
    </div>
  </header>

  <ErrorText {error} />

  {#if voice.settings && !voice.settings.enabled}
    <div class="banner">
      <span class="banner-icon" aria-hidden="true"><Icon name="info" size={16} /></span>
      <p>
        {$t('voice.cmd.offNote')}
        <a href="/settings#voice">{$t('voice.cmd.offLink')}</a>
      </p>
    </div>
  {/if}

  <!-- How a sentence becomes a plan, at a glance. -->
  <section class="how">
    <div class="how-step">
      <span class="how-icon"><Icon name="mic" size={15} /></span>
      <div>
        <strong>{$t('voice.cmd.how1Title')}</strong>
        <p class="muted small">
          {$t('voice.cmd.how1Body')}
          {#if keys.length}<span class="keys">{#each keys as k (k)}<kbd>{k}</kbd>{/each}</span>{/if}
        </p>
      </div>
    </div>
    <div class="how-step">
      <span class="how-icon"><Icon name="link" size={15} /></span>
      <div>
        <strong>{$t('voice.cmd.how2Title')}</strong>
        <p class="muted small">{$t('voice.cmd.how2Body')}</p>
      </div>
    </div>
    <div class="how-step">
      <span class="how-icon"><Icon name="shield" size={15} /></span>
      <div>
        <strong>{$t('voice.cmd.how3Title')}</strong>
        <p class="muted small">{$t('voice.cmd.how3Body')}</p>
      </div>
    </div>
  </section>

  <section class="list-card">
    <div class="card-head">
      <h3>{$t('voice.cmd.listTitle')} <span class="count">{commands.length}</span></h3>
      <input
        class="search"
        type="search"
        placeholder={$t('voice.cmd.search')}
        aria-label={$t('voice.cmd.search')}
        bind:value={search}
      />
      <button class="primary" onclick={() => openCreate()}>
        <Icon name="plus" size={13} /> {$t('voice.cmd.new')}
      </button>
    </div>

    {#if loading}
      <div class="rows" aria-busy="true">
        {#each [0, 1] as i (i)}
          <div class="cmd"><Skeleton height="2.4rem" /></div>
        {/each}
      </div>
    {:else if !commands.length}
      <div class="empty-wrap">
        <EmptyState icon="mic" title={$t('voice.cmd.none')} description={$t('voice.cmd.noneBody')}>
          {#snippet action()}
            <button
              class="ghost"
              onclick={() =>
                openCreate({
                  phrase: 'turtle',
                  aliases: [],
                  steps: [
                    { kind: 'navigate', target: '/journal', label: 'Trading Journal' },
                    { kind: 'agent', prompt: $t('voice.cmd.examplePrompt') }
                  ]
                })}
            >
              <Icon name="sparkles" size={13} /> {$t('voice.cmd.tryExample')}
            </button>
          {/snippet}
        </EmptyState>
      </div>
    {:else if !shown.length}
      <div class="empty-wrap">
        <EmptyState icon="search" title={$t('voice.cmd.noMatch')} compact />
      </div>
    {:else}
      <ul class="rows">
        {#each shown as c (c.id)}
          <li class="cmd" class:off={!c.enabled}>
            <div class="phrase">
              <span class="say">“{c.phrase}”</span>
              {#each c.aliases as a (a)}<span class="alias">{a}</span>{/each}
            </div>
            <ol class="chain">
              {#each c.steps as s, i (i)}
                <li>
                  <Icon name={ICON[s.kind] ?? 'arrow-right'} size={12} />
                  <span>{stepText(s)}</span>
                </li>
              {/each}
            </ol>
            <div class="meta">
              {#if c.bypass_confirm}
                <Badge tone="warn" icon="zap">{$t('voice.cmd.noConfirm')}</Badge>
              {/if}
              <label class="mini-switch" title={c.enabled ? $t('voice.cmd.disable') : $t('voice.cmd.enable')}>
                <input type="checkbox" checked={c.enabled} onchange={() => toggle(c)} />
                <span>{c.enabled ? $t('voice.cmd.active') : $t('voice.cmd.inactive')}</span>
              </label>
              <button class="ghost" onclick={() => openEdit(c)}>
                <Icon name="pencil" size={12} /> {$t('common.edit')}
              </button>
              <button class="ghost danger" class:armed={confirmDelete === c.id} onclick={() => remove(c)}>
                <Icon name="trash" size={12} />
                {confirmDelete === c.id ? $t('voice.cmd.deleteConfirm') : $t('common.delete')}
              </button>
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <aside class="notes-card">
    <span class="notes-icon" aria-hidden="true"><Icon name="lightbulb" size={17} /></span>
    <ul class="notes muted small">
      <li>{$t('voice.cmd.builtinOpen')}</li>
      <li>{$t('voice.cmd.builtinAgent')}</li>
      {#if examples}<li>{examples}</li>{/if}
    </ul>
  </aside>
</div>

<Modal bind:open={modalOpen} title={editing ? $t('voice.cmd.editTitle') : $t('voice.cmd.new')} size="lg">
  <div class="form">
    <div class="grid2">
      <label class="field">
        <span>{$t('voice.cmd.phrase')}</span>
        <input type="text" bind:value={fPhrase} maxlength="120" placeholder={$t('voice.cmd.phrasePlaceholder')} />
        <small class="muted">{$t('voice.cmd.phraseHint')}</small>
      </label>
      <div class="field">
        <span>{$t('voice.cmd.aliases')}</span>
        <TagInput
          bind:this={aliasInput}
          bind:tags={fAliases}
          listId="voice-aliases"
          placeholder={$t('voice.cmd.aliasesPlaceholder')}
        />
        <small class="muted">{$t('voice.cmd.aliasesHint')}</small>
      </div>
    </div>

    <div class="field">
      <span>{$t('voice.cmd.steps')}</span>
      <ol class="builder">
        {#each fSteps as s, i (i)}
          <li class="bstep">
            <span class="bnum">{i + 1}</span>
            <div class="bkind">
              <Dropdown
                value={s.kind}
                ariaLabel={$t('voice.cmd.stepKind')}
                options={KINDS.map((k) => ({ value: k.kind, label: $t(`voice.kind.${k.kind}`) }))}
                onpick={(v) => (fSteps[i] = blankStep(v))}
              />
            </div>
            <div class="bbody">
              {#if s.kind === 'navigate'}
                <Dropdown
                  value={s.target}
                  ariaLabel={$t('voice.kind.navigate')}
                  options={pages}
                  onpick={(v) => (fSteps[i] = { ...s, target: v, label: pages.find((p) => p.value === v)?.label ?? '' })}
                />
              {:else if s.kind === 'agent'}
                <textarea rows="2" bind:value={fSteps[i].prompt} placeholder={$t('voice.cmd.agentPlaceholder')}></textarea>
              {:else if s.kind === 'automator'}
                {#if workflows.length}
                  <Dropdown
                    value={s.workflow_id}
                    ariaLabel={$t('voice.kind.automator')}
                    options={workflows.map((w) => ({ value: w.id, label: w.name }))}
                    onpick={(v) => (fSteps[i] = { ...s, workflow_id: v, name: workflows.find((w) => w.id === v)?.name ?? '' })}
                  />
                {:else}
                  <p class="muted small">{$t('voice.cmd.noWorkflows')}</p>
                {/if}
              {:else if s.kind === 'theme'}
                <Dropdown
                  value={s.value}
                  ariaLabel={$t('voice.kind.theme')}
                  options={['toggle', 'dark', 'light'].map((v) => ({ value: v, label: $t(`voice.step.theme.${v}`) }))}
                  onpick={(v) => (fSteps[i] = { ...s, value: v })}
                />
              {:else if s.kind === 'privacy'}
                <Dropdown
                  value={s.value}
                  ariaLabel={$t('voice.kind.privacy')}
                  options={['toggle', 'on', 'off'].map((v) => ({ value: v, label: $t(`voice.step.privacy.${v}`) }))}
                  onpick={(v) => (fSteps[i] = { ...s, value: v })}
                />
              {:else if s.kind === 'say'}
                <div class="sayrow">
                  <input type="text" bind:value={fSteps[i].text} placeholder={$t('voice.cmd.sayPlaceholder')} />
                  <button
                    type="button"
                    class="ghost icon"
                    title={$t('voice.settings.listen')}
                    aria-label={$t('voice.settings.listen')}
                    onclick={() => speak(s.text, voice.settings?.language)}
                  >
                    <Icon name="volume-2" size={13} />
                  </button>
                </div>
              {/if}
            </div>
            <div class="bactions">
              <button type="button" class="ghost icon" disabled={i === 0} aria-label={$t('voice.cmd.moveUp')} onclick={() => moveStep(i, -1)}>
                <Icon name="arrow-up" size={12} />
              </button>
              <button type="button" class="ghost icon" disabled={i === fSteps.length - 1} aria-label={$t('voice.cmd.moveDown')} onclick={() => moveStep(i, 1)}>
                <Icon name="arrow-down" size={12} />
              </button>
              <button type="button" class="ghost icon danger" disabled={fSteps.length === 1} aria-label={$t('common.delete')} onclick={() => removeStep(i)}>
                <Icon name="trash" size={12} />
              </button>
            </div>
          </li>
        {/each}
      </ol>
      {#if addOpen}
        <div class="kinds">
          {#each KINDS as k (k.kind)}
            <button type="button" class="kind" onclick={() => addStep(k.kind)}>
              <Icon name={k.icon} size={15} />
              <span>
                <strong>{$t(`voice.kind.${k.kind}`)}</strong>
                <small class="muted">{$t(`voice.kindHint.${k.kind}`)}</small>
              </span>
            </button>
          {/each}
        </div>
      {:else}
        <button type="button" class="ghost add" onclick={() => (addOpen = true)}>
          <Icon name="plus" size={12} /> {$t('voice.cmd.addStep')}
        </button>
      {/if}
    </div>

    <label class="bypass" class:on={fBypass}>
      <input type="checkbox" bind:checked={fBypass} />
      <span>
        <strong>{$t('voice.cmd.bypass')}</strong>
        <small>{$t('voice.cmd.bypassHint')}</small>
      </span>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={fEnabled} />
      <span>{$t('voice.cmd.enabled')}</span>
    </label>

    <ErrorText error={formError} />
  </div>
  {#snippet footer()}
    <button class="ghost" onclick={() => (modalOpen = false)}>{$t('common.cancel')}</button>
    <button class="primary" disabled={saving} onclick={save}>
      {saving ? $t('common.saving') : $t('common.save')}
    </button>
  {/snippet}
</Modal>

<style>
  .section {
    display: flex;
    width: min(100%, 980px);
    flex-direction: column;
    gap: var(--space-4);
  }
  .section h2 {
    margin: 0;
    color: var(--text);
    font-size: var(--fs-item-title);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
  }
  .heading {
    display: flex;
    max-width: 72ch;
    flex-direction: column;
    gap: 6px;
  }
  .heading p {
    margin: 0;
  }
  .muted {
    color: var(--dim);
  }
  .small {
    font-size: var(--fs-desc);
    line-height: 1.5;
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-left: var(--active-rule, 2px) solid var(--amber);
    border-radius: var(--radius);
    background: var(--surface-2);
    padding: 11px 14px;
    color: var(--muted);
  }
  .banner p {
    margin: 0;
    font-size: var(--text-sm);
  }
  .banner a {
    color: var(--accent);
  }
  .banner-icon {
    display: inline-flex;
    margin-top: 2px;
    color: var(--amber-ink, var(--amber));
  }
  .how {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-2);
  }
  .how-step {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    padding: var(--space-3);
  }
  .how-step strong {
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .how-step p {
    margin: 3px 0 0;
  }
  .how-icon {
    display: inline-flex;
    width: 30px;
    height: 30px;
    flex: none;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    color: var(--accent);
  }
  .keys {
    display: inline-flex;
    gap: 3px;
    margin-left: 4px;
  }
  kbd {
    border: var(--hairline) solid var(--border-control);
    border-bottom-width: 2px;
    border-radius: 4px;
    background: var(--surface-2);
    padding: 0 5px;
    color: var(--text);
    font-family: inherit;
    font-size: var(--text-xs);
  }
  .list-card {
    overflow: hidden;
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius-lg);
    background: var(--surface);
  }
  .card-head {
    display: flex;
    min-height: 60px;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
  }
  .card-head h3 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
    color: var(--text);
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
  }
  .count {
    border-radius: 999px;
    background: var(--surface-2);
    padding: 0 8px;
    color: var(--dim);
    font-size: var(--text-xs);
  }
  .card-head .search {
    margin-left: auto;
    width: 100%;
    max-width: 240px;
  }
  .empty-wrap {
    min-height: 220px;
    border-top: var(--hairline) solid var(--border);
    display: grid;
    place-items: center;
  }
  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: var(--hairline) solid var(--border);
  }
  .cmd {
    display: grid;
    grid-template-columns: minmax(180px, 0.9fr) 1.6fr auto;
    align-items: center;
    gap: var(--space-4);
    border-bottom: var(--hairline) solid var(--border);
    padding: var(--space-3) var(--space-4);
  }
  .cmd:last-child {
    border-bottom: 0;
  }
  .cmd:hover {
    background: color-mix(in srgb, var(--surface-2) 58%, transparent);
  }
  .cmd.off .phrase,
  .cmd.off .chain {
    opacity: 0.5;
  }
  .phrase {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px;
    min-width: 0;
  }
  .say {
    color: var(--text);
    font-size: var(--text-lg);
    font-weight: var(--fw-medium);
  }
  .alias {
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    padding: 1px 7px;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .chain {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px;
    min-width: 0;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chain li {
    display: inline-flex;
    max-width: 100%;
    align-items: center;
    gap: 5px;
    border: var(--hairline) solid var(--border);
    border-radius: 999px;
    padding: 3px 10px;
    color: var(--muted);
    font-size: var(--text-xs);
  }
  .chain li span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chain li + li::before {
    content: '';
    display: none;
  }
  .meta {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    white-space: nowrap;
  }
  .meta button.armed {
    border-color: color-mix(in srgb, var(--red) 50%, var(--border-control));
    color: var(--red);
  }
  .mini-switch {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--muted);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .mini-switch input {
    width: auto;
  }
  .notes-card {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 72%, var(--surface));
    padding: var(--space-3) var(--space-4);
  }
  .notes-icon {
    display: inline-flex;
    width: 34px;
    height: 34px;
    align-items: center;
    justify-content: center;
    flex: none;
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
  }
  .notes {
    display: flex;
    flex-direction: column;
    gap: 5px;
    margin: 1px 0 0;
    padding-left: 18px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .grid2 {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    font-size: var(--text-base);
  }
  .field small {
    font-size: var(--text-xs);
  }
  .builder {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .bstep {
    display: grid;
    grid-template-columns: 24px 190px 1fr auto;
    align-items: start;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--surface-2) 40%, var(--surface));
    padding: var(--space-2);
  }
  .bnum {
    display: inline-flex;
    width: 22px;
    height: 22px;
    margin-top: 6px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
    color: var(--accent);
    font-size: var(--text-xs);
  }
  .bbody textarea {
    width: 100%;
    resize: vertical;
  }
  .bbody p {
    margin: 8px 0 0;
  }
  .sayrow {
    display: flex;
    gap: var(--space-1);
  }
  .sayrow input {
    flex: 1;
  }
  .bactions {
    display: flex;
    gap: 2px;
  }
  .icon {
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    padding: 0;
  }
  .add {
    align-self: flex-start;
    margin-top: var(--space-1);
  }
  .kinds {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-2);
    margin-top: var(--space-1);
  }
  .kind {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    border: var(--hairline) solid var(--border-control);
    border-radius: var(--radius);
    background: var(--surface);
    padding: 9px 11px;
    color: var(--accent);
    text-align: left;
    cursor: pointer;
  }
  .kind:hover {
    border-color: var(--accent);
  }
  .kind span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .kind strong {
    color: var(--text);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .kind small {
    font-size: var(--text-xs);
  }
  .bypass {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    border: var(--hairline) solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-3);
    cursor: pointer;
  }
  .bypass.on {
    border-color: color-mix(in srgb, var(--amber) 50%, var(--border-control));
    background: color-mix(in srgb, var(--amber) 7%, var(--surface));
  }
  .bypass input,
  .check input {
    width: auto;
    margin-top: 3px;
  }
  .bypass span {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .bypass strong {
    color: var(--text);
    font-weight: var(--fw-medium);
  }
  .bypass small {
    color: var(--muted);
    font-size: var(--text-xs);
    line-height: 1.5;
  }
  .check {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  @media (max-width: 860px) {
    .how {
      grid-template-columns: 1fr;
    }
    .cmd {
      grid-template-columns: 1fr;
      gap: var(--space-2);
    }
    .meta {
      flex-wrap: wrap;
    }
    .grid2 {
      grid-template-columns: 1fr;
    }
    .bstep {
      grid-template-columns: 24px 1fr;
    }
    .bbody,
    .bactions {
      grid-column: 2;
    }
    .card-head {
      flex-wrap: wrap;
    }
  }
</style>

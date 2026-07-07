<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Customize the check-in prompts: per phase, list prompts with enable toggle, inline label
  // edit, option editing for choice/tags, delete, add new, and reset-to-defaults.
  import Modal from '$lib/ui/Modal.svelte';
  import { mindsetApi, PHASES, KINDS } from './api.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), onchanged = () => {} } = $props();

  let prompts = $state([]);
  let error = $state('');

  // New-prompt form.
  let nphase = $state('pre');
  let nkind = $state('scale');
  let nlabel = $state('');
  let noptions = $state('');

  $effect(() => {
    if (open) load();
  });

  function load() {
    error = '';
    mindsetApi
      .listPrompts()
      .then((r) => (prompts = r))
      .catch((e) => (error = e.message));
  }

  function done() {
    onchanged();
  }

  async function toggle(p) {
    try {
      await mindsetApi.updatePrompt(p.id, { active: !p.active });
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function rename(p, label) {
    if (!label.trim() || label === p.label) return;
    try {
      await mindsetApi.updatePrompt(p.id, { label: label.trim() });
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function saveOptions(p, raw) {
    const options = raw.split(',').map((s) => s.trim()).filter(Boolean);
    if (options.length === 0) return;
    try {
      await mindsetApi.updatePrompt(p.id, { config: { ...p.config, options } });
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function remove(p) {
    if (!confirm($t('mindset.promptsModal.confirmDeletePrompt', { label: p.label }))) return;
    try {
      await mindsetApi.deletePrompt(p.id);
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function add() {
    if (!nlabel.trim()) return;
    const body = { phase: nphase, kind: nkind, label: nlabel.trim() };
    if (nkind === 'choice' || nkind === 'tags') {
      body.config = { options: noptions.split(',').map((s) => s.trim()).filter(Boolean) };
      if (body.config.options.length === 0) {
        error = $t('mindset.promptsModal.optionsListRequired');
        return;
      }
    }
    try {
      await mindsetApi.addPrompt(body);
      nlabel = '';
      noptions = '';
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function reset() {
    if (!confirm($t('mindset.promptsModal.confirmReset'))) return;
    try {
      await mindsetApi.resetPrompts();
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }

  async function clearAll() {
    if (!confirm($t('mindset.promptsModal.confirmClearAll'))) return;
    try {
      await mindsetApi.clearPrompts();
      load();
      done();
    } catch (e) {
      error = e.message;
    }
  }
</script>

<Modal bind:open size="lg" title={$t('mindset.promptsModal.title')}>
  <div class="wrap">
    {#if error}<p class="err">{error}</p>{/if}

    {#each PHASES as phase (phase.key)}
      <section>
        <h4>{phase.icon} {phase.label}</h4>
        <ul>
          {#each prompts.filter((p) => p.phase === phase.key) as p (p.id)}
            <li class:off={!p.active}>
              <input
                class="lbl"
                value={p.label}
                onchange={(e) => rename(p, e.target.value)}
                aria-label={$t('mindset.promptsModal.promptLabel')}
              />
              <span class="kind">{KINDS.find((k) => k.key === p.kind)?.label ?? p.kind}</span>
              {#if p.kind === 'choice' || p.kind === 'tags'}
                <input
                  class="opts"
                  value={(p.config?.options ?? []).join(', ')}
                  onchange={(e) => saveOptions(p, e.target.value)}
                  aria-label={$t('mindset.promptsModal.optionsCommaSeparated')}
                  title={$t('mindset.promptsModal.optionsCommaSeparated')}
                />
              {/if}
              <button class="tgl" onclick={() => toggle(p)}>{p.active ? $t('mindset.promptsModal.on') : $t('mindset.promptsModal.off')}</button>
              <button class="x" onclick={() => remove(p)} aria-label={$t('mindset.promptsModal.deletePrompt')}><Icon name="x" size={13} /></button>
            </li>
          {/each}
        </ul>
      </section>
    {/each}

    <section class="add">
      <h4>{$t('mindset.promptsModal.addAPrompt')}</h4>
      <div class="addrow">
        <select bind:value={nphase}>
          {#each PHASES as ph (ph.key)}<option value={ph.key}>{ph.label}</option>{/each}
        </select>
        <select bind:value={nkind}>
          {#each KINDS as k (k.key)}<option value={k.key}>{k.label}</option>{/each}
        </select>
        <input class="grow" placeholder={$t('mindset.promptsModal.promptLabelPlaceholder')} bind:value={nlabel} />
      </div>
      {#if nkind === 'choice' || nkind === 'tags'}
        <input placeholder={$t('mindset.promptsModal.optionsPlaceholder')} bind:value={noptions} />
      {/if}
      <div class="addfoot">
        <button class="btn subtle" onclick={reset}>{$t('mindset.promptsModal.resetToDefaults')}</button>
        <button class="btn danger" onclick={clearAll}>{$t('mindset.promptsModal.deleteAll')}</button>
        <div class="spacer"></div>
        <button class="btn primary" onclick={add}>{$t('mindset.promptsModal.addPrompt')}</button>
      </div>
    </section>
  </div>
</Modal>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  h4 {
    font-size: 0.9rem;
    font-weight: 700;
    margin-bottom: var(--space-2);
  }
  ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  li.off .lbl,
  li.off .kind,
  li.off .opts {
    opacity: 0.45;
  }
  .lbl {
    flex: 2 1 220px;
    min-width: 0;
  }
  .opts {
    flex: 2 1 180px;
    min-width: 0;
    color: var(--muted);
  }
  .kind {
    font-size: 0.72rem;
    color: var(--muted);
    background: var(--surface-2);
    border-radius: var(--radius);
    padding: 2px var(--space-1);
    white-space: nowrap;
  }
  .tgl {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: 0.75rem;
    padding: 2px var(--space-2);
    cursor: pointer;
    width: 40px;
  }
  li:not(.off) .tgl {
    color: var(--green);
    border-color: var(--green);
  }
  .x {
    background: transparent;
    border: none;
    color: var(--muted);
    cursor: pointer;
    font-size: 0.75rem;
  }
  .x:hover {
    color: var(--red);
  }
  .add {
    border-top: 1px solid var(--border);
    padding-top: var(--space-3);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .addrow {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }
  .grow {
    flex: 1 1 220px;
  }
  .addfoot {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: 0.85rem;
    cursor: pointer;
  }
  .btn.primary {
    border-color: var(--accent);
  }
  .btn.subtle {
    color: var(--muted);
  }
  .btn.danger {
    color: var(--red);
    border-color: var(--red);
    background: transparent;
  }
  .err {
    color: var(--red);
    font-size: 0.85rem;
  }
</style>

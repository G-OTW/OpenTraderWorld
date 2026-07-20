<script>
  import Icon from '$lib/ui/Icon.svelte';
  // Customize the check-in prompts: per phase, list prompts with enable toggle, inline label
  // edit, option editing for choice/tags, delete, add new, and reset-to-defaults.
  import Modal from '$lib/ui/Modal.svelte';
  import ConfirmModal from '$lib/ui/ConfirmModal.svelte';
  import { mindsetApi, PHASES, KINDS } from './api.js';
  import { t } from '$lib/i18n';
  import ErrorText from '$lib/ui/ErrorText.svelte';

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

  // Three destructive actions (delete one prompt / reset to the starter set / clear all)
  // share one dialog: `pendingAction` carries its label, message and the work to run.
  let confirmOpen = $state(false);
  let pendingAction = $state(null); // { label, message, run }

  function askConfirm(label, message, run) {
    pendingAction = { label, message, run };
    confirmOpen = true;
  }

  async function confirmPending() {
    const a = pendingAction;
    pendingAction = null;
    if (!a) return;
    await a.run();
  }

  function remove(p) {
    askConfirm(
      $t('mindset.promptsModal.deletePrompt'),
      $t('mindset.promptsModal.confirmDeletePrompt', { label: p.label }),
      async () => {
        try {
          await mindsetApi.deletePrompt(p.id);
          load();
          done();
        } catch (e) {
          error = e.message;
        }
      }
    );
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

  function reset() {
    askConfirm(
      $t('mindset.promptsModal.resetToDefaults'),
      $t('mindset.promptsModal.confirmReset'),
      async () => {
        try {
          await mindsetApi.resetPrompts();
          load();
          done();
        } catch (e) {
          error = e.message;
        }
      }
    );
  }

  function clearAll() {
    askConfirm(
      $t('mindset.promptsModal.deleteAll'),
      $t('mindset.promptsModal.confirmClearAll'),
      async () => {
        try {
          await mindsetApi.clearPrompts();
          load();
          done();
        } catch (e) {
          error = e.message;
        }
      }
    );
  }
</script>

<Modal bind:open size="lg" title={$t('mindset.promptsModal.title')}>
  <div class="wrap">
    <ErrorText error={error} />

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

<!-- Rendered after the host Modal, so at equal --z-modal it stacks on top of it. -->
<ConfirmModal
  bind:open={confirmOpen}
  title={pendingAction?.label ?? ''}
  message={pendingAction?.message ?? ''}
  confirmLabel={pendingAction?.label ?? ''}
  cancelLabel={$t('common.cancel')}
  danger
  onconfirm={confirmPending}
  oncancel={() => (pendingAction = null)}
/>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  h4 {
    font-size: var(--text-base);
    font-weight: var(--fw-medium);
    letter-spacing: 0.02em;
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
    font-size: var(--text-xs);
    color: var(--muted);
    background: var(--surface-2);
    border-radius: 0;
    padding: 2px var(--space-1);
    white-space: nowrap;
  }
  .tgl {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    color: var(--muted);
    font-size: var(--text-xs);
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
    font-size: var(--text-xs);
  }
  .x:hover {
    color: var(--red);
  }
  .add {
    border-top: 0.5px solid var(--border);
    margin-top: var(--space-3);
    padding-top: var(--space-4);
    padding-bottom: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
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
    margin-top: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .btn {
    background: transparent;
    border: 0.5px solid var(--border-control);
    border-radius: 0;
    padding: var(--space-1) var(--space-3);
    color: var(--text);
    font-size: var(--text-base);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .btn:hover {
    background: var(--surface-2);
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
</style>

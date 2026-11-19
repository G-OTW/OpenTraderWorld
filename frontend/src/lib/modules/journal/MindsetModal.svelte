<script>
  // Pick the mindset check-ins this book reads on its calendar. Nothing is created or
  // answered here: templates are written in the Mindset module, and the choice is a
  // user preference per book (`MINDSET_PREF`), so the calendar follows it at once.
  import Modal from '$lib/ui/Modal.svelte';
  import { mindsetApi, PHASES } from '$lib/modules/mindset/api.js';
  import { pref } from '$lib/prefs.svelte.js';
  import { MINDSET_PREF } from './mindsetDays.js';
  import { t } from '$lib/i18n';

  let { open = $bindable(false), categoryId = '', categoryName = '' } = $props();

  const picks = pref(MINDSET_PREF, {});
  const selected = $derived(picks.value?.[categoryId] ?? []);

  let library = $state([]);
  let loading = $state(false);
  let error = $state('');

  $effect(() => {
    if (!open) return;
    loading = true;
    mindsetApi
      .listTemplates()
      .then((r) => {
        // The module answers [{ template, prompts }]; inactive templates are never asked.
        const rows = (r?.templates ?? r ?? []).map((x) => x.template ?? x);
        const order = (p) => PHASES.findIndex((ph) => ph.key === p);
        library = rows
          .filter((tpl) => tpl.active !== false)
          .sort((a, b) => order(a.phase) - order(b.phase) || a.position - b.position);
        error = '';
      })
      .catch((e) => (error = e?.message ?? 'failed to load templates'))
      .finally(() => (loading = false));
  });

  const phaseIcon = (p) => PHASES.find((ph) => ph.key === p)?.icon ?? '';

  function toggle(id) {
    const next = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
    const map = { ...(picks.value ?? {}) };
    if (next.length) map[categoryId] = next;
    else delete map[categoryId];
    picks.value = map;
  }
</script>

<Modal bind:open title={$t('journal.mindset.title')} size="sm" fit>
  <div class="mind">
    <p class="scope">{categoryName}</p>
    {#if error}<p class="err">{error}</p>{/if}
    {#if loading}
      <p class="muted">{$t('common.loading')}</p>
    {:else if library.length === 0}
      <p class="muted">{$t('journal.mindset.empty')}</p>
    {:else}
      <div class="picklist">
        {#each library as tpl (tpl.id)}
          <label class="prow">
            <input type="checkbox" checked={selected.includes(tpl.id)} onchange={() => toggle(tpl.id)} />
            <span class="pname">{tpl.name}</span>
            <span class="pphase" title={$t(`mindset.phase.${tpl.phase}`)}>{phaseIcon(tpl.phase)}</span>
          </label>
        {/each}
      </div>
      <p class="hint">{$t('journal.mindset.hint')}</p>
    {/if}
  </div>
</Modal>

<style>
  .mind {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }
  .scope,
  .muted,
  .hint {
    font-size: var(--text-sm);
    color: var(--muted);
    margin: 0;
  }
  .hint {
    font-size: var(--text-xs);
    color: var(--dim);
  }
  .err {
    color: var(--red);
    font-size: var(--text-sm);
    margin: 0;
  }
  .picklist {
    border: 0.5px solid var(--border-control);
    max-height: 280px;
    overflow-y: auto;
  }
  .prow {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    cursor: pointer;
  }
  .prow:hover {
    background: var(--surface-2);
  }
  .pname {
    font-size: var(--text-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (max-width: 767px) {
    .prow {
      min-height: 44px;
    }
    .pname {
      white-space: normal;
      overflow-wrap: anywhere;
    }
  }
</style>

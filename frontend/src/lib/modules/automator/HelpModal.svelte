<script>
  // What the editor cannot say on its own: the reading order of the grid, how a block
  // reaches the previous one's result, and where the credentials live. One screen, no
  // navigation, because it is read once with the canvas already open behind it.
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import { t } from '$lib/i18n';

  let { open = $bindable(false) } = $props();

  const SECTIONS = [
    { id: 'grid', lines: ['g1', 'g2', 'g3', 'g4'] },
    { id: 'data', lines: ['d1', 'd2', 'd3', 'd4'] },
    { id: 'blocks', lines: ['api', 'http', 'agent', 'transform', 'notify', 'delay'] },
    { id: 'secrets', lines: ['s1'] },
    { id: 'run', lines: ['r1', 'r2'] },
    { id: 'fail', lines: ['f1', 'f2'] }
  ];
</script>

<Modal bind:open title={$t('automator.help.title')} size="lg">
  <div class="doc">
    {#each SECTIONS as section (section.id)}
      <section>
        <h4>{$t(`automator.help.${section.id}`)}</h4>
        {#each section.lines as line (line)}
          <p>{$t(`automator.help.${section.id}.${line}`)}</p>
        {/each}
      </section>
    {/each}
  </div>
  {#snippet footer()}
    <Button variant="primary" onclick={() => (open = false)}>{$t('common.close')}</Button>
  {/snippet}
</Modal>

<style>
  .doc {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  h4 {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  p {
    margin: 0 0 var(--space-2);
    font-size: var(--text-sm);
    color: var(--dim);
    line-height: var(--lh-normal, 1.5);
  }
  p:last-child {
    margin-bottom: 0;
  }
  section + section {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-4);
  }
</style>

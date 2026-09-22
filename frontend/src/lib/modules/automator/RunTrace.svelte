<script>
  // A run, step by step. This view is the product: a workflow that fails at 3 a.m. is only
  // fixable if the trace says which block failed, with what payload, and how long it took.
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import { automatorApi, KIND_META, STATUS_TONE, fmtDuration } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let { steps = [], runId = '', dense = false } = $props();

  let openStep = $state('');

  const blobOf = (output) =>
    output && typeof output === 'object' && output._blob ? output._blob : '';
</script>

<ol class="trace" class:dense>
  {#each steps as step (step.seq)}
    <li class="step {step.status}">
      <button class="row" onclick={() => (openStep = openStep === step.seq ? '' : step.seq)}>
        <Icon name={openStep === step.seq ? 'chevron-down' : 'chevron-right'} size={12} />
        <Icon name={KIND_META[step.kind]?.icon ?? 'zap'} size={13} />
        <span class="name">{step.node_name || step.node_id}</span>
        <Badge tone={STATUS_TONE[step.status] ?? 'neutral'}>
          {$t(`automator.status.${step.status}`)}
        </Badge>
        {#if step.duration_ms}<span class="ms">{fmtDuration(step.duration_ms)}</span>{/if}
      </button>

      {#if step.error}
        <p class="err">{step.error}</p>
      {/if}

      {#if openStep === step.seq}
        <div class="detail">
          {#if step.request && Object.keys(step.request).length}
            <p class="lbl">{$t('automator.node.request')}</p>
            <pre>{JSON.stringify(step.request, null, 2)}</pre>
          {/if}
          <p class="lbl">{$t('automator.node.output')}</p>
          <pre>{JSON.stringify(step.output, null, 2)}</pre>
          {#if blobOf(step.output) && runId}
            <a class="blob" href={automatorApi.blobUrl(runId, blobOf(step.output))} download>
              <Icon name="download" size={12} />
              {$t('automator.trace.download')}
            </a>
          {/if}
        </div>
      {/if}
    </li>
  {/each}
</ol>

<style>
  .trace {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 0.5px solid var(--border);
  }
  .step + .step {
    border-top: 0.5px solid var(--border);
  }
  .step.failed {
    border-left: 2px solid var(--red);
  }
  .step.ok {
    border-left: 2px solid var(--green);
  }
  .step.simulated {
    border-left: 2px solid var(--amber);
  }
  .step.skipped {
    opacity: 0.6;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    background: none;
    border: 0;
    padding: var(--space-2) var(--space-3);
    cursor: pointer;
    text-align: left;
    color: var(--text);
  }
  .row:hover {
    background: var(--surface-2);
  }
  .name {
    font-size: var(--text-sm);
    font-weight: var(--fw-medium);
  }
  .ms {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--muted);
    font-family: var(--mono);
  }
  .err {
    margin: 0;
    padding: 0 var(--space-3) var(--space-2) calc(var(--space-3) + 20px);
    font-size: var(--text-xs);
    color: var(--red);
  }
  .detail {
    padding: 0 var(--space-3) var(--space-3);
  }
  .lbl {
    margin: var(--space-2) 0 var(--space-1);
    font-size: var(--text-xs);
    color: var(--dim);
  }
  pre {
    background: var(--surface-2);
    padding: var(--space-2);
    font-size: 10px;
    max-height: 260px;
    overflow: auto;
    margin: 0;
  }
  .blob {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--accent);
  }
  .dense pre {
    max-height: 160px;
  }
</style>

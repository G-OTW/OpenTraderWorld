<script>
  // The block editor: its settings, its failure policy, and a Test button that runs this
  // block alone so nobody has to save a schedule to find out whether a call works.
  //
  // Edits are made on a copy and applied on save, so closing the modal is a real cancel.
  import { untrack } from 'svelte';
  import Modal from '$lib/ui/Modal.svelte';
  import Button from '$lib/ui/Button.svelte';
  import Input from '$lib/ui/Input.svelte';
  import Select from '$lib/ui/Select.svelte';
  import Badge from '$lib/ui/Badge.svelte';
  import Icon from '$lib/ui/Icon.svelte';
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import ApiEditor from './ApiEditor.svelte';
  import HttpEditor from './HttpEditor.svelte';
  import AgentEditor from './AgentEditor.svelte';
  import NotifyEditor from './NotifyEditor.svelte';
  import IfEditor from './IfEditor.svelte';
  import TransformEditor from './TransformEditor.svelte';
  import { automatorApi, KIND_META, STATUS_TONE } from '$lib/modules/automator/api.js';
  import { t } from '$lib/i18n';

  let {
    open = $bindable(false),
    node = null,
    catalog = null,
    sources = [],
    workflowId = '',
    onapply = () => {},
    onremove = () => {}
  } = $props();

  // A working copy: the canvas only hears about the change when Save is pressed.
  let draft = $state(null);
  let testing = $state(false);
  let result = $state(null);
  let error = $state('');

  // Only the identity of the block reloads the draft. Reading its contents here would
  // subscribe to them, and any later touch of that node would wipe what is being typed.
  $effect(() => {
    if (!open || !node) return;
    untrack(() => {
      const copy = structuredClone($state.snapshot(node));
      // A block written by an older revision can miss a field this form binds to, and an
      // undefined bound value is a fatal render error. The defaults fill the gaps.
      copy.config = { ...(KIND_META[copy.kind]?.defaults() ?? {}), ...(copy.config ?? {}) };
      draft = copy;
      result = null;
      error = '';
    });
  });

  async function test() {
    testing = true;
    result = null;
    error = '';
    try {
      result = await automatorApi.testNode({
        node: $state.snapshot(draft),
        workflow_id: workflowId || null
      });
    } catch (e) {
      error = e.message;
    } finally {
      testing = false;
    }
  }

  function apply() {
    onapply($state.snapshot(draft));
    open = false;
  }
</script>

<Modal bind:open title={draft ? $t(`automator.kind.${draft.kind}`) : ''} size="lg">
  {#if draft}
    <div class="form">
      <div class="idrow">
        <Input label={$t('automator.node.name')} bind:value={draft.name} placeholder={draft.id} />
        <div class="idbox">
          <span class="lbl">{$t('automator.node.id')}</span>
          <code>{draft.id}</code>
          <em>{$t('automator.node.idHint')}</em>
        </div>
      </div>

      <!-- Keyed on the block: two tasks of the same kind would otherwise share one editor
           instance, and its local state (the API body text, the transform field rows) would
           follow from the first block into the second. -->
      {#key draft.id}
        {#if draft.kind === 'api'}
          <ApiEditor bind:config={draft.config} {catalog} {sources} />
        {:else if draft.kind === 'http'}
          <HttpEditor bind:config={draft.config} {catalog} {sources} />
        {:else if draft.kind === 'agent'}
          <AgentEditor bind:config={draft.config} {sources} />
        {:else if draft.kind === 'notify'}
          <NotifyEditor bind:config={draft.config} {sources} />
        {:else if draft.kind === 'if'}
          <IfEditor bind:config={draft.config} {catalog} {sources} />
        {:else if draft.kind === 'transform'}
          <TransformEditor bind:config={draft.config} {catalog} {sources} />
        {:else if draft.kind === 'delay'}
          <Input
            label={$t('automator.delay.seconds')}
            type="number"
            min="1"
            max="300"
            bind:value={draft.config.seconds}
          />
        {/if}
      {/key}

      <details class="advanced">
        <summary>{$t('automator.node.advanced')}</summary>
        <div class="advgrid">
          <Select
            label={$t('automator.node.onError')}
            options={[
              { value: 'stop', label: $t('automator.node.onErrorStop') },
              { value: 'continue', label: $t('automator.node.onErrorContinue') }
            ]}
            bind:value={draft.on_error}
          />
          <Input
            label={$t('automator.node.retries')}
            type="number"
            min="0"
            max="5"
            bind:value={draft.retry.count}
          />
          <Input
            label={$t('automator.node.backoff')}
            type="number"
            min="1"
            max="300"
            bind:value={draft.retry.backoff_secs}
          />
          <Input
            label={$t('automator.node.timeout')}
            type="number"
            min="1"
            max="600"
            bind:value={draft.timeout_secs}
            placeholder={$t('automator.node.timeoutDefault')}
          />
        </div>
        <p class="hint">{$t('automator.node.onErrorHint')}</p>
      </details>

      <div class="testpanel">
        <div class="testhead">
          <Button icon="play" loading={testing} onclick={test}>{$t('automator.node.test')}</Button>
          <span class="hint">{$t('automator.node.testHint')}</span>
        </div>
        <ErrorText error={error} />
        {#if result}
          <div class="testresult">
            <Badge tone={STATUS_TONE[result.status] ?? 'neutral'}>
              {$t(`automator.status.${result.status}`)}
            </Badge>
            {#if result.error}
              <pre class="err">{result.error}</pre>
            {/if}
            {#if result.request}
              <p class="lbl">{$t('automator.node.request')}</p>
              <pre>{JSON.stringify(result.request, null, 2)}</pre>
            {/if}
            {#if result.output !== undefined && result.output !== null}
              <p class="lbl">{$t('automator.node.output')}</p>
              <pre>{JSON.stringify(result.output, null, 2)}</pre>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {/if}

  {#snippet footer()}
    <button
      class="danger"
      onclick={() => {
        onremove(node?.id);
        open = false;
      }}
    >
      <Icon name="trash" size={13} />
      {$t('automator.node.remove')}
    </button>
    <Button onclick={() => (open = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={apply}>{$t('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }
  .idrow {
    display: flex;
    gap: var(--space-3);
    align-items: flex-end;
  }
  .idrow :global(> *:first-child) {
    flex: 1;
  }
  .idbox {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .idbox code {
    font-family: var(--mono);
    font-size: var(--text-xs);
  }
  .idbox em {
    font-style: normal;
    font-size: 10px;
    color: var(--muted);
  }
  .lbl {
    font-size: var(--text-xs);
    color: var(--dim);
    margin: var(--space-2) 0 var(--space-1);
  }
  .advanced summary {
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--dim);
  }
  .advgrid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: var(--space-3);
    margin-top: var(--space-3);
  }
  .hint {
    margin: var(--space-2) 0 0;
    font-size: var(--text-xs);
    color: var(--muted);
  }
  .testpanel {
    border-top: 0.5px solid var(--border);
    padding-top: var(--space-3);
  }
  .testhead {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  pre {
    background: var(--surface-2);
    padding: var(--space-2);
    font-size: 10px;
    max-height: 220px;
    overflow: auto;
    margin: 0;
  }
  pre.err {
    color: var(--red);
  }
  .danger {
    margin-right: auto;
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: 0;
    color: var(--red);
    font-size: var(--text-sm);
    cursor: pointer;
  }
</style>

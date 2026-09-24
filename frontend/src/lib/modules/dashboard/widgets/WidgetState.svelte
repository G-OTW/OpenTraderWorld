<script>
  // The five states every widget owes its reader, in one place: edit-mode preview,
  // error, loading, empty, populated. Before this each widget re-declared the same
  // {#if editing}…{:else if err}…{:else if x === null}… ladder plus its own `.hint`
  // rule, and they drifted — different skeleton shapes, different empty colours,
  // one of them still rendering its empty text in red.
  //
  //   <WidgetState {editing} {error} loading={rows === null} empty={rows?.length === 0}
  //     preview={$t('…preview')} emptyText={$t('…empty')}>
  //     …populated markup…
  //   </WidgetState>
  //
  // `rows` shapes the skeleton to the content that is coming (a list gets list-shaped
  // bones), so the layout does not jump when the data lands.
  import ErrorText from '$lib/ui/ErrorText.svelte';
  import Skeleton from '$lib/ui/Skeleton.svelte';

  let {
    editing = false,
    preview = '',
    error = '',
    loading = false,
    empty = false,
    emptyText = '',
    rows = 3,
    children
  } = $props();
</script>

{#if editing}
  <p class="w-state">{preview}</p>
{:else if error}
  <ErrorText {error} compact />
{:else if loading}
  <div class="w-skeleton" aria-busy="true">
    <Skeleton {rows} height="1rem" gap="10px" />
  </div>
{:else if empty}
  <p class="w-state">{emptyText}</p>
{:else}
  {@render children?.()}
{/if}

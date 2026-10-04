<script>
  // Accessible underline tabs. Controlled via bindable `value`.
  //   <Tabs tabs={[{id:'a',label:'A'},…]} bind:value={tab} />
  // Styling comes from the global `.tabs` class (theme/components.css); this
  // component owns the roving-tabindex keyboard behavior and ARIA wiring.
  let { tabs = [], value = $bindable(tabs[0]?.id), ariaLabel = 'Tabs' } = $props();

  let els = $state([]);

  function onKeydown(e, i) {
    const last = tabs.length - 1;
    let next = null;
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = i === last ? 0 : i + 1;
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = i === 0 ? last : i - 1;
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = last;
    if (next === null) return;
    e.preventDefault();
    value = tabs[next].id;
    els[next]?.focus();
  }
</script>

<div class="tabs" role="tablist" aria-label={ariaLabel}>
  {#each tabs as tab, i (tab.id)}
    <button
      bind:this={els[i]}
      role="tab"
      id="tab-{tab.id}"
      aria-selected={value === tab.id}
      aria-controls="panel-{tab.id}"
      tabindex={value === tab.id ? 0 : -1}
      onclick={() => (value = tab.id)}
      onkeydown={(e) => onKeydown(e, i)}
    >
      {tab.label}
    </button>
  {/each}
</div>

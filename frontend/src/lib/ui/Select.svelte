<script>
  import Field from './Field.svelte';

  // Labelled native <select>, matched to Input. For a searchable / large-list
  // picker use ComboSelect instead — these stay separate on purpose: a native
  // select is the right control for a short, known set (it gets the OS picker on
  // mobile for free), and ComboSelect is the right one when you must type to find.
  //
  //   <Select label="Currency" options={[{value:'EUR',label:'Euro'}]} bind:value={ccy} />
  // props: value (bindable), options ([{value,label,disabled}] or string[]),
  //        label, hint, error, id, required, disabled, placeholder
  let {
    value = $bindable(''),
    options = [],
    label = '',
    hint = '',
    error = '',
    id = `sel-${Math.random().toString(36).slice(2, 9)}`,
    required = false,
    disabled = false,
    placeholder = '',
    ...rest
  } = $props();

  // Accept both `['EUR','USD']` and `[{value,label}]`.
  const items = $derived(
    options.map((o) => (typeof o === 'object' && o !== null ? o : { value: o, label: String(o) }))
  );
  const describedBy = $derived(error || hint ? `${id}-msg` : undefined);
</script>

<Field {id} {label} {hint} {error} {required}>
  <select
    {id}
    {disabled}
    {required}
    bind:value
    aria-invalid={!!error}
    aria-describedby={describedBy}
    {...rest}
  >
    {#if placeholder}
      <option value="" disabled>{placeholder}</option>
    {/if}
    {#each items as opt (opt.value)}
      <option value={opt.value} disabled={opt.disabled}>{opt.label}</option>
    {/each}
  </select>
</Field>

<style>
  /* Fill the field width; the global layer owns the rest of the look. */
  select {
    width: 100%;
  }
</style>

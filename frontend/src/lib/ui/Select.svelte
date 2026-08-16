<script>
  import Field from './Field.svelte';
  import Dropdown from './Dropdown.svelte';

  // Labelled single-select, matched to Input. Renders the themed Dropdown, not a native
  // <select>: the OS draws the native popup with its own palette, which reads as a foreign
  // light-grey menu over the dark UI. For a searchable / large-list picker use ComboSelect —
  // that one is the right control when you must type to find.
  //
  //   <Select label="Currency" options={[{value:'EUR',label:'Euro'}]} bind:value={ccy} />
  // props: value (bindable), options ([{value,label}] or string[]),
  //        label, hint, error, id, required, disabled, placeholder, onpick
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
    onpick = null
  } = $props();

  // Accept both `['EUR','USD']` and `[{value,label}]`. Values keep their type (an interval
  // in seconds stays a number), so callers binding a number still get a number back.
  const items = $derived(
    options.map((o) => (typeof o === 'object' && o !== null ? o : { value: o, label: String(o) }))
  );
</script>

<Field {id} {label} {hint} {error} {required}>
  <Dropdown
    bind:value
    options={items}
    {placeholder}
    {disabled}
    {onpick}
    ariaLabel={label || placeholder || undefined}
  />
</Field>

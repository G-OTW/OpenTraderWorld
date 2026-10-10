<script>
  import Field from './Field.svelte';

  // Labelled text/number/date input. The element itself is already on-theme via
  // the global control layer; this adds the label, the hint/error, and the a11y
  // wiring. Numbers get tabular figures so columns of them line up.
  //
  //   <Input label="Entry price" type="number" bind:value={price} />
  //   <Input label="Email" type="email" error={emailError} required />
  // props: value (bindable), label, hint, error, type, id, required, disabled,
  //        placeholder, multiline, rows
  let {
    value = $bindable(''),
    label = '',
    hint = '',
    error = '',
    type = 'text',
    id = `in-${Math.random().toString(36).slice(2, 9)}`,
    required = false,
    disabled = false,
    placeholder = '',
    multiline = false,
    rows = 3,
    ...rest
  } = $props();

  const describedBy = $derived(error || hint ? `${id}-msg` : undefined);
  const numeric = $derived(type === 'number');
</script>

<Field {id} {label} {hint} {error} {required}>
  {#if multiline}
    <textarea
      {id}
      {rows}
      {disabled}
      {placeholder}
      {required}
      bind:value
      aria-invalid={!!error}
      aria-describedby={describedBy}
      {...rest}
    ></textarea>
  {:else}
    <!-- `type` can't be spread onto a bound input in Svelte, so branch on the
         cases the app actually uses rather than fight the compiler. -->
    {#if numeric}
      <input
        {id}
        type="number"
        class="num"
        {disabled}
        {placeholder}
        {required}
        bind:value
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...rest}
      />
    {:else if type === 'date'}
      <input
        {id}
        type="date"
        {disabled}
        {required}
        bind:value
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...rest}
      />
    {:else if type === 'password'}
      <input
        {id}
        type="password"
        {disabled}
        {placeholder}
        {required}
        bind:value
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...rest}
      />
    {:else if type === 'email'}
      <input
        {id}
        type="email"
        {disabled}
        {placeholder}
        {required}
        bind:value
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...rest}
      />
    {:else}
      <input
        {id}
        type="text"
        {disabled}
        {placeholder}
        {required}
        bind:value
        aria-invalid={!!error}
        aria-describedby={describedBy}
        {...rest}
      />
    {/if}
  {/if}
</Field>

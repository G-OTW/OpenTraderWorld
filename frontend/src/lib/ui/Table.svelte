<script>
  import Icon from './Icon.svelte';

  // Dense data table. Row height comes from --row-h, never from cell padding —
  // that is what keeps a long table scannable. Rows are separated by hairlines,
  // not zebra stripes: stripes fight with the red/green PnL semantics.
  //
  //   <Table columns={cols} rows={trades} sort={sort} onsort={setSort}>
  //     {#snippet cell(row, col)} … {/snippet}
  //   </Table>
  //
  // columns: [{ key, label, align?: 'left'|'right', numeric?, sortable?, width? }]
  // Numeric columns right-align and get tabular figures — prices only line up
  // when both are true.
  // props: columns, rows, rowKey, sort ({key,dir}), onsort, comfortable, cell, empty
  let {
    columns = [],
    rows = [],
    rowKey = (r, i) => r?.id ?? i,
    sort = null,
    onsort = undefined,
    comfortable = false,
    cell,
    empty
  } = $props();

  function align(col) {
    return col.align ?? (col.numeric ? 'right' : 'left');
  }

  function nextDir(col) {
    if (sort?.key !== col.key) return 'asc';
    return sort.dir === 'asc' ? 'desc' : 'asc';
  }

  function headerSort(col) {
    if (!col.sortable || !onsort) return;
    onsort({ key: col.key, dir: nextDir(col) });
  }

  function ariaSort(col) {
    if (!col.sortable) return undefined;
    if (sort?.key !== col.key) return 'none';
    return sort.dir === 'asc' ? 'ascending' : 'descending';
  }
</script>

<div class="wrap" class:comfortable>
  <table>
    <thead>
      <tr>
        {#each columns as col (col.key)}
          <th
            scope="col"
            style:text-align={align(col)}
            style:width={col.width ?? null}
            aria-sort={ariaSort(col)}
          >
            {#if col.sortable && onsort}
              <button class="sort" onclick={() => headerSort(col)}>
                <span>{col.label}</span>
                <span class="arrow" class:active={sort?.key === col.key}>
                  <Icon
                    name={sort?.key === col.key && sort.dir === 'desc' ? 'arrow-down' : 'arrow-up'}
                    size={12}
                  />
                </span>
              </button>
            {:else}
              {col.label}
            {/if}
          </th>
        {/each}
      </tr>
    </thead>

    <tbody>
      {#if rows.length === 0}
        <tr class="empty-row">
          <td colspan={columns.length}>{@render empty?.()}</td>
        </tr>
      {:else}
        {#each rows as row, i (rowKey(row, i))}
          <tr>
            {#each columns as col (col.key)}
              <td style:text-align={align(col)} class:num={col.numeric}>
                {#if cell}{@render cell(row, col)}{:else}{row[col.key] ?? ''}{/if}
              </td>
            {/each}
          </tr>
        {/each}
      {/if}
    </tbody>
  </table>
</div>

<style>
  /* Horizontal overflow scrolls inside the table, never the page. */
  .wrap {
    width: 100%;
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  thead th {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    background: var(--surface);
    color: var(--muted);
    font-size: var(--text-xs);
    font-weight: var(--fw-medium);
    line-height: var(--lh-tight);
    text-align: left;
    white-space: nowrap;
    padding: 0 var(--space-3);
    height: var(--row-h);
    border-bottom: 1px solid var(--border);
  }

  .sort {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: none;
    border: none;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  /* The arrow is a persistent affordance: invisible until hover or active, so
     the header reads clean but the column announces it can be sorted. */
  .arrow {
    display: inline-flex;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease);
  }
  .sort:hover .arrow,
  .arrow.active {
    opacity: 1;
  }
  .arrow.active {
    color: var(--accent);
  }

  tbody td {
    height: var(--row-h);
    padding: 0 var(--space-3);
    color: var(--text);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
  }
  .comfortable thead th,
  .comfortable tbody td {
    height: var(--row-h-lg);
  }

  tbody tr:last-child td {
    border-bottom: none;
  }
  tbody tr:hover td {
    background: var(--surface-2);
  }

  .empty-row td {
    height: auto;
    padding: 0;
    border-bottom: none;
  }
  .empty-row td:hover {
    background: transparent;
  }
</style>

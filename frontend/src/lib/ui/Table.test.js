// The shared data table: sortable headers expose their state to assistive technology,
// a click or a keyboard press flips the order, and an empty list shows the caller's empty
// state across the whole row.
import { expect, test } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import TradesTableHarness from '../../test/harness/TradesTableHarness.svelte';

const MINUS = '\u2212';
const TRADES = [
  { id: 1, ticker: 'MSFT', qty: 5, pnl: -52 },
  { id: 2, ticker: 'AAPL', qty: 10, pnl: 98 },
  { id: 3, ticker: 'NVDA', qty: 2, pnl: 1234.5 }
];

const bodyRows = () => screen.getAllByRole('row').slice(1);
const column = (i) => bodyRows().map((r) => within(r).getAllByRole('cell')[i].textContent.trim());

test('headers are column headers; only sortable ones carry aria-sort', () => {
  render(TradesTableHarness, { trades: TRADES });
  const headers = screen.getAllByRole('columnheader');
  expect(headers.map((h) => h.textContent.trim())).toEqual(['Ticker', 'Qty', 'Net PnL']);
  expect(headers.map((h) => h.getAttribute('aria-sort'))).toEqual(['ascending', null, 'none']);
  // A non-sortable header is plain text, not a control.
  expect(within(headers[1]).queryByRole('button')).toBeNull();
});

test('rows render formatted money, numbers right-aligned', () => {
  render(TradesTableHarness, { trades: TRADES });
  expect(column(0)).toEqual(['AAPL', 'MSFT', 'NVDA']);
  expect(column(2)).toEqual(['+$98.00', `${MINUS}$52.00`, '+$1,234.50']);
  const pnlCell = within(bodyRows()[0]).getAllByRole('cell')[2];
  expect(pnlCell).toHaveStyle({ textAlign: 'right' });
});

test('clicking a header sorts ascending, a second click descending', async () => {
  const user = userEvent.setup();
  render(TradesTableHarness, { trades: TRADES });
  await user.click(screen.getByRole('button', { name: 'Net PnL' }));
  expect(column(0)).toEqual(['MSFT', 'AAPL', 'NVDA']);
  expect(screen.getByRole('columnheader', { name: 'Net PnL' })).toHaveAttribute('aria-sort', 'ascending');
  expect(screen.getByRole('columnheader', { name: 'Ticker' })).toHaveAttribute('aria-sort', 'none');
  await user.click(screen.getByRole('button', { name: 'Net PnL' }));
  expect(column(0)).toEqual(['NVDA', 'AAPL', 'MSFT']);
  expect(screen.getByRole('columnheader', { name: 'Net PnL' })).toHaveAttribute('aria-sort', 'descending');
});

test('the sort works from the keyboard', async () => {
  const user = userEvent.setup();
  render(TradesTableHarness, { trades: TRADES });
  await user.tab();
  expect(screen.getByRole('button', { name: 'Ticker' })).toHaveFocus();
  await user.keyboard('{Enter}');
  expect(column(0)).toEqual(['NVDA', 'MSFT', 'AAPL']);
});

test('an empty list shows the empty state spanning every column', () => {
  render(TradesTableHarness, { trades: [] });
  const rows = bodyRows();
  expect(rows).toHaveLength(1);
  const cell = within(rows[0]).getByRole('cell');
  expect(cell).toHaveTextContent('No trades yet.');
  expect(cell).toHaveAttribute('colspan', '3');
});

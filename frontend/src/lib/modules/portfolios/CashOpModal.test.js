// Booking a cash row on a portfolio: what the form lets through, the exact body it sends,
// and what the user sees when the server refuses.
import { beforeEach, describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { http, HttpResponse } from 'msw';
import { server } from '../../../mocks/node.js';
import { apiError } from '../../../mocks/handlers.js';
import CashOpModal from './CashOpModal.svelte';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));

const ASSETS = [
  { id: 'a-1', symbol: 'AAPL' },
  { id: 'a-2', symbol: 'MSFT' }
];

let posted;
beforeEach(() => {
  vi.useFakeTimers({ now: new Date('2026-03-14T12:00:00Z'), toFake: ['Date'] });
  posted = [];
  server.use(
    http.post('*/api/portfolios/:id/operations', async ({ request, params }) => {
      posted.push({ id: params.id, body: await request.json() });
      return HttpResponse.json({ operation: { id: 'op-1' } });
    })
  );
});

function open(props = {}) {
  const onsaved = vi.fn();
  const onclose = vi.fn();
  const user = userEvent.setup();
  render(CashOpModal, { portfolioId: 'pf-1', assets: ASSETS, currency: 'EUR', onsaved, onclose, ...props });
  return { user, onsaved, onclose };
}

const bookButton = () => screen.getByRole('button', { name: 'Book it' });
// Amount is the first number field of a cash row, Fee the second (see the label bug below).
const numberFields = () => screen.getAllByRole('spinbutton');

describe('cash operation form', () => {
  test('the dialog is named and Book it stays disabled until an amount is typed', () => {
    open();
    expect(screen.getByRole('dialog', { name: 'Book cash, income or a cost' })).toBeInTheDocument();
    expect(bookButton()).toBeDisabled();
    expect(screen.getByText('Adds 0 EUR to your cash.')).toBeInTheDocument();
  });

  test('a deposit sends the typed amount, fee, note, date and currency', async () => {
    const { user, onsaved, onclose } = open();
    const [amount, fee] = numberFields();
    await user.type(amount, '1500.25');
    await user.type(fee, '2.5');
    await user.type(screen.getByRole('textbox'), '  March top-up  ');
    expect(screen.getByText('Adds 1500.25 EUR to your cash.')).toBeInTheDocument();
    await user.click(bookButton());
    expect(posted).toEqual([
      {
        id: 'pf-1',
        body: {
          side: 'deposit',
          op_date: '2026-03-14',
          amount: 1500.25,
          fee: 2.5,
          note: 'March top-up',
          currency: 'EUR'
        }
      }
    ]);
    expect(onsaved).toHaveBeenCalledOnce();
    expect(onclose).toHaveBeenCalledOnce();
  });

  test('zero is not an amount: nothing is sent', async () => {
    const { user } = open();
    await user.type(numberFields()[0], '0');
    expect(bookButton()).toBeDisabled();
    await user.click(bookButton());
    expect(posted).toEqual([]);
  });

  test('a withdrawal warns that money leaves the cash', async () => {
    const { user } = open();
    await user.click(screen.getByRole('button', { name: /Withdrawal/ }));
    await user.type(numberFields()[0], '300');
    expect(screen.getByText('Takes 300 EUR out of your cash.')).toBeInTheDocument();
    await user.click(bookButton());
    expect(posted[0].body.side).toBe('withdraw');
  });

  test('a refusal is shown in the dialog, which stays open for a retry', async () => {
    server.use(
      http.post('*/api/portfolios/:id/operations', () => apiError(400, 'amount exceeds the cash balance'))
    );
    const { user, onsaved, onclose } = open();
    await user.type(numberFields()[0], '999999');
    await user.click(bookButton());
    expect(await screen.findByRole('alert')).toHaveTextContent('amount exceeds the cash balance');
    expect(onsaved).not.toHaveBeenCalled();
    expect(onclose).not.toHaveBeenCalled();
    expect(bookButton()).toBeEnabled();
  });

  test('a split needs a holding and both ratio terms, and sends no cash amount', async () => {
    const { user } = open();
    await user.click(screen.getByRole('button', { name: 'Split' }));
    expect(bookButton()).toBeDisabled();
    const [num, den] = numberFields();
    await user.type(num, '4');
    await user.type(den, '1');
    expect(bookButton()).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Pick a holding' }));
    await user.click(screen.getByRole('option', { name: 'AAPL' }));
    expect(bookButton()).toBeEnabled();
    await user.click(bookButton());
    expect(posted[0].body).toEqual({
      side: 'split',
      op_date: '2026-03-14',
      asset_id: 'a-1',
      ratio_num: 4,
      ratio_den: 1,
      note: ''
    });
  });

  test('every CashOpModal field is named by its label', () => {
    open();
    expect(screen.getByLabelText('Amount')).toBeInTheDocument();
    expect(screen.getByLabelText('Fee')).toBeInTheDocument();
    expect(screen.getByLabelText('Note')).toBeInTheDocument();
  });
});

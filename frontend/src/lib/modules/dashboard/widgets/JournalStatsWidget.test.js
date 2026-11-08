// The journal Net PnL card: the five states every data view owes its reader (loading,
// empty, error, partial, success), read the way a user sees them. Data comes from the
// shared MSW fixtures; expected text is written from those literals by hand.
import { describe, expect, test, vi } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { http, HttpResponse, delay } from 'msw';
import { server } from '../../../../mocks/node.js';
import { apiError } from '../../../../mocks/handlers.js';
import { breakdown, emptyBreakdown } from '../../../../mocks/fixtures.js';
import JournalStatsWidget from './JournalStatsWidget.svelte';

vi.mock('$app/navigation', () => ({ goto: vi.fn() }));

const MINUS = '\u2212';
const pnlCard = (config = {}) => ({ item: { config: { variant: 'pnl', ...config } }, editing: false });
const answer = (body) =>
  server.use(http.get('*/api/journal/breakdown', () => HttpResponse.json({ breakdown: body })));

describe('journal Net PnL card', () => {
  test('loading: a busy skeleton until the breakdown lands, then the figure', async () => {
    let release;
    const gate = new Promise((r) => (release = r));
    server.use(
      http.get('*/api/journal/breakdown', async () => {
        await gate;
        return HttpResponse.json({ breakdown: breakdown() });
      })
    );
    const { container } = render(JournalStatsWidget, pnlCard());
    expect(container.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(screen.queryByText('+$1,234.50')).toBeNull();
    release();
    expect(await screen.findByText('+$1,234.50')).toBeInTheDocument();
    expect(container.querySelector('[aria-busy="true"]')).toBeNull();
  });

  test('success: signed net PnL, signed return and the cumulative sparkline', async () => {
    render(JournalStatsWidget, pnlCard());
    expect(await screen.findByText('+$1,234.50')).toBeInTheDocument();
    expect(screen.getByText('+12.50%')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Cumulative' })).toBeInTheDocument();
  });

  test('a losing book shows the true minus in the figure and the return', async () => {
    answer(breakdown({ realized_pnl: -52, return_pct: -0.52 }));
    render(JournalStatsWidget, pnlCard());
    expect(await screen.findByText(`${MINUS}$52.00`)).toBeInTheDocument();
    expect(screen.getByText(`${MINUS}0.52%`)).toBeInTheDocument();
  });

  test('empty: a book with no trade says so instead of drawing zeros', async () => {
    answer(emptyBreakdown());
    render(JournalStatsWidget, pnlCard());
    expect(await screen.findByText('No trades in this period.')).toBeInTheDocument();
    expect(screen.queryByText('+$0.00')).toBeNull();
  });

  test('error: the server message is announced and no figure is drawn', async () => {
    server.use(http.get('*/api/journal/breakdown', () => apiError(500, 'database unavailable')));
    render(JournalStatsWidget, pnlCard());
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent('database unavailable');
    expect(screen.queryByText(/\$/)).toBeNull();
  });

  test('partial: no return and a one-point curve show the figure alone', async () => {
    answer(
      breakdown({
        return_pct: null,
        equity_curve: [{ at: '2026-03-02T15:00:00Z', equity: 10500, cum_pnl: 500 }]
      })
    );
    render(JournalStatsWidget, pnlCard());
    expect(await screen.findByText('+$1,234.50')).toBeInTheDocument();
    expect(screen.queryByText(/%$/)).toBeNull();
    expect(screen.queryByRole('img', { name: 'Cumulative' })).toBeNull();
  });

  test('amounts are in the display currency the server reports', async () => {
    answer(breakdown({ display_currency: 'EUR', realized_pnl: 98 }));
    render(JournalStatsWidget, pnlCard());
    expect(await screen.findByText('+€98.00')).toBeInTheDocument();
  });

  test('the card forwards its filter to the breakdown query', async () => {
    let query = null;
    server.use(
      http.get('*/api/journal/breakdown', async ({ request }) => {
        query = Object.fromEntries(new URL(request.url).searchParams);
        await delay(0);
        return HttpResponse.json({ breakdown: breakdown() });
      })
    );
    render(JournalStatsWidget, pnlCard({ categoryId: 'cat-2', since: '2026-01-01' }));
    await screen.findByText('+$1,234.50');
    expect(query).toEqual({ category_id: 'cat-2', since: '2026-01-01' });
  });

  test('edit mode shows the preview and asks nothing of the server', async () => {
    let calls = 0;
    server.use(
      http.get('*/api/journal/*', () => {
        calls += 1;
        return HttpResponse.error();
      })
    );
    render(JournalStatsWidget, { item: { config: { variant: 'pnl' } }, editing: true });
    expect(screen.getByText('Journal analytics preview')).toBeInTheDocument();
    // Let any effect that would fetch run to completion before counting.
    await Promise.resolve();
    expect(calls).toBe(0);
  });
});

// Fixed API payloads shared by the MSW handlers and the tests that assert on them.
//
// Shapes follow the backend serializers (core/otw-store/src/journal.rs `Breakdown`,
// `EquityPoint`; journal_api.rs list routes). Numbers are small and hand-checkable: the
// expected text in a test comes from these literals, never from the code under test.

/** Breakdown builder: every field present, overridable one by one. */
export function breakdown(overrides = {}) {
  return {
    invested_capital: 10000,
    realized_pnl: 1234.5,
    total_fees: 12,
    current_equity: 11234.5,
    return_pct: 12.5,
    trade_count: 3,
    closed_count: 3,
    open_count: 0,
    win_count: 2,
    loss_count: 1,
    win_rate: 66.6667,
    avg_win: 750,
    avg_loss: -265.5,
    profit_factor: 5.6497,
    expectancy: 411.5,
    sharpe: 1.2,
    sortino: 1.8,
    max_drawdown: -265.5,
    best_trade: 1000,
    worst_trade: -265.5,
    peak_margin: 5000,
    return_on_margin: 24.69,
    equity_curve: [
      { at: '2026-03-02T15:00:00Z', equity: 10500, cum_pnl: 500 },
      { at: '2026-03-03T15:00:00Z', equity: 10234.5, cum_pnl: 234.5 },
      { at: '2026-03-04T15:00:00Z', equity: 11234.5, cum_pnl: 1234.5 }
    ],
    display_currency: 'USD',
    unconverted_trades: 0,
    ...overrides
  };
}

/** A book with no trade at all: what a fresh install answers. */
export const emptyBreakdown = () =>
  breakdown({
    realized_pnl: 0,
    total_fees: 0,
    current_equity: 10000,
    return_pct: null,
    trade_count: 0,
    closed_count: 0,
    win_count: 0,
    loss_count: 0,
    win_rate: null,
    avg_win: null,
    avg_loss: null,
    profit_factor: null,
    expectancy: null,
    sharpe: null,
    sortino: null,
    max_drawdown: null,
    best_trade: null,
    worst_trade: null,
    return_on_margin: null,
    equity_curve: []
  });

export const categories = [
  { id: 'cat-1', name: 'Futures' },
  { id: 'cat-2', name: 'Swing' }
];

export const templates = [{ id: 'tpl-1', name: 'ES scalp' }];

/** USD-based rates for one date, as /api/journal/fx/rates/{date} serves them. */
export const fxRows = [
  { quote: 'EUR', rate: 0.8 },
  { quote: 'JPY', rate: 150 }
];

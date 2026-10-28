-- Portfolio Tracker: delete the snapshots that recorded a book holding assets as worth nothing.
--
-- The rebuild used to write a day even when it could not price a single holding, so a
-- portfolio whose bars had never been downloaded got one zero-valued row per calendar day.
-- Those rows are not a flat curve, they are a hole: the day after one is a -100% return, the
-- drawdown is total, the time-weighted return pins at -100% and every measure built on the
-- curve (Sharpe, Calmar, drag, average net worth) reads off that. `history::rebuild` now
-- skips a day it cannot value; this drops the ones already stored.
--
-- The condition is exact: a book carrying cost basis owned something that day, and something
-- is never worth exactly zero. A genuinely empty book has no basis either and is left alone.
DELETE FROM portfolio_snapshots
WHERE market_value = 0 AND cost_basis > 0;

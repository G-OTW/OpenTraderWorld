-- Backtest filters: repair `on_window_end` values the engine refuses.
--
-- The engine has only ever accepted "hold" or "flat", but an early strategy preset shipped
-- "close" (the word the UI once used for flattening at the window edge). A saved strategy is
-- the worked example everything copies from, the agent included, so one bad preset turns into
-- a 400 on every run composed from it: `filters: on_window_end must be "hold" or "flat"`.
--
-- Anything that is not "hold" meant "close the position when the window ends", which is
-- "flat". Runs and paper sessions are rewritten too, since both are replayed from the same
-- settings object: a row the engine cannot read is a dead result, not just a stale one.

UPDATE backtest_strategies
   SET settings = jsonb_set(settings, '{filters,on_window_end}', '"flat"')
 WHERE settings -> 'filters' ? 'on_window_end'
   AND settings -> 'filters' ->> 'on_window_end' NOT IN ('hold', 'flat');

UPDATE backtest_runs
   SET settings = jsonb_set(settings, '{filters,on_window_end}', '"flat"')
 WHERE settings -> 'filters' ? 'on_window_end'
   AND settings -> 'filters' ->> 'on_window_end' NOT IN ('hold', 'flat');

UPDATE paper_sessions
   SET settings = jsonb_set(settings, '{filters,on_window_end}', '"flat"')
 WHERE settings -> 'filters' ? 'on_window_end'
   AND settings -> 'filters' ->> 'on_window_end' NOT IN ('hold', 'flat');

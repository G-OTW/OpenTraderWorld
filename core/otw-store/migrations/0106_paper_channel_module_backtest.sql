-- Paper trading notifies as the module it lives in.
--
-- The grant list is read as module ids, and the channel screen labels them from the
-- module registry: 'paper' matched nothing there and showed up raw. Paper sessions are
-- part of Backtest, so the producer id is 'backtest' and an existing grant follows.

UPDATE channel_modules SET module = 'backtest' WHERE module = 'paper'
  AND NOT EXISTS (
      SELECT 1 FROM channel_modules c2
      WHERE c2.channel_id = channel_modules.channel_id AND c2.module = 'backtest'
  );
DELETE FROM channel_modules WHERE module = 'paper';

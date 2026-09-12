-- Trade tags (mistakes / rules followed / setups) and the planned stop that makes
-- R-multiples computable.
--
-- Tags are the discipline loop: a `mistake` tag marks a rule breach, a `rule` tag marks
-- a rule honoured, a `setup` tag is neutral classification. Stats group by tag exactly
-- like they group by strategy, so "cost of mistakes" is a real number, not a feeling.
--
-- `stop_price` is the planned stop for a *simple* trade (advanced trades already carry
-- SL brackets). Risk = |avg entry - stop| x qty x multiplier; R = net PnL / risk. Both
-- are read from the user's own plan, never inferred from bars.

CREATE TABLE IF NOT EXISTS journal_tags (
    id          UUID PRIMARY KEY,
    name        TEXT NOT NULL,
    -- mistake | rule | setup
    kind        TEXT NOT NULL DEFAULT 'mistake' CHECK (kind IN ('mistake', 'rule', 'setup')),
    color       TEXT,
    description TEXT,
    position    DOUBLE PRECISION NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One tag name overall: the same label meaning two things in two lists would make the
-- per-tag stats unreadable.
CREATE UNIQUE INDEX IF NOT EXISTS idx_journal_tags_name ON journal_tags(lower(name));

CREATE TABLE IF NOT EXISTS journal_trade_tags (
    trade_id UUID NOT NULL REFERENCES journal_trades(id) ON DELETE CASCADE,
    tag_id   UUID NOT NULL REFERENCES journal_tags(id) ON DELETE CASCADE,
    PRIMARY KEY (trade_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_journal_trade_tags_tag ON journal_trade_tags(tag_id);

ALTER TABLE journal_trades
    ADD COLUMN IF NOT EXISTS stop_price DOUBLE PRECISION;

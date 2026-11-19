-- The journal a chart instrument books its trades into, picked on the order ticket.
--
-- A link is a period: from `since` until `until` (open while NULL), every position whose
-- opening fill falls inside it is filed into `category_id`, whatever closes it (the ticket,
-- the pane's buttons, a bracket, the broker directly). Picking another journal ends the
-- current link and starts the next one; picking none only ends it. A position open when the
-- link ends stays in its journal until it is flat.
CREATE TABLE journal_chart_links (
    id          UUID PRIMARY KEY,
    ticker      TEXT NOT NULL,
    asset_type  TEXT NOT NULL,
    category_id UUID NOT NULL REFERENCES journal_categories(id) ON DELETE CASCADE,
    since       TIMESTAMPTZ NOT NULL,
    until       TIMESTAMPTZ,
    CHECK (until IS NULL OR until >= since)
);
CREATE INDEX idx_journal_chart_links_instrument ON journal_chart_links(ticker, asset_type, since);
CREATE UNIQUE INDEX idx_journal_chart_links_open ON journal_chart_links(ticker, asset_type) WHERE until IS NULL;

-- Where each trading account stands on a linked instrument.
--
-- `resume_from`: where the next read of fills starts (the opening of the oldest position
-- still open, or the last read when flat). `seeds`: positions the account already held when
-- the instrument was first linked, so their closes are not read as new positions; they are
-- never booked. `mark`: the fills last booked, so an unchanged window writes nothing.
-- `batches`: the import batch per journal the booked trades belong to (revertible there).
CREATE TABLE journal_chart_books (
    account_id  UUID NOT NULL REFERENCES broker_accounts(id) ON DELETE CASCADE,
    ticker      TEXT NOT NULL,
    asset_type  TEXT NOT NULL,
    resume_from TIMESTAMPTZ NOT NULL,
    seeds       JSONB NOT NULL DEFAULT '[]',
    mark        TEXT NOT NULL DEFAULT '',
    batches     JSONB NOT NULL DEFAULT '{}',
    last_error  TEXT,
    PRIMARY KEY (account_id, ticker, asset_type)
);

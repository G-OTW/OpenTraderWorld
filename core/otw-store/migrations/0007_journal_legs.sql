-- Trading Journal: multi-leg positions (scaling in/out + SL/TP brackets).
--
-- A trade can now be a full position made of legs rather than a single entry/exit.
-- This supports pyramiding (multiple entries), partial scale-outs (multiple exits),
-- and planned SL/TP brackets where only the triggered ones become real fills.
--
-- Storage is JSONB on journal_trades (single-user, computed in Rust). The flat columns
-- (entry_price/exit_price/quantity/…) stay as the "simple mode" representation; when
-- `advanced` is true the legs below take over. Every leg carries its own price, qty,
-- optional date and fees so a FIFO tax report is fully derivable later without a
-- schema change. `cost_basis_method` selects how PnL is displayed in the journal.
--
-- Leg shapes (keys are camelCase to match the JSON the API speaks):
--   entries/exits: [{ "id","price","qty","at"?,"fees"?,"signal"? }, ...]
--   brackets:      [{ "id","kind":"sl"|"tp","price","qty"?,"at"?,"triggered":bool,"note"? }, ...]
-- SL/TP brackets carry no signal (they are not signal-driven). A triggered bracket is
-- folded into an exit leg by the API, so PnL has a single source of truth (the legs).

ALTER TABLE journal_trades
    ADD COLUMN IF NOT EXISTS advanced          BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS cost_basis_method TEXT NOT NULL DEFAULT 'avg'
        CHECK (cost_basis_method IN ('avg', 'fifo')),
    ADD COLUMN IF NOT EXISTS entries           JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS exits             JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS brackets          JSONB NOT NULL DEFAULT '[]'::jsonb;

-- Subscriptions: monthly budget per category, in the display currency.
-- `category_budgets`: { "<category>": cap }. `budget_channels`: channel ids, `[]` = every
-- channel granted to Subscriptions. `budget_notified`: { "<category>": "YYYY-MM" }, the
-- month a breach was last notified, so a category notifies once per month.
ALTER TABLE subscription_settings
    ADD COLUMN IF NOT EXISTS category_budgets JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS budget_channels  JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS budget_notified  JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Trading Journal: let a template carry a default fee schedule.
--
-- Logging a trade from the template pre-selects this fee schedule (still overridable per
-- trade). The fee-schedule picker itself is always available on the trade form regardless
-- of template; this only sets the default selection.

ALTER TABLE journal_templates
    ADD COLUMN IF NOT EXISTS default_fee_schedule_id UUID
        REFERENCES journal_fee_schedules(id) ON DELETE SET NULL;

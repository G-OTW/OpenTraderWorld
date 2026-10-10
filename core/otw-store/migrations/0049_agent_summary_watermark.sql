-- Rolling-summary watermark for agent conversations.
--
-- `summary_covers` = how many of the conversation's oldest messages are already folded
-- into `summary`. The run sends only messages after the watermark verbatim, and the
-- summarizer only transcribes messages between the watermark and the keep-recent tail —
-- without it, every send past the size threshold re-summarized the ENTIRE history
-- (unbounded transcript, one extra LLM call per message, and the folded span was also
-- resent verbatim).
ALTER TABLE agent_conversations
    ADD COLUMN summary_covers INTEGER NOT NULL DEFAULT 0;

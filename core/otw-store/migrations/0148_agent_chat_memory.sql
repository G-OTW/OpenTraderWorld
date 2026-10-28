-- Agent: long-term memory can be turned off per chat.
--
-- When FALSE the memory index is left out of the prompt and the memory tools are neither
-- offered nor accepted for that conversation. New chats start with memory on.
ALTER TABLE agent_conversations
    ADD COLUMN memory_enabled BOOLEAN NOT NULL DEFAULT TRUE;

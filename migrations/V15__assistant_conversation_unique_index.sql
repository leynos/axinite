-- One assistant conversation per user per channel.
CREATE UNIQUE INDEX IF NOT EXISTS uq_conv_assistant
ON conversations (user_id, channel)
WHERE metadata->>'thread_type' = 'assistant';

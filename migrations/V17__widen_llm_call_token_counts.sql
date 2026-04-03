ALTER TABLE llm_calls
    ALTER COLUMN input_tokens TYPE BIGINT,
    ALTER COLUMN output_tokens TYPE BIGINT;

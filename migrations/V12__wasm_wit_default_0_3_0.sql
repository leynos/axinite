ALTER TABLE wasm_tools
    ALTER COLUMN wit_version SET DEFAULT '0.3.0';

UPDATE wasm_tools
SET wit_version = '0.3.0'
WHERE wit_version = '0.1.0';

ALTER TABLE wasm_channels
    ALTER COLUMN wit_version SET DEFAULT '0.3.0';

UPDATE wasm_channels
SET wit_version = '0.3.0'
WHERE wit_version = '0.1.0';

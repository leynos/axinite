ALTER TABLE wasm_tools
    ALTER COLUMN wit_version SET DEFAULT '0.3.0';

ALTER TABLE wasm_channels
    ALTER COLUMN wit_version SET DEFAULT '0.3.0';

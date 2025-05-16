-- Initial schema for Bitcoin transaction analytics

-- Create a script_types enum table
CREATE TABLE script_types (
    script_type VARCHAR(20) PRIMARY KEY,
    description TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Populate the script_types table with known types
INSERT INTO script_types (script_type, description) VALUES
('p2pkh', 'Pay to Public Key Hash - Bitcoin addresses starting with 1'),
('p2sh', 'Pay to Script Hash - Multisig and complex script addresses starting with 3'),
('p2pk', 'Pay to Public Key - Early Bitcoin format that exposes public keys'),
('p2wpkh', 'Pay to Witness Public Key Hash - SegWit PKH addresses starting with bc1q'),
('p2wsh', 'Pay to Witness Script Hash - SegWit script addresses starting with bc1q'),
('p2tr', 'Pay to Taproot - Taproot addresses starting with bc1p'),
('p2ms', 'Pay to MultiSig - Legacy (raw) multisig scripts'),
('non-standard', 'Non-standard scripts with recognisable patterns'),
('unknown', 'Completely unknown script pattern');

-- Core block data
CREATE TABLE blocks (
    block_height INTEGER PRIMARY KEY,
    block_hash BYTEA NOT NULL UNIQUE, -- 32 bytes
    block_timestamp TIMESTAMP NOT NULL,
    transaction_count INTEGER NOT NULL
);

-- All transactions
CREATE TABLE transactions (
    transaction_id BYTEA NOT NULL, -- 32 bytes
    block_height INTEGER NOT NULL REFERENCES blocks(block_height),
    transaction_index INTEGER NOT NULL, -- Position within block
    is_coinbase BOOLEAN NOT NULL DEFAULT FALSE,
    fee_satoshis BIGINT,
    input_count INTEGER NOT NULL,
    output_count INTEGER NOT NULL,
    PRIMARY KEY (transaction_id, block_height), -- Ensures TXID is unique per block
    UNIQUE(block_height, transaction_index)
);

-- TXID to block_height lookup table
CREATE TABLE txid_block_index (
    transaction_id BYTEA NOT NULL,
    block_height INTEGER NOT NULL,
    PRIMARY KEY (transaction_id, block_height),
    FOREIGN KEY (transaction_id, block_height) REFERENCES transactions(transaction_id, block_height)
);

-- Index for fast lookups
CREATE INDEX idx_txid_block_index_txid ON txid_block_index(transaction_id);

-- All unique addresses
CREATE TABLE addresses (
    address_id BIGSERIAL PRIMARY KEY,
    address_string VARCHAR(255) NOT NULL UNIQUE,
    script_type VARCHAR(20) NOT NULL REFERENCES script_types(script_type),
    first_seen_block_height INTEGER NOT NULL,
    total_receive_count INTEGER NOT NULL DEFAULT 0,
    total_spend_count INTEGER NOT NULL DEFAULT 0,
    is_public_key_exposed BOOLEAN NOT NULL DEFAULT FALSE,
    public_key BYTEA, -- Stored when revealed in a spend transaction
    script_extra_data JSONB -- Extra data for P2MS info, compressed/uncompressed for P2PK, script info for non-standard
);

-- Index for script type lookups
CREATE INDEX idx_addresses_script_type ON addresses(script_type);

-- Index for script pubkey exposure
CREATE INDEX idx_addresses_script_pubkey_exposed ON addresses(script_type, is_public_key_exposed);

-- Index for efficient block height range queries
CREATE INDEX idx_addresses_first_seen ON addresses(first_seen_block_height);

-- Index for JSONB data lookups
CREATE INDEX idx_addresses_extra_data ON addresses USING GIN (script_extra_data);

-- Outputs associated with addresses
CREATE TABLE address_outputs (
    output_id BIGSERIAL PRIMARY KEY,
    address_id BIGINT NOT NULL REFERENCES addresses(address_id),
    transaction_id BYTEA NOT NULL,
    block_height INTEGER NOT NULL,
    output_index INTEGER NOT NULL,
    value_satoshis BIGINT NOT NULL,
    is_spent BOOLEAN NOT NULL DEFAULT FALSE,
    spending_input_id BIGINT, -- Links to the input that spent this output
    FOREIGN KEY (transaction_id, block_height) REFERENCES transactions(transaction_id, block_height),
    UNIQUE(transaction_id, block_height, output_index) -- Updated unique constraint
);

-- Index for fast lookup of unspent outputs (for input processing)
CREATE INDEX idx_address_outputs_not_spent ON address_outputs(transaction_id, output_index) WHERE is_spent = false;

-- Index for fast lookup by address ID and transaction ID
CREATE INDEX idx_address_outputs_addr_tx ON address_outputs(address_id, transaction_id);

-- Index for fast lookup by spending state (e.g. UTXO set queries)
CREATE INDEX idx_address_outputs_address_spent ON address_outputs(address_id, is_spent);

-- Index for fast lookup by output value
CREATE INDEX idx_address_outputs_value ON address_outputs(value_satoshis);

-- Index for fast lookup by address ID and block height
CREATE INDEX idx_address_outputs_address_block ON address_outputs(address_id, block_height);


-- Inputs (spends) from addresses
CREATE TABLE address_inputs (
    input_id BIGSERIAL PRIMARY KEY,
    address_id BIGINT NOT NULL REFERENCES addresses(address_id),
    transaction_id BYTEA NOT NULL,
    block_height INTEGER NOT NULL, -- Added for composite FK
    input_index INTEGER NOT NULL,
    spent_output_id BIGINT NOT NULL REFERENCES address_outputs(output_id), -- Links to the output that was spent
    value_satoshis BIGINT NOT NULL,
    public_key_revealed BYTEA,
    FOREIGN KEY (transaction_id, block_height) REFERENCES transactions(transaction_id, block_height),
    UNIQUE(transaction_id, block_height, input_index) -- Updated unique constraint
);

-- Index for fast lookup by address ID and transaction ID
CREATE INDEX idx_address_inputs_addr_tx ON address_inputs(address_id, transaction_id);

-- Index for fast lookups for input/output relationships
CREATE INDEX idx_address_inputs_spent_output ON address_inputs(spent_output_id);

-- Index for public key revelation analysis
CREATE INDEX idx_address_inputs_pubkey ON address_inputs(public_key_revealed) WHERE public_key_revealed IS NOT NULL;

-- Index for fast lookup by address ID and block height
CREATE INDEX idx_address_inputs_address_block ON address_inputs(address_id, block_height);

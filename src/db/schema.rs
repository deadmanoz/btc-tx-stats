// @generated automatically by Diesel CLI.

diesel::table! {
    address_inputs (input_id) {
        input_id -> Int8,
        address_id -> Int8,
        transaction_id -> Bytea,
        block_height -> Int4,
        input_index -> Int4,
        spent_output_id -> Int8,
        value_satoshis -> Int8,
        public_key_revealed -> Nullable<Bytea>,
    }
}

diesel::table! {
    address_outputs (output_id) {
        output_id -> Int8,
        address_id -> Int8,
        transaction_id -> Bytea,
        block_height -> Int4,
        output_index -> Int4,
        value_satoshis -> Int8,
        is_spent -> Bool,
        spending_input_id -> Nullable<Int8>,
    }
}

diesel::table! {
    addresses (address_id) {
        address_id -> Int8,
        #[max_length = 255]
        address_string -> Varchar,
        #[max_length = 20]
        script_type -> Varchar,
        first_seen_block_height -> Int4,
        total_receive_count -> Int4,
        total_spend_count -> Int4,
        is_public_key_exposed -> Bool,
        public_key -> Nullable<Bytea>,
        script_extra_data -> Nullable<Jsonb>,
    }
}

diesel::table! {
    blocks (block_height) {
        block_height -> Int4,
        block_hash -> Bytea,
        block_timestamp -> Timestamp,
        transaction_count -> Int4,
    }
}

diesel::table! {
    script_types (script_type) {
        #[max_length = 20]
        script_type -> Varchar,
        description -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    transactions (transaction_id, block_height) {
        transaction_id -> Bytea,
        block_height -> Int4,
        transaction_index -> Int4,
        is_coinbase -> Bool,
        fee_satoshis -> Nullable<Int8>,
        input_count -> Int4,
        output_count -> Int4,
    }
}

diesel::table! {
    txid_block_index (transaction_id, block_height) {
        transaction_id -> Bytea,
        block_height -> Int4,
    }
}

diesel::joinable!(address_inputs -> address_outputs (spent_output_id));
diesel::joinable!(address_inputs -> addresses (address_id));
diesel::joinable!(address_outputs -> addresses (address_id));
diesel::joinable!(addresses -> script_types (script_type));
diesel::joinable!(transactions -> blocks (block_height));

diesel::allow_tables_to_appear_in_same_query!(
    address_inputs,
    address_outputs,
    addresses,
    blocks,
    script_types,
    transactions,
    txid_block_index,
);

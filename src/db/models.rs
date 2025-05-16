use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde_json::Value;

use super::schema::{
    address_inputs, address_outputs, addresses, blocks, transactions, txid_block_index,
};

// Model for querying and inserting into 'blocks' table
#[derive(Queryable, Selectable, Insertable)]
#[diesel(table_name = blocks)]
pub struct Block {
    pub block_height: i32,
    pub block_hash: Vec<u8>,
    pub block_timestamp: NaiveDateTime,
    pub transaction_count: i32,
}

// Model for inserting into the 'transactions' table
#[derive(Insertable)]
#[diesel(table_name = transactions)]
pub struct NewTransaction {
    pub transaction_id: Vec<u8>, // BYTEA
    pub block_height: i32,
    pub transaction_index: i32,
    pub is_coinbase: bool,
    pub fee_satoshis: Option<i64>,
    pub input_count: i32,
    pub output_count: i32,
}

// Model for querying 'transactions' table
#[derive(Queryable, Selectable)]
#[diesel(table_name = transactions)]
#[diesel(primary_key(transaction_id, block_height))]
pub struct Transaction {
    pub transaction_id: Vec<u8>,
    pub block_height: i32,
    pub transaction_index: i32,
    pub is_coinbase: bool,
    pub fee_satoshis: Option<i64>,
    pub input_count: i32,
    pub output_count: i32,
}

// Model for inserting into the 'addresses' table
#[derive(Insertable)]
#[diesel(table_name = addresses)]
pub struct NewAddress {
    pub address_string: String, // VARCHAR(100)
    pub script_type: String,    // VARCHAR(20)
    pub first_seen_block_height: i32,
    pub script_extra_data: Option<Value>, // JSONB
    pub public_key: Option<Vec<u8>>,      // BYTEA
}

// Model for querying 'addresses' table
#[derive(Queryable, Selectable)]
#[diesel(table_name = addresses)]
pub struct Address {
    pub address_id: i64,
    pub address_string: String,
    pub script_type: String,
    pub first_seen_block_height: i32,
    pub total_receive_count: i32,
    pub total_spend_count: i32,
    pub is_public_key_exposed: bool,
    pub public_key: Option<Vec<u8>>,
    pub script_extra_data: Option<Value>,
}

// Model for inserting into the 'address_outputs' table
#[derive(Insertable)]
#[diesel(table_name = address_outputs)]
pub struct NewAddressOutput {
    pub address_id: i64,
    pub transaction_id: Vec<u8>, // BYTEA
    pub block_height: i32,
    pub output_index: i32,
    pub value_satoshis: i64,
    pub spending_input_id: Option<i64>,
}

// Model for querying 'address_outputs' table
#[derive(Queryable, Selectable)]
#[diesel(table_name = address_outputs)]
pub struct AddressOutput {
    pub output_id: i64,
    pub address_id: i64,
    pub transaction_id: Vec<u8>,
    pub block_height: i32,
    pub output_index: i32,
    pub value_satoshis: i64,
    pub is_spent: bool,
    pub spending_input_id: Option<i64>,
}

// Model for inserting into the 'address_inputs' table
#[derive(Insertable)]
#[diesel(table_name = address_inputs)]
pub struct NewAddressInput {
    pub address_id: i64,
    pub transaction_id: Vec<u8>, // BYTEA
    pub block_height: i32,
    pub input_index: i32,
    pub spent_output_id: i64,
    pub value_satoshis: i64,
    pub public_key_revealed: Option<Vec<u8>>, // BYTEA
}

// Model for querying 'address_inputs' table
#[derive(Queryable, Selectable)]
#[diesel(table_name = address_inputs)]
pub struct AddressInput {
    pub input_id: i64,
    pub address_id: i64,
    pub transaction_id: Vec<u8>,
    pub block_height: i32,
    pub input_index: i32,
    pub spent_output_id: i64,
    pub value_satoshis: i64,
    pub public_key_revealed: Option<Vec<u8>>,
}

// Model for inserting into the 'txid_block_index' table
#[derive(Insertable)]
#[diesel(table_name = txid_block_index)]
pub struct NewTxidBlockIndex {
    pub transaction_id: Vec<u8>, // BYTEA
    pub block_height: i32,
}

// Model for querying 'txid_block_index' table
#[derive(Queryable, Selectable)]
#[diesel(table_name = txid_block_index)]
pub struct TxidBlockIndex {
    pub transaction_id: Vec<u8>,
    pub block_height: i32,
}

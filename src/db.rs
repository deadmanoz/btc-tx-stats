use anyhow::{Context, Result};
use chrono;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel::PgConnection;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use hex;
use serde_json::Value;
use std::time::Duration;
use tracing::info;

// Define migrations
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("./migrations");

// Define database schema (will be populated by diesel)
pub mod models;
pub mod schema;

/// Type alias for database connection pool
pub type DbPool = Pool<ConnectionManager<PgConnection>>;

/// Creates a Postgres connection pool
pub fn create_connection_pool(database_url: &str) -> Result<DbPool> {
    let manager = ConnectionManager::<PgConnection>::new(database_url);

    Pool::builder()
        .connection_timeout(Duration::from_secs(30))
        .test_on_check_out(true)
        .build(manager)
        .context("Failed to create database connection pool")
}

/// Runs database migrations
pub fn run_migrations(conn: &mut PgConnection) -> Result<()> {
    info!("Running database migrations");

    // Run migrations and map the error to anyhow
    match conn.run_pending_migrations(MIGRATIONS) {
        Ok(_) => {
            info!("Database migrations completed successfully");
            Ok(())
        }
        Err(e) => {
            anyhow::bail!("Failed to run database migrations: {}", e)
        }
    }
}

/// Gets the last processed block height from the database
pub fn get_last_processed_height(conn: &mut PgConnection) -> Result<Option<u32>> {
    use schema::blocks::dsl::*;

    let result = blocks
        .select(block_height)
        .order(block_height.desc())
        .limit(1)
        .first::<i32>(conn)
        .optional()
        .context("Failed to query last processed block")?;

    Ok(result.map(|h| h as u32))
}

/// Stores a new processed block in the database
pub fn store_processed_block(
    conn: &mut PgConnection,
    block_height_val: u32,
    block_hash_val: &str,
    block_timestamp_val: i64,
    tx_count_val: u32,
) -> Result<()> {
    use diesel::insert_into;
    use schema::blocks::dsl::*;

    let block_hash_bytes =
        hex::decode(block_hash_val).context("Failed to decode block hash hex string")?;

    let new_block_record = models::Block {
        block_height: block_height_val as i32,
        block_hash: block_hash_bytes,
        block_timestamp: chrono::DateTime::from_timestamp(block_timestamp_val, 0)
            .map(|dt| dt.naive_utc())
            .context("Invalid timestamp value for DateTime conversion")?,
        transaction_count: tx_count_val as i32,
    };

    insert_into(blocks)
        .values(&new_block_record)
        .on_conflict(block_height)
        .do_update()
        .set((
            block_hash.eq(&new_block_record.block_hash),
            block_timestamp.eq(&new_block_record.block_timestamp),
            transaction_count.eq(new_block_record.transaction_count),
        ))
        .execute(conn)
        .context("Failed to store block")?;

    Ok(())
}

/// Stores details of a single transaction in the database
pub fn store_transaction(
    conn: &mut PgConnection,
    block_height_val: u32,
    tx_index_val: u32,
    tx_id_str: &str,
    is_coinbase_val: bool,
    input_count_val: i32,
    output_count_val: i32,
    fee_satoshis_val: Option<i64>,
) -> Result<()> {
    use crate::db::models::NewTransaction;
    use diesel::insert_into;
    use schema::transactions::dsl::*;

    let tx_id_bytes =
        hex::decode(tx_id_str).context("Failed to decode transaction ID hex string")?;

    let new_tx_record = NewTransaction {
        transaction_id: tx_id_bytes.clone(),
        block_height: block_height_val as i32,
        transaction_index: tx_index_val as i32,
        is_coinbase: is_coinbase_val,
        input_count: input_count_val,
        output_count: output_count_val,
        fee_satoshis: fee_satoshis_val,
    };

    insert_into(transactions)
        .values(&new_tx_record)
        .on_conflict((transaction_id, block_height))
        .do_nothing()
        .execute(conn)
        .context(format!("Failed to store transaction {}", tx_id_str))?;

    // Add to TXID index
    add_txid_to_index(conn, &tx_id_bytes, block_height_val)?;

    Ok(())
}

/// Add a transaction ID to the TXID index table
pub fn add_txid_to_index(
    conn: &mut PgConnection,
    txid_bytes: &[u8],
    block_height_val: u32,
) -> Result<()> {
    use crate::db::models::NewTxidBlockIndex;
    use diesel::insert_into;
    use schema::txid_block_index::dsl::*;

    let new_index_record = NewTxidBlockIndex {
        transaction_id: txid_bytes.to_vec(),
        block_height: block_height_val as i32,
    };

    insert_into(txid_block_index)
        .values(&new_index_record)
        .on_conflict((transaction_id, block_height))
        .do_nothing()
        .execute(conn)
        .context(format!("Failed to add TXID to index"))?;

    Ok(())
}

/// Gets or creates an address record, returning the address_id
pub fn get_or_create_address(
    conn: &mut PgConnection,
    address_string_val: &str,
    script_type_val: &str,
    first_seen_block_height_val: u32,
    extra_data_val: Option<Value>,
) -> Result<i64> {
    use crate::db::models::NewAddress;
    use diesel::insert_into;
    use schema::addresses::dsl::*;

    // 1. Try to find the address
    // DB QUERY!
    let existing_address = addresses
        .filter(address_string.eq(address_string_val))
        .select(address_id)
        .first::<i64>(conn)
        .optional()
        .context("Failed to query address")?;

    if let Some(id) = existing_address {
        // Address exists, return its ID
        return Ok(id);
    }

    // 2. Address doesn't exist, create it
    let new_address = NewAddress {
        address_string: address_string_val.to_string(),
        script_type: script_type_val.to_string(),
        first_seen_block_height: first_seen_block_height_val as i32,
        script_extra_data: extra_data_val,
        public_key: None, // Will be updated if revealed in an input
    };

    //3. DB INSERT!
    insert_into(addresses)
        .values(&new_address)
        .returning(address_id)
        .get_result(conn)
        .context("Failed to insert new address")
}

/// Store a transaction output associated with an address
pub fn store_transaction_output(
    conn: &mut PgConnection,
    address_id_val: i64,
    txid_str: &str,
    block_height_val: i32,
    output_index_val: i32,
    value_satoshis_val: u64,
) -> Result<i64> {
    use crate::db::models::NewAddressOutput;
    use diesel::insert_into;
    use schema::address_outputs::dsl::*;

    let txid_bytes = hex::decode(txid_str).context("Failed to decode transaction ID hex string")?;

    let new_output = NewAddressOutput {
        address_id: address_id_val,
        transaction_id: txid_bytes,
        block_height: block_height_val,
        output_index: output_index_val,
        value_satoshis: value_satoshis_val as i64,
        spending_input_id: None, // Will be updated when spent
    };

    // Insert and get the new output_id
    // DB INSERT!
    let output_id_val = insert_into(address_outputs)
        .values(&new_output)
        .returning(output_id)
        .get_result(conn)
        .context("Failed to insert transaction output")?;

    // Update the address receive count
    update_address_receive_count(conn, address_id_val)?;

    Ok(output_id_val)
}

/// Find an output by transaction ID and output index
pub fn find_output(
    conn: &mut PgConnection,
    txid_str: &str,
    output_index_val: i32,
) -> Result<Option<OutputInfo>> {
    // Import table namespaces rather than columns to avoid ambiguity
    use schema::address_outputs;
    use schema::txid_block_index;

    let txid_bytes = hex::decode(txid_str).context("Failed to decode transaction ID hex string")?;

    // First, find all blocks containing this TXID
    let block_heights: Vec<i32> = txid_block_index::table
        .filter(txid_block_index::transaction_id.eq(&txid_bytes))
        .select(txid_block_index::block_height)
        .load(conn)
        .context("Failed to query txid_block_index")?;

    // If no blocks contain this TXID, return None
    if block_heights.is_empty() {
        return Ok(None);
    }

    // For each block_height, try to find the output
    for height in block_heights {
        let output_info = address_outputs::table
            .filter(address_outputs::transaction_id.eq(&txid_bytes))
            .filter(address_outputs::block_height.eq(height))
            .filter(address_outputs::output_index.eq(output_index_val))
            .filter(address_outputs::is_spent.eq(false)) // Ensure it's not already spent
            .select((
                address_outputs::output_id,
                address_outputs::address_id,
                address_outputs::value_satoshis,
            ))
            .first::<(i64, i64, i64)>(conn)
            .optional()
            .context("Failed to query output")?;

        if let Some((out_id, addr_id, value)) = output_info {
            // Found it!
            return Ok(Some(OutputInfo {
                output_id: out_id,
                address_id: addr_id,
                value_satoshis: value,
            }));
        }
    }

    // No matching output found in any block
    Ok(None)
}

/// Store a transaction input that spends a previous output
pub fn store_transaction_input(
    conn: &mut PgConnection,
    address_id_val: i64,
    txid_str: &str,
    block_height_val: i32,
    input_index_val: i32,
    spent_output_id_val: i64,
    value_satoshis_val: i64,
    public_key_revealed_val: Option<Vec<u8>>,
) -> Result<i64> {
    use crate::db::models::NewAddressInput;
    use diesel::insert_into;
    use schema::address_inputs::dsl::*;

    let txid_bytes = hex::decode(txid_str).context("Failed to decode transaction ID hex string")?;

    let new_input = NewAddressInput {
        address_id: address_id_val,
        transaction_id: txid_bytes,
        block_height: block_height_val,
        input_index: input_index_val,
        spent_output_id: spent_output_id_val,
        value_satoshis: value_satoshis_val,
        public_key_revealed: public_key_revealed_val.clone(),
    };

    // Insert and get the new input_id
    // DB INSERT!
    let input_id_val = insert_into(address_inputs)
        .values(&new_input)
        .returning(input_id)
        .get_result(conn)
        .context("Failed to insert transaction input")?;

    // Update the address spend count
    update_address_spend_count(conn, address_id_val)?;

    // If a public key was revealed, update the address record
    if let Some(ref pubkey) = public_key_revealed_val {
        update_address_public_key(conn, address_id_val, pubkey.clone())?;
    }

    Ok(input_id_val)
}

/// Mark an output as spent by an input
pub fn mark_output_spent(
    conn: &mut PgConnection,
    output_id_val: i64,
    spending_input_id_val: i64,
) -> Result<()> {
    use diesel::update;
    use schema::address_outputs::dsl::*;

    // DB UPDATE!
    update(address_outputs.filter(output_id.eq(output_id_val)))
        .set((
            is_spent.eq(true),
            spending_input_id.eq(spending_input_id_val),
        ))
        .execute(conn)
        .context("Failed to mark output as spent")?;

    Ok(())
}

/// Update the receive count for an address
fn update_address_receive_count(conn: &mut PgConnection, address_id_val: i64) -> Result<()> {
    use diesel::update;
    use schema::addresses::dsl::*;

    // DB UPDATE!
    update(addresses.filter(address_id.eq(address_id_val)))
        .set(total_receive_count.eq(total_receive_count + 1))
        .execute(conn)
        .context("Failed to update address receive count")?;

    Ok(())
}

/// Update the spend count for an address
fn update_address_spend_count(conn: &mut PgConnection, address_id_val: i64) -> Result<()> {
    use diesel::update;
    use schema::addresses::dsl::*;

    // DB UPDATE!
    update(addresses.filter(address_id.eq(address_id_val)))
        .set(total_spend_count.eq(total_spend_count + 1))
        .execute(conn)
        .context("Failed to update address spend count")?;

    Ok(())
}

/// Update an address's public key if revealed
fn update_address_public_key(
    conn: &mut PgConnection,
    address_id_val: i64,
    pubkey: Vec<u8>,
) -> Result<()> {
    use diesel::update;
    use schema::addresses::dsl::*;

    // DB UPDATE!
    update(addresses.filter(address_id.eq(address_id_val)))
        .set((public_key.eq(pubkey), is_public_key_exposed.eq(true)))
        .execute(conn)
        .context("Failed to update address public key")?;

    Ok(())
}

/// Structure to return output information
pub struct OutputInfo {
    pub output_id: i64,
    pub address_id: i64,
    pub value_satoshis: i64,
}

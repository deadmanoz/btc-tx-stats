use anyhow::{Context, Result};
use diesel::Connection;
use diesel::PgConnection;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

use crate::bitcoin_client::BitcoinClient;
use crate::db::{self, DbPool};

use bech32::{hrp, segwit, Hrp};
use bitcoin::base58;
use bitcoin::blockdata::opcodes::all::*;
use bitcoin::blockdata::script::Instruction;
use bitcoin::hashes::{hash160, Hash};
use bitcoin::script::Script;
use secp256k1::PublicKey;

/// Processes Bitcoin blocks and extracts analytics data
pub struct BlockProcessor {
    bitcoin_client: BitcoinClient,
    db_pool: DbPool,
}

impl BlockProcessor {
    /// Creates a new block processor
    pub fn new(bitcoin_client: BitcoinClient, db_pool: DbPool) -> Self {
        Self {
            bitcoin_client,
            db_pool,
        }
    }

    /// Gets the current blockchain tip height from the Bitcoin node
    pub async fn get_current_blockchain_tip(&self) -> Result<u64> {
        self.bitcoin_client
            .get_block_count()
            .await
            .context("Failed to get current blockchain tip from bitcoin_client")
    }

    const RETRY_DELAY: Duration = Duration::from_secs(2);
    const MAX_RETRIES: u32 = 3;

    /// Process blocks from start_height up to current tip
    pub async fn process_all_blocks(&self, start_height: u64) -> Result<()> {
        let current_tip = self.get_current_blockchain_tip().await?;

        if start_height > current_tip {
            info!(
                "No blocks to sync. Start height {} is already ahead of current node tip {}.",
                start_height, current_tip
            );
            return Ok(());
        }

        info!(
            "Syncing blocks from height {} to {}",
            start_height, current_tip
        );

        let mut current_height = start_height;

        // Process blocks until we reach the current tip
        while current_height <= current_tip {
            // Process one block at a time to maintain sequential relationships
            match self.process_single_block(current_height).await {
                Ok(_) => {
                    current_height += 1;
                }
                Err(e) => {
                    error!(
                        "Failed to process block at height {}: {}",
                        current_height, e
                    );
                    return Err(e);
                }
            }

            // Periodically check for updated chain tip
            if current_height % 100 == 0 {
                let new_tip = self.get_current_blockchain_tip().await?;
                if new_tip > current_tip {
                    info!(
                        "Chain tip advanced from {} to {} during sync",
                        current_tip, new_tip
                    );
                }
            }
        }

        info!("Blockchain sync complete");
        Ok(())
    }

    /// Process new blocks as they arrive
    pub async fn process_new_blocks(&self, starting_height: u32) -> Result<()> {
        info!(
            "Starting continuous block processing from height {}",
            starting_height
        );

        let mut current_height = starting_height as u64;

        loop {
            // Get current blockchain tip
            let chain_tip = self.get_current_blockchain_tip().await?;

            // Process new blocks if available
            if current_height <= chain_tip {
                while current_height <= chain_tip {
                    match self.process_single_block(current_height).await {
                        Ok(_) => {
                            info!("Processed block at height {}", current_height);
                            current_height += 1;
                        }
                        Err(e) => {
                            error!("Error processing block {}: {}", current_height, e);

                            // Retry with backoff
                            let mut retries = 0;
                            let mut retry_success = false;

                            while retries < Self::MAX_RETRIES {
                                retries += 1;
                                sleep(Self::RETRY_DELAY).await;

                                match self.process_single_block(current_height).await {
                                    Ok(_) => {
                                        info!(
                                            "Successfully processed block {} on retry {}",
                                            current_height, retries
                                        );
                                        current_height += 1;
                                        retry_success = true;
                                        break;
                                    }
                                    Err(retry_e) => {
                                        error!(
                                            "Retry {} failed for block {}: {}",
                                            retries, current_height, retry_e
                                        );
                                    }
                                }
                            }

                            if !retry_success {
                                error!(
                                    "Failed to process block {} after {} retries",
                                    current_height,
                                    Self::MAX_RETRIES
                                );
                                // Exit application if block processing fails after max retries
                                return Err(anyhow::anyhow!(
                                    "Failed to process block {} after {} retries - exiting",
                                    current_height,
                                    Self::MAX_RETRIES
                                ));
                            }
                        }
                    }
                }
            } else {
                debug!("No new blocks to process. Waiting...");
            }

            // Wait before checking for new blocks
            sleep(Duration::from_secs(10)).await;
        }
    }

    async fn process_single_block(&self, height: u64) -> Result<()> {
        debug!("Processing block at height {}", height);

        // Get block data
        let block = self.bitcoin_client.get_block_by_height(height).await?;
        let block_hash = block.block_hash().to_string();
        let timestamp = block.header.time as i64;
        let tx_count = block.txdata.len() as u32;

        // Get a database connection from the pool
        let mut conn = self
            .db_pool
            .get()
            .context("Failed to get database connection")?;

        // Use a database transaction to ensure atomicity
        conn.transaction(|tx_conn| {
            // 1. Store block data
            db::store_processed_block(tx_conn, height as u32, &block_hash, timestamp, tx_count)?;

            // 2. Process all transactions in the block
            self.process_block_transactions(tx_conn, height as u32, &block_hash, &block.txdata)?;

            Ok::<(), anyhow::Error>(())
        })
        .context(format!("Database transaction failed for block {}", height))?;

        info!(
            "Successfully processed block {} with {} transactions",
            height, tx_count
        );
        Ok(())
    }

    /// Process all transactions in a block with their inputs and outputs
    fn process_block_transactions(
        &self,
        conn: &mut PgConnection,
        height: u32,
        block_hash: &str,
        txs: &[bitcoin::Transaction],
    ) -> Result<()> {
        debug!(
            "Processing {} transactions for block {} ({})",
            txs.len(),
            height,
            block_hash
        );

        for (tx_index, tx) in txs.iter().enumerate() {
            let txid = tx.compute_txid().to_string();
            let is_coinbase = tx.is_coinbase();
            let input_count = tx.input.len() as i32;
            let output_count = tx.output.len() as i32;

            let fee_satoshis = Some(0);
            // // Calculate transaction fee
            // let fee_satoshis: Option<i64> = if is_coinbase {
            //     Some(0) // Coinbase transactions have no fee
            // } else {
            //     let mut total_input_value: i64 = 0;
            //     for input in &tx.input {
            //         // DB QUERY!
            //         if let Some(prev_output_info) = db::find_output(
            //             conn,
            //             &input.previous_output.txid.to_string(),
            //             input.previous_output.vout as i32,
            //         )? {
            //             total_input_value += prev_output_info.value_satoshis;
            //         } else {
            //             error!("Could not find previous output ({}:{}) for input in tx {}. Fee calculation might be incorrect.", input.previous_output.txid, input.previous_output.vout, txid);
            //             return Err(anyhow::anyhow!(
            //                 "Failed to find previous output for fee calculation in tx {}",
            //                 txid
            //             ));
            //         }
            //     }

            //     let total_output_value: i64 =
            //         tx.output.iter().map(|o| o.value.to_sat() as i64).sum();

            //     if total_input_value >= total_output_value {
            //         Some(total_input_value - total_output_value)
            //     } else {
            //         error!("Transaction {} has more output value than input value. Invalid transaction.", txid);
            //         return Err(anyhow::anyhow!(
            //             "Invalid transaction {} with more output than input value.",
            //             txid
            //         ));
            //     }
            // };

            // 1. Store transaction record
            db::store_transaction(
                conn,
                height,
                tx_index as u32,
                &txid,
                is_coinbase,
                input_count,
                output_count,
                fee_satoshis,
            )?;

            // 2. Process transaction outputs
            self.process_transaction_outputs(conn, height, &txid, tx)?;

            // 3. Process transaction inputs (except for coinbase)
            if !is_coinbase {
                self.process_transaction_inputs(conn, height, &txid, tx)?;
            }
        }

        Ok(())
    }

    /// Process outputs for a transaction (creating address records as needed)
    fn process_transaction_outputs(
        &self,
        conn: &mut PgConnection,
        height: u32,
        txid: &str,
        tx: &bitcoin::Transaction,
    ) -> Result<()> {
        // For each output in the transaction
        for (output_index, output) in tx.output.iter().enumerate() {
            // Extract address from scriptPubKey
            if let Some(script_info) = extract_address_from_script(&output.script_pubkey) {
                // Store or get address ID
                let address_id = db::get_or_create_address(
                    conn,
                    &script_info.address,
                    &script_info.script_type,
                    height,
                    script_info.extra_data,
                )?;

                // Store the output - convert Amount to u64
                db::store_transaction_output(
                    conn,
                    address_id,
                    txid,
                    height as i32,
                    output_index as i32,
                    output.value.to_sat(),
                )?;
            }
        }

        Ok(())
    }

    /// Process inputs for a transaction (linking to previous outputs)
    fn process_transaction_inputs(
        &self,
        conn: &mut PgConnection,
        height: u32,
        txid: &str,
        tx: &bitcoin::Transaction,
    ) -> Result<()> {
        // For each input in the transaction
        for (input_index, input) in tx.input.iter().enumerate() {
            let prev_txid = input.previous_output.txid.to_string();
            let prev_vout = input.previous_output.vout as i32;

            // Find the previous output - now without needing to specify height
            if let Some(output_info) = db::find_output(conn, &prev_txid, prev_vout)? {
                // Extract public key from input script if available
                let public_key = extract_public_key_from_script(&input.script_sig);

                // Store the input and mark the output as spent
                let input_id = db::store_transaction_input(
                    conn,
                    output_info.address_id,
                    txid,
                    height as i32,
                    input_index as i32,
                    output_info.output_id,
                    output_info.value_satoshis,
                    public_key,
                )?;

                // Update the output to mark it as spent
                db::mark_output_spent(conn, output_info.output_id, input_id)?;
            }
        }

        Ok(())
    }
}

/// Structure to represent script type and address
pub struct ScriptInfo {
    pub address: String,
    pub script_type: String,
    pub extra_data: Option<serde_json::Value>, // JSON for flexible additional data
}

/// Extract address and script type information from output script
fn extract_address_from_script(script: &Script) -> Option<ScriptInfo> {
    let instructions = script
        .instructions()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    // P2PKH (Pay to Public Key Hash)
    // P2PKH is of the form: OP_DUP OP_HASH160 <20-byte hash> OP_EQUALVERIFY OP_CHECKSIG
    // Actually there is a OP_PUSHBYTES_20 before the 20-byte hash
    // but this is included in the same element as the hash in the instructions vector
    if script.is_p2pkh() {
        // hash160 is the 20-byte hash of the public key
        if let Some(Instruction::PushBytes(hash160)) = instructions.get(2) {
            if hash160.len() == 20 {
                // Create address from hash160
                // https://learnmeabitcoin.com/technical/script/p2pkh/#address
                let mut data = vec![0]; // mainnet prefix is 00, 6f for testnet
                data.extend_from_slice(hash160.as_bytes());
                let address = base58::encode_check(&data);
                return Some(ScriptInfo {
                    address,
                    script_type: "p2pkh".to_string(),
                    extra_data: None,
                });
            }
        }
    }
    // P2SH (Pay to Script Hash)
    // P2SH is of the form: OP_HASH160 <20-byte hash> OP_EQUAL
    // Actually there is a OP_PUSHBYTES_20 before the 20-byte hash
    // but this is included in the same element as the hash in the instructions vector
    else if script.is_p2sh() {
        if let Some(Instruction::PushBytes(hash160)) = instructions.get(1) {
            if hash160.len() == 20 {
                // Create address from hash160
                // https://learnmeabitcoin.com/technical/script/p2sh/#address
                let mut data = vec![5]; // mainnet p2sh prefix 05, c4 for testnet
                data.extend_from_slice(hash160.as_bytes());
                let address = base58::encode_check(&data);
                return Some(ScriptInfo {
                    address,
                    script_type: "p2sh".to_string(),
                    extra_data: None,
                });
            }
        }
    }
    // P2PK (Pay to Public Key)
    // P2PK is of the form: <pubkey> OP_CHECKSIG
    else if instructions.len() == 2
    && matches!(instructions[0], Instruction::PushBytes(_))
    && (instructions[1].opcode() == Some(OP_CHECKSIG))
    {
        if let Instruction::PushBytes(pubkey_bytes) = &instructions[0] {
            if pubkey_bytes.len() == 33 || pubkey_bytes.len() == 65 {
                // Use the pubkey directly as the address - hex encode it for storage
                let pubkey_hex = hex::encode(pubkey_bytes.as_bytes());

                // Store pubkey format as extra data
                let extra_data = serde_json::json!({
                    "pubkey_format": if pubkey_bytes.len() == 33 { "compressed" } else { "uncompressed" }
                });

                return Some(ScriptInfo {
                    address: pubkey_hex,  // Use the pubkey hex directly as address
                    script_type: "p2pk".to_string(),
                    extra_data: Some(extra_data),
                });
            } else {
                error!("Invalid P2PK public key length: {}", pubkey_bytes.len());
            }
        }
    }
    // All script types that use witness program: P2WPKH, P2WSH, P2TR
    else if script.is_witness_program() {
        // P2WPKH (Pay to Witness Public Key Hash)
        // P2WPKH is of the form: OP_0 <20-byte hash>
        // https://learnmeabitcoin.com/technical/script/p2wpkh/#address
        if script.is_p2wpkh() {
            // TODO: maybe remove these redundant checks?
            if let Some(Instruction::PushBytes(witness_program)) = instructions.get(1) {
                if witness_program.len() == 20 {
                    match encode_bech32_address("bc", 0, witness_program.as_bytes()) {
                        Ok(address) => {
                            return Some(ScriptInfo {
                                address,
                                script_type: "p2wpkh".to_string(),
                                extra_data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to encode P2WPKH address: {}", e);
                            return None; // Skip this script if we can't encode the address
                        }
                    }
                }
            }
        }
        // P2WSH (Pay to Witness Script Hash)
        // P2WSH is of the form: OP_0 <32-byte hash>
        // https://learnmeabitcoin.com/technical/script/p2wsh/#address
        else if script.is_p2wsh() {
            // TODO: maybe remove these redundant checks?
            if let Some(Instruction::PushBytes(witness_program)) = instructions.get(1) {
                if witness_program.len() == 32 {
                    match encode_bech32_address("bc", 0, witness_program.as_bytes()) {
                        Ok(address) => {
                            return Some(ScriptInfo {
                                address,
                                script_type: "p2wsh".to_string(),
                                extra_data: None,
                            });
                        }
                        Err(e) => {
                            error!("Failed to encode P2WSH address: {}", e);
                            return None; // Skip this script if we can't encode the address
                        }
                    }
                }
            }
        }
        // P2TR (Pay to Taproot)
        // P2TR is of the form: OP_1 <32-byte hash>
        // https://learnmeabitcoin.com/technical/script/p2tr/#address
        else if instructions.len() == 2 &&
                instructions[0].opcode() == Some(bitcoin::opcodes::all::OP_PUSHNUM_1) && // Use OP_PUSHNUM_1 instead of OP_1
                matches!(instructions[1], Instruction::PushBytes(bytes) if bytes.len() == 32)
        {
            if let Instruction::PushBytes(taproot_output_key) = &instructions[1] {
                match encode_bech32_address("bc", 1, taproot_output_key.as_bytes()) {
                    Ok(address) => {
                        return Some(ScriptInfo {
                            address,
                            script_type: "p2tr".to_string(),
                            extra_data: None,
                        });
                    }
                    Err(e) => {
                        error!("Failed to encode P2TR address: {}", e);
                        return None; // Skip this script if we can't encode the address
                    }
                }
            }
        }
    }

    // P2MS (Pay to MultiSig)
    // P2MS is of the form: <m> <pubkey1> ... <pubkeyN> <n> OP_CHECKMULTISIG
    // https://learnmeabitcoin.com/technical/script/p2ms/#address
    // P2MS is a locking script for up to 3 public keys (to meet standardness requirements)
    // It's possible to create a multisig script with more public keys (up to 20)
    // but it will be considered non-standard and will not be relayed by nodes.
    // TODO: add support for more than 3 public keys
    if instructions.len() >= 4
        && instructions
            .last()
            .map_or(false, |i| i.opcode() == Some(OP_CHECKMULTISIG))
    {
        // Get first and second-to-last opcodes
        let first_op = instructions.first().and_then(|i| i.opcode());
        let n_op = instructions
            .get(instructions.len() - 2)
            .and_then(|i| i.opcode());

        // Check if valid m-of-n pattern
        if let (Some(first_op), Some(n_op)) = (first_op, n_op) {
            // Extract m and n values
            let m = match first_op {
                bitcoin::opcodes::all::OP_PUSHNUM_1 => 1,
                bitcoin::opcodes::all::OP_PUSHNUM_2 => 2,
                bitcoin::opcodes::all::OP_PUSHNUM_3 => 3,
                _ => return None, // Invalid m value for standard P2MS
            };

            let n = match n_op {
                bitcoin::opcodes::all::OP_PUSHNUM_1 => 1,
                bitcoin::opcodes::all::OP_PUSHNUM_2 => 2,
                bitcoin::opcodes::all::OP_PUSHNUM_3 => 3,
                _ => return None, // Invalid n value for standard P2MS
            };

            // Valid multisig must have m ≤ n and expected number of pubkeys
            if m <= n && instructions.len() == n as usize + 3 {
                // Create a hash of the script to use as an "address"
                let script_hash = hash160::Hash::hash(&script.to_bytes());
                let mut data = vec![5]; // Use same prefix as P2SH for consistency
                data.extend_from_slice(&script_hash[..]);
                let address = base58::encode_check(&data);

                // Store m and n in the extra data
                let extra_data = serde_json::json!({
                    "m": m,
                    "n": n
                });

                return Some(ScriptInfo {
                    address,
                    script_type: "p2ms".to_string(),
                    extra_data: Some(extra_data),
                });
            }
        }
    }

    // Non-standard scripts

    // Non-standard: P2PKH with extra operations (like the OP_NOP case)
    if instructions.len() > 5
        && instructions[0].opcode() == Some(OP_DUP)
        && instructions[1].opcode() == Some(OP_HASH160)
        && matches!(instructions[2], Instruction::PushBytes(_))
        && instructions[3].opcode() == Some(OP_EQUALVERIFY)
        && instructions[4].opcode() == Some(OP_CHECKSIG)
    {
        if let Instruction::PushBytes(hash160) = &instructions[2] {
            if hash160.len() == 20 {
                // Create address from hash160
                let mut data = vec![0]; // mainnet prefix
                data.extend_from_slice(hash160.as_bytes());
                let address = base58::encode_check(&data);

                // Create extra data with script details
                let script_ops: Vec<String> = instructions
                    .iter()
                    .map(|inst| match inst {
                        Instruction::PushBytes(bytes) => format!("PUSH({} bytes)", bytes.len()),
                        Instruction::Op(op) => format!("{:?}", op),
                        _ => "Unknown".to_string(),
                    })
                    .collect();

                let extra_ops = if instructions.len() > 5 {
                    script_ops[5..].to_vec()
                } else {
                    Vec::new()
                };

                let extra_data = serde_json::json!({
                    "pattern": "p2pkh-plus",
                    "extra_ops": extra_ops
                });

                debug!("Found non-standard script: {}", script_ops.join(" "));
                return Some(ScriptInfo {
                    address,
                    script_type: "non-standard".to_string(),
                    extra_data: Some(extra_data),
                });
            }
        }
    }

    // Generic non-standard script with a 20-byte hash (likely a pubkey hash)
    for (i, instruction) in instructions.iter().enumerate() {
        if matches!(instruction, Instruction::PushBytes(bytes) if bytes.len() == 20) {
            if let Instruction::PushBytes(hash_bytes) = instruction {
                // Create a hash160-based address
                let mut data = vec![0]; // Use mainnet P2PKH prefix
                data.extend_from_slice(hash_bytes.as_bytes());
                let address = base58::encode_check(&data);

                // Create extra data for analysis
                let script_ops: Vec<String> = instructions
                    .iter()
                    .map(|inst| match inst {
                        Instruction::PushBytes(bytes) => format!("PUSH({} bytes)", bytes.len()),
                        Instruction::Op(op) => format!("{:?}", op),
                        _ => "Unknown".to_string(),
                    })
                    .collect();

                let extra_data = serde_json::json!({
                    "pattern": "hash160-found",
                    "hash_position": i,
                    "script_ops": script_ops
                });

                debug!(
                    "Processing non-standard script with 20-byte hash: {}",
                    script_ops.join(" ")
                );
                return Some(ScriptInfo {
                    address,
                    script_type: "non-standard".to_string(),
                    extra_data: Some(extra_data),
                });
            }
        }
    }

    // If we still haven't found a match, hash the entire script
    let script_bytes = script.to_bytes();
    if !script_bytes.is_empty() {
        let script_hash = hash160::Hash::hash(&script_bytes);
        let mut data = vec![5]; // Use P2SH prefix
        data.extend_from_slice(&script_hash[..]);
        let address = base58::encode_check(&data);

        // Log the script pattern
        let script_ops: Vec<String> = instructions
            .iter()
            .map(|inst| match inst {
                Instruction::PushBytes(bytes) => format!("PUSH({} bytes)", bytes.len()),
                Instruction::Op(op) => format!("{:?}", op),
                _ => "Unknown".to_string(),
            })
            .collect();

        let extra_data = serde_json::json!({ "script_pattern": script_ops });

        debug!(
            "Handling unknown script with custom hash: {}",
            script_ops.join(" ")
        );
        return Some(ScriptInfo {
            address,
            script_type: "unknown".to_string(),
            extra_data: Some(extra_data),
        });
    }

    // Truly empty or invalid script
    None
}

/// Extract public key from input script if available
fn extract_public_key_from_script(script: &Script) -> Option<Vec<u8>> {
    let instructions = script
        .instructions()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();

    // P2PKH input script: <signature> <pubkey>
    if instructions.len() == 2
        && matches!(instructions[0], Instruction::PushBytes(_))
        && matches!(instructions[1], Instruction::PushBytes(_))
    {
        if let Instruction::PushBytes(pubkey_bytes) = &instructions[1] {
            if pubkey_bytes.len() == 33 || pubkey_bytes.len() == 65 {
                // Compressed (33 bytes) or uncompressed (65 bytes) public key
                return Some(pubkey_bytes.as_bytes().to_vec());
            }
        }
    }

    // No public key found
    None
}

/// Helper function to encode a bech32/bech32m address
/// Returns Result<String, String> to properly handle encoding errors
fn encode_bech32_address(hrp_str: &str, version_u8: u8, program: &[u8]) -> Result<String, String> {
    // Parse the HRP (Human Readable Part)
    let hrp = match hrp_str {
        "bc" => hrp::BC,
        _ => Hrp::parse(hrp_str).expect("Invalid HRP"),
    };

    // Handle known versions with constants
    if version_u8 == 0 {
        segwit::encode(hrp, segwit::VERSION_0, program)
            .map_err(|e| format!("Failed to encode SegWit v0 address: {}", e))
    } else if version_u8 == 1 {
        segwit::encode(hrp, segwit::VERSION_1, program)
            .map_err(|e| format!("Failed to encode SegWit v1 address: {}", e))
    } else {
        Err(format!("Unsupported witness version: {}", version_u8))
    }
}

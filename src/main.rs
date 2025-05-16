use anyhow::{Context, Result};
use dotenv::dotenv;
use std::env;
use std::time::Duration;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod bitcoin_client;
mod db;
mod processor;

fn run() -> Result<()> {
    info!("Entered run function");

    // Load environment variables
    dotenv().ok();
    info!("dotenv loaded");

    // Init DB connection
    let database_url = env::var("DATABASE_URL").context("DATABASE_URL must be set")?;
    info!("Read DATABASE_URL: {}", database_url);

    let db_pool = db::create_connection_pool(&database_url)
        .context("Failed to create database connection pool")?;
    info!("Database connection pool created");

    // Get a DB connection and run migrations
    info!("Attempting to get DB connection for migrations");
    let mut conn = db_pool
        .get()
        .context("Failed to get database connection for migrations")?;
    info!("Got DB connection for migrations");

    info!("Attempting to run migrations (in Rust app)");
    db::run_migrations(&mut conn).context("Failed to run database migrations")?;
    info!("Rust app migrations completed");

    // Init Bitcoin REST client
    let bitcoin_rest_url =
        env::var("BITCOIN_REST_URL").unwrap_or_else(|_| "http://127.0.0.1:8332".to_string());
    info!("Bitcoin REST URL: {}", bitcoin_rest_url);

    // Start tokio runtime for async operations
    info!("Creating tokio runtime");
    let rt = tokio::runtime::Runtime::new().context("Failed to create Tokio runtime")?;
    info!("Tokio runtime created");

    info!("Starting blockchain processing (rt.block_on)");
    rt.block_on(async {
        // Retry Bitcoin client connection with exponential backoff
        let mut retry_delay = Duration::from_secs(5);
        let max_retry_delay = Duration::from_secs(300); // 5 minutes

        info!("Starting Bitcoin REST client connection loop");
        let bitcoin_client = loop {
            info!("Attempting BitcoinClient::new()");
            let client_result = bitcoin_client::BitcoinClient::new(
                bitcoin_rest_url.clone(),
            ).await;
            info!("BitcoinClient::new() returned");

            match client_result {
                Ok(client) => {
                    info!("Successfully connected to Bitcoin REST API!");
                    break client;
                },
                Err(e) => {
                    error!("Failed to connect to Bitcoin REST API: {}. Retrying in {}s...", e, retry_delay.as_secs());
                    tokio::time::sleep(retry_delay).await;
                    retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
                }
            }
            info!("End of client connection loop iteration");
        };
        info!("Bitcoin REST client initialised");

        // Init and run the block processor
        info!("Initialising block processor");
        let processor = processor::BlockProcessor::new(bitcoin_client, db_pool.clone());

        // Phase 1: Catch-up to the current chain tip
        // Sync up to the current blockchain tip before proceeding
        // Handle both initial sync (when the DB is empty) and resuming (if process was stopped for some reason)
        let next_height_for_continuous_processing;
        loop {
            // Get current blockchain tip height from the Bitcoin node
            let current_node_tip_height = match processor.get_current_blockchain_tip().await {
                Ok(tip) => tip,
                Err(e) => {
                    error!("Failed to get current blockchain tip from node: {}. Retrying in 30 seconds...", e);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    continue; // Retry getting tip
                }
            };
            info!("Current Bitcoin node tip height: {}", current_node_tip_height);

            // Get the last block height processed and stored in DB
            let last_processed_block_height_db = {
                let mut conn = db_pool.get().context("Failed to get DB connection for sync check")?;
                db::get_last_processed_height(&mut conn)?
            };

            // Determine the next block to process. If DB is empty, start from 0
            // Otherwise, start from the block after the last processed one
            let next_block_to_process_if_needed: u64 = match last_processed_block_height_db {
                Some(db_height) => u64::from(db_height) + 1,
                None => 0,
            };

            // Handle the case where the database has entries
            if let Some(db_height) = last_processed_block_height_db {
                info!("Last processed block height in DB: {}. Node tip height: {}.", db_height, current_node_tip_height);
                // Check if already synced or ahead
                if u64::from(db_height) >= current_node_tip_height {
                    info!("Database is synced with (or ahead of) the current node tip.");
                    next_height_for_continuous_processing = db_height + 1;
                    break; // Exit catch-up loop, proceed to continuous processing.
                }
                // If not synced, we fall through to the processing logic below.
                // The "Database is behind..." log will be handled there.
            }
            // If last_processed_block_height_db was None, we proceed directly to processing from 0 (as next_block_to_process_if_needed would be 0).

            match last_processed_block_height_db {
                Some(db_height) => {
                    // This case implies db_height < current_node_tip_height due to the 'break' condition handled above.
                    info!("Database is behind. Last processed: {}, Node tip: {}. Attempting to sync missing blocks starting from {}.", db_height, current_node_tip_height, next_block_to_process_if_needed);
                }
                None => {
                    info!("No blocks processed yet (DB is empty/new). Starting initial sync from block {} up to node tip {}.", next_block_to_process_if_needed, current_node_tip_height);
                }
            }

            // Do the work!!!
            if let Err(e) = processor.process_all_blocks(next_block_to_process_if_needed).await {
                error!("Error during sync (process_all_blocks from {}): {:#}. Retrying...", next_block_to_process_if_needed, e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            } else {
                info!("Sync iteration (process_all_blocks from {}) completed. Re-checking status shortly.", next_block_to_process_if_needed);
                // Small delay to avoid tight looping if progress is slow or node tip hasn't updated.
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
        info!("Catch-up phase complete. Database is synced with the Bitcoin node tip.");

        // Phase 2: Continuous block processing
        // Start processing new blocks as they arrive, from the next height after what's been synced
        info!("Starting continuous block processing from height {}", next_height_for_continuous_processing);
        processor.process_new_blocks(next_height_for_continuous_processing).await
            .context("Failed during continuous block processing")?;

        Ok::<(), anyhow::Error>(())
    })?;
    info!("rt.block_on finished"); // Should not be reached if process_new_blocks loops

    Ok(())
}

fn main() {
    info!("Starting Bitcoin block and transaction processor");
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    if let Err(e) = run() {
        error!("Application error: {:#}", e);
        std::process::exit(1);
    }
    info!("Application has finished and is shutting down.");
}

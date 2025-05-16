use anyhow::{Context, Result};
use bitcoin::{consensus::Decodable, Block, BlockHash};
use hex;
use reqwest::Client;
use serde::Deserialize;
use std::io::Cursor;
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, error, info};

/// Represents the JSON response from /rest/chaininfo.json
#[derive(Deserialize, Debug)]
struct ChainInfo {
    chain: String,
    blocks: u64,
}

/// Client for interacting with Bitcoin Core via REST API
pub struct BitcoinClient {
    client: Client,
    base_url: String, // e.g., http://127.0.0.1:8332
}

impl BitcoinClient {
    /// Creates a new Bitcoin REST API client.
    /// The `url` should be the base URL of the Bitcoin Core REST interface (e.g., "http://127.0.0.1:8332").
    pub async fn new(url: String) -> Result<Self> {
        let client_builder = Client::builder().timeout(Duration::from_secs(30));
        let client = client_builder
            .build()
            .context("Failed to build reqwest client")?;

        let mut final_url = url;
        if !final_url.starts_with("http://") && !final_url.starts_with("https://") {
            final_url = format!("http://{}", final_url);
        }
        if final_url.ends_with('/') {
            final_url.pop(); // Remove trailing slash if present
        }

        debug!("Creating Bitcoin REST client with URL: {}", final_url);

        let instance = Self {
            client,
            base_url: final_url,
        };

        // Test connection by getting blockchain info
        match instance.get_chain_info().await {
            Ok(info_resp) => {
                info!(
                    "Connected to Bitcoin node via REST. Chain: {}, Blocks: {}",
                    info_resp.chain, info_resp.blocks
                );
                Ok(instance)
            }
            Err(e) => {
                error!("Failed to connect to Bitcoin REST API: {:?}", e);
                Err(anyhow::anyhow!(
                    "Failed to connect to Bitcoin REST API: {}",
                    e
                ))
            }
        }
    }

    async fn get_chain_info(&self) -> Result<ChainInfo> {
        let request_url = format!("{}/rest/chaininfo.json", self.base_url);
        debug!("Fetching chain info from: {}", request_url);

        let request_builder = self.client.get(&request_url);

        let response = request_builder
            .send()
            .await
            .context("Failed to send request to /rest/chaininfo.json")?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            error!(
                "Error response from /rest/chaininfo.json: {} - {}",
                status, err_text
            );
            return Err(anyhow::anyhow!(
                "Failed to get chain info: {} - {}",
                status,
                err_text
            ));
        }

        let chain_info_resp = response
            .json::<ChainInfo>()
            .await
            .context("Failed to deserialize chain info JSON response")?;

        Ok(chain_info_resp)
    }

    /// Get the current block count (blockchain height)
    pub async fn get_block_count(&self) -> Result<u64> {
        self.get_chain_info().await.map(|info| info.blocks)
    }

    /// Helper to make a GET request to a REST endpoint
    async fn rest_get(&self, path: &str) -> Result<reqwest::Response> {
        let request_url = format!("{}{}", self.base_url, path);
        debug!("Sending GET request to: {}", request_url);

        let request_builder = self.client.get(&request_url);

        let response = request_builder
            .send()
            .await
            .with_context(|| format!("Failed to send GET request to {}", path))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            error!("Error response from {}: {} - {}", path, status, err_text);
            return Err(anyhow::anyhow!(
                "REST request failed for {}: {} - {}",
                path,
                status,
                err_text
            ));
        }

        Ok(response)
    }

    /// Get block hash by height using /rest/blockhashbyheight/
    async fn get_block_hash_rest(&self, height: u64) -> Result<BlockHash> {
        let path = format!("/rest/blockhashbyheight/{}.hex", height);
        let response = self.rest_get(&path).await?;

        let hash_hex = response.text().await.with_context(|| {
            format!(
                "Failed to read block hash response text for height {}",
                height
            )
        })?;

        // Trim potential whitespace/newline
        let trimmed_hash_hex = hash_hex.trim();

        BlockHash::from_str(trimmed_hash_hex).with_context(|| {
            format!(
                "Failed to parse block hash hex '{}' for height {}",
                trimmed_hash_hex, height
            )
        })
    }

    /// Get a block by its height
    pub async fn get_block_by_height(&self, height: u64) -> Result<Block> {
        let hash = self
            .get_block_hash_rest(height)
            .await
            .context("Failed to get block hash via REST")?;
        self.get_block_by_hash(&hash).await
    }

    /// Get a block by its hash using /rest/block/
    pub async fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Block> {
        let path = format!("/rest/block/{}.hex", hash);
        let response = self.rest_get(&path).await?;

        let block_hex = response
            .text()
            .await
            .with_context(|| format!("Failed to read block response text for hash {}", hash))?;

        // Decode the hex string into bytes
        let block_bytes = hex::decode(block_hex.trim())
            .with_context(|| format!("Failed to decode block hex for hash {}", hash))?;

        // Deserialize the bytes into a Block object
        let mut cursor = Cursor::new(block_bytes);
        Block::consensus_decode(&mut cursor)
            .with_context(|| format!("Failed to deserialize block data for hash {}", hash))
    }
}

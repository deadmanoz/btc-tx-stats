# Bitcoin Transaction Analytics

A containerised system for collecting and analysing Bitcoin blockchain data using a Rust application with Diesel ORM and PostgreSQL database.

This is currently a work in progress and the schema is subject to change - probably incremental migrations won't be used and the initial migration will just be updated.

The original motivation for this project was to understand the extent of public key exposure on the Bitcoin blockchain.
Older script types such as P2PK and P2MS use public keys directly in the scriptPubKey, and so the public keys are directly exposed.
Similarly, P2TR has a "tweaked" public key in the scriptPubKey, and so is also considered exposed.
For other script types, the public key is revealed when "addresses are reused", that is,the public key becomes exposed on-chain during the spending process.

## Technologies Used

- **Rust** - Primary programming language
- **Diesel** - ORM and query builder with migrations
- **PostgreSQL** - Database for storing analytics
- **Docker** - Containerisation for easy deployment
- **Bitcoin Core REST API** - Interface with Bitcoin node

## Prerequisites

- Docker and Docker Compose
- Just command runner (optional, for running commands from the `justfile`)

## Project Structure

- `migrations/` - Diesel database migrations
- `scripts/` - Helper scripts for Docker
- `src/` - Rust application code
- `.env` - Environment variables configuration
- `Cargo.lock` - Rust dependencies lock file
- `Cargo.toml` - Rust project and dependencies configuration
- `compose.yaml` - Docker services configuration
- `diesel.toml` - Diesel ORM configuration
- `Dockerfile.app` - Main Rust application container
- `Dockerfile.diesel` - Container for DB migrations and schema generation
- `Dockerfile.rust` - Container for Rust toolchain (cargo fmt, clippy, etc.)
- `justfile` - Common operations runner
- `LICENSE` - Project license
- `README.md` - Project documentation

### Docker Containers
The project uses three different Docker containers:

- `Dockerfile.app`: Main Rust application container that runs the Bitcoin blockchain data processing application
- `Dockerfile.rust`: Container with Rust toolchain for development tasks (formatting, linting, checking)
- `Dockerfile.diesel`: Minimal container for database migrations and schema generation

## Getting Started

1. Clone this repository:
   ```
   git clone <repository-url>
   cd btc-tx-stats
   ```

2. Create a `.env` file with your Bitcoin node connection details:
   ```
   # Bitcoin Core RPC connection
   POSTGRES_USER=<username>
   POSTGRES_PASSWORD=<password>
   POSTGRES_DB=<database_name> # e.g. btc_analytics
   DATABASE_HOST=<host> # e.g. postgres
   DATABASE_PORT=<port> # e.g. 5432
   
   # Logging
   RUST_LOG=info
   ```

3. Start the Docker containers:
   ```
   just up
   ```
   
   Or without just:
   ```
   docker compose up -d
   ```

## Database Schema

The PostgreSQL database includes the following tables:

- `script_types` - Enum table containing Bitcoin script types (p2pkh, p2sh, p2pk, p2wpkh, p2wsh, p2tr, p2ms, non-standard, unknown)
- `blocks` - Core block data including height, hash, timestamp, and transaction count
- `transactions` - Stores transaction data with analytics (txid, block info, fees, input/output counts)
- `txid_block_index` - Lookup table mapping transaction IDs to block heights
- `addresses` - All unique addresses with script types, revealed public keys, and usage statistics
- `address_outputs` - Outputs associated with addresses (UTXOs and spent outputs)
- `address_inputs` - Inputs (spends) from addresses

## Working with Diesel Migrations

Diesel CLI is included in the Docker container for migrations:

- Generate migrations: `just db-migrate`
- Generate `schema.rs`: `just db-schema-generate`
- Run migrations and generate `schema.rs`: `just db-setup`

## Common Operations

The `justfile` provides shortcuts for common operations:

- `just` - List all available commands
- `just up` - Start containers
- `just up-debug` - Start containers with debug logging
- `just up-trace` - Start containers with trace logging
- `just up-logs` - Start containers and follow logs
- `just up-logs-debug` - Start containers with debug logging and follow logs
- `just up-logs-trace` - Start containers with trace logging and follow logs
- `just down` - Stop containers
- `just down-volumes` - Stop containers and remove volumes (clean slate)
- `just rebuild` - Rebuild containers
- `just restart` - Rebuild and restart containers
- `just restart-debug` - Rebuild and restart containers with debug logging
- `just logs` - View logs
- `just psql` - Enter PostgreSQL shell
- `just query "SELECT * FROM blocks LIMIT 10"` - Run a SQL query
- `just rust-shell` - Enter Rust app container
- `just rust-shell-debug` - Enter Rust app container with debug logging enabled
- `just check` - Check Rust compilation
- `just fmt` - Format Rust code
- `just fmt-check` - Check Rust code formatting
- `just lint` - Lint Rust code
- `just update-lockfile` - Update/Generate Cargo.lock based on Cargo.toml
- `just upgrade-deps` - Upgrade dependencies in Cargo.lock to latest compatible versions
- `just up-db` - Ensure PostgreSQL service is up and healthy
- `just db-migrate` - Run database migrations
- `just db-schema-generate` - Generate src/db/schema.rs
- `just db-setup` - Run migrations and generate schema

## Notes

Some elements of the DB schema might seem strange, there's sometimes logic to it (not always!).
In the case of the `transactions` table, one would think that the `transaction_id` alone could be a primary key,
but instead it's a composite primary key of `transaction_id` and `block_height`.

This is because of an [edge case in Bitcoin's history](https://bitcoindevs.xyz/decoding/inputs-prev-txid).
There was a unique situation in Bitcoin's history where the same transaction ID
(`e3bf3d07d4b0375638d5f1db5255fe07ba2c4cb067cd81b84ee974b6585fb468`) occurred in multiple blocks:
[91,722](https://mempool.space/block/00000000000271a2dc26e7667f8419f2e15416dc6955e5a6c6cdf3f2574dd08e) and
[91,880](https://mempool.space/block/00000000000743f190a18c5577a3c2d2a1f610ae9601ac046a38084ccb7cd721).
Aside: BIP 30 was implemented to prevent blocks from containing duplicate TXIDs.

## License

[MIT License](LICENSE)
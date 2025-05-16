# Bitcoin transaction analytics justfile

set dotenv-load := true

# List available commands
default:
    @just --list

# Start Docker containers
up:
    @echo "Starting Docker containers..."
    docker compose up -d

# Start Docker containers with debug logging
up-debug:
    @echo "Starting Docker containers with debug logging..."
    RUST_LOG=debug docker compose up -d

# Start Docker containers with trace logging (most verbose)
up-trace:
    @echo "Starting Docker containers with trace logging..."
    RUST_LOG=trace docker compose up -d

# Start Docker containers and follow logs
up-logs:
    @echo "Starting Docker containers and following logs..."
    docker compose up

# Start Docker containers with debug logging and follow logs
up-logs-debug:
    @echo "Starting Docker containers with debug logging and following logs..."
    RUST_LOG=debug docker compose up

# Start Docker containers with trace logging and follow logs
up-logs-trace:
    @echo "Starting Docker containers with trace logging and following logs..."
    RUST_LOG=trace docker compose up

# Stop Docker containers
down:
    @echo "Stopping Docker containers..."
    docker compose down

# Stop Docker containers and remove volumes
down-volumes:
    @echo "Stopping Docker containers and removing volumes..."
    docker compose down -v

# Rebuild containers
rebuild:
    @echo "Rebuilding Docker containers..."
    docker compose build

# Rebuild and restart containers
restart:
    @echo "Restarting Docker containers..."
    @just down
    @just rebuild
    @just up

# Rebuild and restart containers with debug logging
restart-debug:
    @echo "Restarting Docker containers with debug logging..."
    @just down
    @just rebuild
    @just up-debug

# View logs
logs:
    @echo "Following Docker container logs..."
    docker compose logs -f

# Enter PostgreSQL shell
psql:
    @echo "Entering PostgreSQL shell..."
    docker compose exec postgres psql -U btc_user -d btc_analytics

# Run a SQL query directly
query query:
    @echo "Running SQL query: {{query}}"
    docker compose exec postgres psql -U btc_user -d btc_analytics -c "{{query}}"

# Enter Rust app container
rust-shell:
    @echo "Entering Rust app container..."
    docker compose exec rust-app bash

# Enter Rust app container with debug logging enabled
rust-shell-debug:
    @echo "Entering Rust app container with debug logging..."
    docker compose exec -e RUST_LOG=debug rust-app bash

# Build the rust_tools_util Docker image always
build-rust-tools-image:
    @echo "Building rust_tools_util Docker image..."
    docker build -t rust_tools_util -f Dockerfile.rust .

# Check compilation
check: build-rust-tools-image
    @echo "Checking Rust compilation..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target" \
        -w /app rust_tools_util cargo check

# Format code
fmt: build-rust-tools-image
    @echo "Formatting Rust code..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target" \
        -w /app rust_tools_util cargo fmt

# Check code formatting
fmt-check: build-rust-tools-image
    @echo "Checking Rust code formatting..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target" \
        -w /app rust_tools_util cargo fmt -- --check

# Lint code
lint: build-rust-tools-image
    @echo "Linting Rust code..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target" \
        -w /app rust_tools_util cargo clippy

# Update/Generate Cargo.lock based on Cargo.toml
# Run this after modifying Cargo.toml to ensure Cargo.lock is synchronised
update-lockfile: build-rust-tools-image
    @echo "Updating Cargo.lock using 'cargo check' in a container..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target_lock_gen" \
        -w /app rust_tools_util cargo check
    @echo "Cargo.lock updated (if changes were needed)."

# Upgrade dependencies in Cargo.lock to the latest compatible versions
upgrade-deps: build-rust-tools-image
    @echo "Upgrading dependencies in Cargo.lock using 'cargo update' in a container..."
    docker run --rm --user "$(id -u):$(id -g)" \
        -v "$(pwd):/app" \
        -e "CARGO_TARGET_DIR=/app/target_lock_gen" \
        -w /app rust_tools_util cargo update
    @echo "Cargo.lock upgraded with latest compatible dependencies."

# Ensure PostgreSQL service is up and healthy by calling the helper script
up-db:
    @echo "Ensuring PostgreSQL service (btc-analytics-db) is running and healthy via script..."
    bash ./scripts/ensure_db_ready.sh

# Build the minimal diesel_cli_util Docker image if it doesn't exist
build-diesel-util-image:
    @if ! docker image inspect diesel_cli_util > /dev/null 2>&1; then \
        echo "INFO: diesel_cli_util image not found, building from Dockerfile.diesel..."; \
        docker build -t diesel_cli_util -f Dockerfile.diesel .; \
    else \
        echo "INFO: diesel_cli_util image already exists."; \
    fi

# Run database migrations using the standalone diesel_cli_util container
# This connects to the 'postgres' service on the 'btc_network'.
# DATABASE_URL is constructed here for the diesel_cli_util container.
db-migrate: up-db build-diesel-util-image
    @echo "Running database migrations using standalone diesel_cli_util container..."
    docker run --rm --network=btc_network \
        -v "$(pwd)/migrations:/app/migrations" \
        -v "$(pwd)/diesel.toml:/app/diesel.toml" \
        -w /app \
        -e DATABASE_URL="postgresql://$POSTGRES_USER:$POSTGRES_PASSWORD@postgres:5432/$POSTGRES_DB" \
        diesel_cli_util diesel migration run
    @echo "Database migrations complete."

# Regenerate src/db/schema.rs using the standalone diesel_cli_util container
# This connects to the 'postgres' service on the 'btc_network'.
# DATABASE_URL is constructed here for the diesel_cli_util container.
db-schema-generate: up-db build-diesel-util-image
    @echo "Generating database schema (src/db/schema.rs) using standalone diesel_cli_util container..."
    docker run --rm --network=btc_network \
        -v "$(pwd)/src/db:/app/src/db" \
        -v "$(pwd)/diesel.toml:/app/diesel.toml" \
        -w /app \
        -e DATABASE_URL="postgresql://$POSTGRES_USER:$POSTGRES_PASSWORD@postgres:5432/$POSTGRES_DB" \
        diesel_cli_util sh -c "diesel print-schema > /app/src/db/schema.rs"
    @echo "src/db/schema.rs generated."

# A combined command to migrate and then regenerate schema
db-setup: db-migrate db-schema-generate
    @echo "Database setup (migrate + schema generate) complete."

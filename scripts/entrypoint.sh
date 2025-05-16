#!/bin/bash
set -e

# Wait for Postgres to be ready
echo "Waiting for PostgreSQL to be ready..."
until PGPASSWORD=${POSTGRES_PASSWORD} psql -h "127.0.0.1" -U "${POSTGRES_USER}" -d "${POSTGRES_DB}" -c '\q'; do
  echo "PostgreSQL is unavailable - sleeping"
  sleep 1
done

# Run migrations using Diesel CLI
echo "PostgreSQL is up - running Diesel migrations"
export DATABASE_URL=postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:${DATABASE_PORT}/${POSTGRES_DB}
diesel migration run

echo "Starting Rust application..."
exec /app/btc-tx-stats

#!/bin/bash

set -e # Exit immediately if a command exits with a non-zero status.

DB_CONTAINER_NAME="btc-analytics-db"
NETWORK_NAME="btc_network"

# Ensure the Docker network exists
echo "INFO: Ensuring Docker network '${NETWORK_NAME}' exists..."
sudo docker network inspect "${NETWORK_NAME}" > /dev/null 2>&1 || \
    (echo "INFO: Network '${NETWORK_NAME}' not found. Creating..." && sudo docker network create "${NETWORK_NAME}")

echo "INFO: Checking status of PostgreSQL container '${DB_CONTAINER_NAME}'..."

# Check if container exists and is running
if ! sudo docker ps --filter "name=^/${DB_CONTAINER_NAME}$" --filter "status=running" --format "{{.Names}}" | grep -q "${DB_CONTAINER_NAME}"; then
    echo "INFO: PostgreSQL container '${DB_CONTAINER_NAME}' is not running. Attempting to start with 'sudo docker compose up -d postgres'..."
    # The postgres service in compose.yaml should be configured to use NETWORK_NAME
    sudo docker compose up -d postgres
    echo "INFO: Waiting for PostgreSQL container '${DB_CONTAINER_NAME}' to initialize and become healthy..."
    # Initial sleep to allow container to start, health check might not be immediate
    sleep 5 
fi

# Wait for health check to pass
echo "INFO: Actively polling health of PostgreSQL container '${DB_CONTAINER_NAME}'..."
retries=30 # Approx 5 minutes (30 * 10s)
count=0
until sudo docker inspect --format='{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "${DB_CONTAINER_NAME}" 2>/dev/null | grep -q 'healthy'; do
    count=$((count + 1))
    if [ ${count} -ge ${retries} ]; then
        echo "ERROR: PostgreSQL container '${DB_CONTAINER_NAME}' did not become healthy after ${retries} attempts."
        exit 1
    fi
    echo "INFO: PostgreSQL not healthy yet (attempt ${count}/${retries}). Retrying in 10s..."
    sleep 10
done

echo "INFO: PostgreSQL container '${DB_CONTAINER_NAME}' is healthy and ready."

exit 0 
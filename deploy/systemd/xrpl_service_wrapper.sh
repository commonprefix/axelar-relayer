#!/bin/bash
# This script expects one argument in the form "binaryName-environment_id"
# For example: "ingestor_testnet_1"

# Ensure an argument is provided.
if [ -z "$1" ]; then
  echo "Usage: $0 <binary_name>-<environment>-<id>"
  exit 1
fi

# Split the input into binary name, environment, and id using '_' as delimiter.
IFS='-' read -r binary_name environment instance_id <<< "$1"

# Validate that all parts are present.
if [ -z "$binary_name" ] || [ -z "$environment" ] || [ -z "$instance_id" ]; then
  echo "Error: Expecting argument in the form <binary_name>-<environment>-<id>" >&2
  exit 1
fi

# Build the paths based on the given parameters.
binary="/usr/local/bin/xrpl_${binary_name}_${environment}"

# Check if the binary exists.
if [ ! -f "$binary" ]; then
  echo "Error: Binary $binary not found" >&2
  exit 1
fi

export INSTANCE_ID="$instance_id"
export NETWORK="$environment"
export ENVIRONMENT="production"

echo "Starting binary: $binary for environment: $environment, instance id: $instance_id"

# Execute the selected binary.
exec "$binary"
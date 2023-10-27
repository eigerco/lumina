#!/bin/bash

set -euo pipefail

# Name for this node or `bridge-0` if not provided
NODE_ID="${NODE_ID:-0}"
NODE_NAME="bridge-$NODE_ID"
# a private local network
P2P_NETWORK="private"
# a bridge node configuration directory
CONFIG_DIR="$CELESTIA_HOME/.celestia-bridge-$P2P_NETWORK"
# directory and the files shared with the validator node
CREDENTIALS_DIR="/credentials"
# node credentials
NODE_KEY_FILE="$CREDENTIALS_DIR/$NODE_NAME.key"
NODE_JWT_FILE="$CREDENTIALS_DIR/$NODE_NAME.jwt"
# directory where validator will write the genesis hash
GENESIS_DIR="/genesis"
GENESIS_HASH_FILE="$GENESIS_DIR/genesis_hash"

# Wait for the validator to set up and provision us via shared dir
wait_for_provision() {
  echo "Waiting for the validator node to start"
  while [[ ! ( -e "$GENESIS_HASH_FILE" && -e "$NODE_KEY_FILE" ) ]]; do
    sleep 0.1
  done

  echo "Validator is ready"
}

# Import the test account key shared by the validator
import_shared_key() {
  echo "password" | cel-key import "$NODE_NAME" "$NODE_KEY_FILE" \
    --keyring-backend="test" \
    --p2p.network "$P2P_NETWORK" \
    --node.type bridge
}

add_trusted_genesis() {
  local genesis_hash

  # Read the hash of the genesis block
  genesis_hash="$(cat "$GENESIS_HASH_FILE")"
  # and make it trusted in the node's config
  echo "Trusting a genesis: $genesis_hash"
  sed -i'.bak' "s/TrustedHash = .*/TrustedHash = $genesis_hash/" "$CONFIG_DIR/config.toml"
}

write_jwt_token() {
  echo "Saving jwt token to $NODE_JWT_FILE"
  celestia bridge auth admin --p2p.network "$P2P_NETWORK" > "$NODE_JWT_FILE"
}

main() {
  # Initialize the bridge node
  celestia bridge init --p2p.network "$P2P_NETWORK"
  # Wait for a validator
  wait_for_provision
  # Import the key with the coins
  import_shared_key
  # Trust the private blockchain
  add_trusted_genesis
  # Update the JWT token
  write_jwt_token
  # give validator some time to set up
  sleep 4
  # Start the bridge node
  echo "Configuration finished. Running a bridge node..."
  celestia bridge start \
    --core.ip validator \
    --keyring.accname "$NODE_NAME" \
    --p2p.network "$P2P_NETWORK"
}

main

#!/bin/bash

set -xeuo pipefail

# a private local network
P2P_NETWORK="private"
# a bridge node configuration directory
CONFIG_DIR="$CELESTIA_HOME/.celestia-bridge-$P2P_NETWORK"
# a name of the user key
USER_NAME=test1
# directory and the files shared with the validator node
SHARED_DIR="/shared"
GENESIS_HASH_FILE="$SHARED_DIR/genesis_hash"
USER_KEY_FILE="$SHARED_DIR/$USER_NAME.keys"

# Wait for the validator to set up and provision us via shared dir
wait_for_provision() {
  echo "Waiting for the validator node to start"
  while [[ ! ( -e "$GENESIS_HASH_FILE" && -e "$USER_KEY_FILE" ) ]]; do
    sleep 0.1
  done

  sleep 3 # let the validator finish setup
  echo "Validator is ready"
}

# Import the test account key shared by the validator
import_shared_key() {
  echo "password" | cel-key import "$USER_NAME" "$USER_KEY_FILE" \
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

main() {
  # Wait for a validator
  wait_for_provision
  # Import the key with the coins
  import_shared_key
  # Initialize the bridge node
  celestia bridge init --p2p.network "$P2P_NETWORK"
  # Trust the private blockchain
  add_trusted_genesis
  # Start the bridge node
  echo "Configuration finished. Running a bridge node..."
  celestia bridge start \
    --core.ip validator \
    --keyring.accname "$USER_NAME" \
    --p2p.network "$P2P_NETWORK"
}

main

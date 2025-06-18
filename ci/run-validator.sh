#!/bin/bash

set -euxo pipefail

# Amount of DA nodes to setup, taken from the first argument
# or 1 if not provided
NODE_COUNT="${NODE_COUNT:-1}"
# a private local network
P2P_NETWORK="private"
# a validator node configuration directory
CONFIG_DIR="$CELESTIA_HOME/.celestia-app"
# the names of the keys
NODE_NAME=validator-0
# amounts of the coins for the keys
NODE_COINS="200000000000000utia"
VALIDATOR_COINS="1000000000000000utia"
# a directory and the files shared with the bridge nodes
CREDENTIALS_DIR="/credentials"
# directory where validator will write the genesis hash
GENESIS_DIR="/genesis"
GENESIS_HASH_FILE="$GENESIS_DIR/genesis_hash"

# Get the address of the node of given name
node_address() {
  local node_name="$1"
  local node_address

  node_address=$(celestia-appd keys show "$node_name" -a --keyring-backend="test")
  echo "$node_address"
}

# Waits for the given block to be created and returns it's hash
wait_for_block() {
  local block_num="$1"
  local block_hash=""

  # Wait for the block to be created
  while [[ -z "$block_hash" ]]; do
    # `|| echo` fallbacks to an empty string in case it's not ready
    # `celestia-appd` skips the block_id field so we use rest
    block_hash="$(curl -sS "http://localhost:26657/block?height=$block_num" 2>/dev/null | jq -r '.result.block_id.hash // ""' || echo)"
    sleep 0.1
  done

  echo "$block_hash"
}

# Saves the hash of the genesis node and the keys funded with the coins
# to the directory shared with the da node
provision_da_nodes() {
  local genesis_hash
  local last_node_idx=$((NODE_COUNT - 1))

  # Get or create the keys for DA nodes
  for node_idx in $(seq 0 "$last_node_idx"); do
    local node_name="node-$node_idx"
    local key_file="$CREDENTIALS_DIR/$node_name.key"
    local plaintext_key_file="$CREDENTIALS_DIR/$node_name.plaintext-key"
    local addr_file="$CREDENTIALS_DIR/$node_name.addr"

    if [ ! -e "$key_file" ]; then
      # if key don't exist yet, then create and export it
      echo "Creating a new keys for the $node_name"
      celestia-appd keys add "$node_name" --keyring-backend "test"
      # export it
      echo "password" | celestia-appd keys export "$node_name" --keyring-backend "test" > "$key_file.lock"
      # export also plaintext key for convenience in tests
      echo y | celestia-appd keys export "$node_name" --unsafe --unarmored-hex --keyring-backend "test" 2> "${plaintext_key_file}"
      # the `.lock` file and `mv` ensures that readers read file only after finished writing
      mv "$key_file.lock" "$key_file"
      # export associated address
      node_address "$node_name" > "$addr_file"
    else
      # otherwise, just import it
      echo "password" | celestia-appd keys import "$node_name" "$key_file" \
        --keyring-backend "test"
    fi
  done

  # Transfer the coins to DA nodes addresses
  # Coins transfer need to be after validator registers EVM address, which happens in block 2.
  # see `setup_private_validator`
  local start_block=3

  for node_idx in $(seq 0 "$last_node_idx"); do
    # TODO: create an issue in celestia-app and link it here
    # we need to transfer the coins for each node in separate
    # block, or the signing of all but the first one will fail
    wait_for_block $((start_block + node_idx))

    local node_name="node-$node_idx"
    local peer_addr

    peer_addr=$(node_address "$node_name")

    echo "Transfering $NODE_COINS coins to the $node_name"
    echo "y" | celestia-appd tx bank send \
      "$NODE_NAME" \
      "$peer_addr" \
      "$NODE_COINS" \
      --fees 21000utia \
      --keyring-backend "test"
  done

  # Save the genesis hash for the DA node
  genesis_hash=$(wait_for_block 1)
  echo "Saving a genesis hash to $GENESIS_HASH_FILE"
  echo "$genesis_hash" > "$GENESIS_HASH_FILE"

  echo "Provisioning finished."
}

# Set up the validator for a private alone network.
# Based on
# https://github.com/celestiaorg/celestia-app/blob/main/scripts/single-node.sh
setup_private_validator() {
  local validator_addr

  # Initialize the validator
  celestia-appd init "$P2P_NETWORK" --chain-id "$P2P_NETWORK"
  # Derive a new private key for the validator
  celestia-appd keys add "$NODE_NAME" --keyring-backend="test"
  validator_addr="$(node_address "$NODE_NAME")"
  # Create a validator's genesis account for the genesis.json with an initial bag of coins
  celestia-appd genesis add-genesis-account "$validator_addr" "$VALIDATOR_COINS"
  # Generate a genesis transaction that creates a validator with a self-delegation
  celestia-appd genesis gentx "$NODE_NAME" 5000000000utia \
    --fees 500utia \
    --keyring-backend="test" \
    --chain-id "$P2P_NETWORK"
  # Collect the genesis transactions and form a genesis.json
  celestia-appd genesis collect-gentxs

  # Set proper defaults and change ports
  # sed -i 's/localhost/0.0.0.0/' "$CONFIG_DIR/config/config.toml"
  # sed -i 's/127.0.0.1/0.0.0.0/' "$CONFIG_DIR/config/config.toml"
  dasel put -f "$CONFIG_DIR/config/config.toml" -t string -v 'tcp://0.0.0.0:9090' rpc.grpc_laddr
  dasel put -f "$CONFIG_DIR/config/config.toml" -t string -v 'tcp://0.0.0.0:26657' rpc.laddr
  dasel put -f "$CONFIG_DIR/config/config.toml" -t string -v 'tcp://0.0.0.0:26658' proxy_app
  # enable transaction indexing
  dasel put -f "$CONFIG_DIR/config/config.toml" -t string -v 'kv' tx_index.indexer

  # enable grpc-web
  dasel put -f "$CONFIG_DIR/config/app.toml" -t bool -v true grpc-web.enable
  # enable CORS as regular grpc is open
  dasel put -f "$CONFIG_DIR/config/app.toml" -t bool -v true grpc-web.enable-unsafe-cors
}

main() {
  # Configure stuff
  setup_private_validator
  # Spawn a job to provision a bridge node later
  provision_da_nodes &
  # Start the celestia-app
  echo "Configuration finished. Running a validator node..."
  celestia-appd start \
    --api.enable \
    --grpc.enable \
    --force-no-bbr
}

main

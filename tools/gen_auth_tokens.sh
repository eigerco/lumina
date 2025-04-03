#!/usr/bin/env bash
set -euxo pipefail

DOTENV=".env"
DOTENV_SAMPLE=".env.sample"
DOCKER_COMPOSE_FILE="./ci/docker-compose.yml"

wait_for_docker_setup() {
  local services_running
  local services_expected

  services_expected="$(docker compose -f "$DOCKER_COMPOSE_FILE" config --services | wc -l)"
  services_running="$(docker compose -f "$DOCKER_COMPOSE_FILE" ps --services | wc -l)"
  
docker compose -f "$DOCKER_COMPOSE_FILE" ps -a

  if [[ "$services_running" != "$services_expected" ]]; then
    echo "Not all required services running, expected $services_expected, found $services_running" >&2
    exit 1
  fi

  # wait for the service to start
  while :; do
    curl http://127.0.0.1:36658 > /dev/null 2>&1 && break
    docker logs ci-node-1-1
    sleep 10
  done
}

ensure_dotenv_file() {
  if [ ! -e "$DOTENV" ]; then
    if [ ! -e "$DOTENV_SAMPLE" ]; then
      echo "$DOTENV_SAMPLE file not found." \
        "Make sure to run this script from repository root" >&2
      exit 1
    fi

    echo "$DOTENV file not found, creating a new one"
    cp "$DOTENV_SAMPLE" "$DOTENV"
  else
    echo "Found existing $DOTENV file"
  fi
}

generate_token() {
  local auth_level="$1"
  local node_type
  node_type="$(docker compose -f "$DOCKER_COMPOSE_FILE" exec -T node-1 \
    ls -a /root | grep private | cut -d- -f 2)"
  docker compose -f "$DOCKER_COMPOSE_FILE" exec -T node-1 \
    celestia "$node_type" auth "$auth_level" --p2p.network private
}

write_token() {
  local auth_level="$1"
  local token="$2"

  auth_level="$(echo "$auth_level" | tr '[:lower:]' '[:upper:]')"

  local var_name="CELESTIA_NODE_AUTH_TOKEN_${auth_level}"

  sed -i.bak "s/.*$var_name.*/$var_name=$token/" "$DOTENV"
  # there's no compatible way to tell sed not to do a backup file
  # accept it and remove the file afterwards
  rm "$DOTENV.bak"
}

main() {
  wait_for_docker_setup
  ensure_dotenv_file

  for auth_level in "read" "write" "admin"; do
    local token
    token="$(generate_token "$auth_level")"
    write_token "$auth_level" "$token"
  done
}

main

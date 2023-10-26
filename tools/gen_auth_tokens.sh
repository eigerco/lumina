#!/bin/bash
set -euo pipefail

DOTENV=".env"
DOTENV_SAMPLE=".env.sample"
DOCKER_COMPOSE_FILE="./ci/docker-compose.yml"

wait_for_docker_setup() {
 # we follow the logs with -f and then kill it with awk exit so we
 # need to suppress the exit status of the docker compose command
 # but we check the correctness with the grep instead
 set +o pipefail
  docker compose -f "$DOCKER_COMPOSE_FILE" logs -f |
    awk '/Configuration finished. Running a bridge/ {print; exit}' |
    grep -o Configuration >/dev/null
 set -o pipefail
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
  docker-compose -f "$DOCKER_COMPOSE_FILE" exec -T bridge-0 \
    celestia bridge auth "$auth_level" --p2p.network private
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

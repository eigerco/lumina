#!/bin/bash
set -euo pipefail

DOTENV=".env"
DOTENV_SAMPLE=".env.sample"
DOCKER_COMPOSE_FILE="./ci/docker-compose.yml"

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
  docker-compose -f "$DOCKER_COMPOSE_FILE" exec -T bridge \
    celestia bridge auth "$auth_level" --p2p.network private
}

write_token() {
  local auth_level="$1" $(echo "$1" | tr '[:lower:]' '[:upper:]')
  local token="$2"

  local auth_level=$(echo "$auth_level" | tr '[:lower:]' '[:upper:]')

  local var_name="CELESTIA_NODE_AUTH_TOKEN_${auth_level}"

  sed -i '.bak' -e "s/.*$var_name.*/$var_name=$token/" "$DOTENV"
  rm "$DOTENV.bak" # there's no compatible way to tell sed not to do a backup file
                   # accept it and remove the file afterwards
}

main() {
  ensure_dotenv_file

  for auth_level in "read" "write" "admin"; do
    local token
    token="$(generate_token "$auth_level")"
    write_token "$auth_level" "$token"
  done
}

main

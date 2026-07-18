#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT_DIR/examples/.uhb"
BASE_URL="http://www.iso20022.org/15022/uhb/"
INDEX_OUTPUT="$OUT_DIR/uhb-index.md"
JINA_MIRROR_PREFIX="https://r.jina.ai/http://"
CURL_MAX_TIME=12
CURL_CONNECT_TIMEOUT=4
CURL_RETRY_COUNT=1

mkdir -p "$OUT_DIR"

if ! command -v rg >/dev/null 2>&1; then
  echo "Error: required 'rg' command not found" >&2
  exit 1
fi

resolve_url() {
  local target="$1"

  case "$target" in
    mt[0-9][0-9][0-9]|finmt[0-9][0-9][0-9]|[0-9][0-9][0-9])
      if [[ "$target" == mt* ]]; then
        target="finmt${target#mt}"
      elif [[ "$target" == [0-9][0-9][0-9] ]]; then
        target="finmt${target}"
      fi
      echo "https://www.iso20022.org/15022/uhb/$target.htm"
      ;;
    http://*|https://*)
      echo "$target"
      ;;
    *)
      echo "https://www.iso20022.org/15022/uhb/$target"
      ;;
  esac
}

canonicalize_uhb_url() {
  local raw_url="$1"
  echo "${raw_url//\/uhb\/uhb\//\/uhb\/}"
}

fetch_with_fallback() {
  local url="$1"
  local out_file="$2"
  local tmp_file
  tmp_file="$(mktemp)"
  local normalized_url
  local mirror_url
  local -a attempts

  normalized_url="$(canonicalize_uhb_url "$url")"
  mirror_url="$(mirror_url "$normalized_url")"

  echo "Downloading: $url"
  if [[ "$normalized_url" == *"iso20022.org/"* ]]; then
    attempts=("$mirror_url" "$normalized_url")
  else
    attempts=("$normalized_url" "$mirror_url")
  fi

  for candidate in "${attempts[@]}"; do
    if curl_fetch "$candidate" "$tmp_file"; then
      mv "$tmp_file" "$out_file"
      return 0
    fi
  done

  if [[ "${USE_PLAYWRIGHT:-}" == "1" ]]; then
    if playwright_fetch "$normalized_url" "$tmp_file"; then
      mv "$tmp_file" "$out_file"
      return 0
    fi
  fi

  rm -f "$tmp_file"
  return 1
}

mirror_url() {
  local source_url="$1"
  if [[ "$source_url" == https://* ]]; then
    echo "https://r.jina.ai/http://${source_url#https://}"
  elif [[ "$source_url" == http://* ]]; then
    echo "https://r.jina.ai/${source_url}"
  else
    echo "${JINA_MIRROR_PREFIX}${source_url}"
  fi
}

curl_fetch() {
  local source="$1"
  local out_file="$2"

  local code=0
  curl \
    --silent \
    --fail \
    --location \
    --max-time "${CURL_MAX_TIME}" \
    --connect-timeout "${CURL_CONNECT_TIMEOUT}" \
    --retry "${CURL_RETRY_COUNT}" \
    --retry-delay 1 \
    --retry-all-errors \
    -A "swiftpipe-fetch/1.0" \
    "$source" \
    --output "$out_file" \
  || code=$?

  return "$code"
}

playwright_fetch() {
  local url="$1"
  local out_file="$2"

  if ! command -v node >/dev/null 2>&1; then
    echo "Node.js unavailable for Playwright fallback for $url." >&2
    return 1
  fi

  if ! node -e "import('playwright').then(() => process.exit(0)).catch(() => process.exit(1))"; then
    echo "Playwright fallback unavailable for $url (module not present)." >&2
    return 1
  fi

  node --input-type=module - "$url" "$out_file" <<'NODE'
import { chromium } from 'playwright';
import fs from 'node:fs';

const [_, __, targetUrl, destination] = process.argv;
if (!targetUrl || !destination) {
  process.exit(1);
}

const browser = await chromium.launch({ headless: true });
const page = await browser.newPage();

try {
  await page.goto(targetUrl, { waitUntil: 'networkidle', timeout: 60000 });
  const html = await page.content();
  await fs.promises.writeFile(destination, html);
} finally {
  await browser.close();
}
NODE
}

url_to_output_name() {
  local target="$1"
  local base_name
  base_name="$(basename "${target%%\?*}")"
  echo "${base_name%.htm}.md"
}

collect_target_urls() {
  rg -o 'https?://www\.iso20022\.org/[^)]*finmt[0-9]{3}\.htm' "$INDEX_OUTPUT" \
    | tr -d '\r' \
    | sort -u \
    | while IFS= read -r link; do
      [[ -z "$link" ]] && continue
      canonicalize_uhb_url "$link"
    done
}

extract_common_group_urls() {
  local source_file="$1"
  if [[ ! -f "$source_file" ]]; then
    return 0
  fi

  rg -o 'https?://www\.iso20022\.org/[^)]*finmtn[0-9]{2}\.htm' "$source_file" \
    | tr -d '\r' \
    | sort -u \
    | while IFS= read -r link; do
      [[ -z "$link" ]] && continue
      canonicalize_uhb_url "$link"
    done
}

declare -a urls=()
declare -a seen_urls=()

has_seen_url() {
  local target="$1"

  if [[ ${#seen_urls[@]-0} -eq 0 ]]; then
    return 1
  fi

  local existing
  for existing in "${seen_urls[@]}"; do
    [[ "$existing" == "$target" ]] && return 0
  done
  return 1
}

enqueue_url() {
  local target
  target="$(resolve_url "$1")"
  if has_seen_url "$target"; then
    return 0
  fi
  seen_urls+=("$target")
  urls+=("$target")
}

discover_and_enqueue_common_groups() {
  local source_file="$1"
  while IFS= read -r common_url; do
    [[ -z "$common_url" ]] && continue
    enqueue_url "$common_url"
  done < <(extract_common_group_urls "$source_file")
}

if [[ $# -eq 0 ]]; then
  if ! fetch_with_fallback "$BASE_URL" "$INDEX_OUTPUT"; then
    echo "Failed to download $BASE_URL" >&2
    exit 1
  fi

  while IFS= read -r link; do
    [[ -z "$link" ]] && continue
    enqueue_url "$link"
  done < <(collect_target_urls)
else
  for mt in "$@"; do
    enqueue_url "$mt"
  done
fi

if [[ -n "${urls[*]-}" ]]; then
  for ((i = 0; i < ${#urls[@]}; i++)); do
    target="${urls[$i]}"
    output_name="$(url_to_output_name "$target")"
    output_path="$OUT_DIR/$output_name"
    fetch_with_fallback "$target" "$output_path" || {
      echo "Skipping failed download: $target" >&2
      continue
    }
    discover_and_enqueue_common_groups "$output_path"
  done
else
  echo "No message URLs were discovered for download." >&2
  exit 0
fi

echo "Downloaded ${#urls[@]} UHB message pages into $OUT_DIR"

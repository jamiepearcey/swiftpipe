#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UHBS_DIR="$ROOT_DIR/examples/.uhb"
SCHEMA_DIR="$ROOT_DIR/examples/schemas"
TEST_FILE="$ROOT_DIR/crates/swift-schema/tests/spec_reproduction.rs"
SAMPLE_DIR="$ROOT_DIR/examples"

if ! [[ -f "$TEST_FILE" ]]; then
  echo "Test file not found: $TEST_FILE" >&2
  exit 1
fi

if ! [[ -d "$UHBS_DIR" ]]; then
  echo "UHB cache directory not found: $UHBS_DIR" >&2
  echo "Run ./scripts/fetch-uhb-specs.sh first."
  exit 1
fi

uhb_parse_status() {
  local file="$1"
  awk '
    BEGIN {
      field_rows = 0
      tag_rows = 0
      has = 0
      in_format = 0
    }

    function trim(s) {
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", s)
      return s
    }

    /\| Status \| Tag \|/ {
      has = 1
      in_format = 1
      next
    }

    in_format && /^## MT/ && /Network Validated Rules/ {
      exit
    }

    in_format && /^\|/ {
      line = $0
      sub(/^\|/, "", line)
      sub(/\|$/, "", line)
      n = split(line, cells, /\|/)
      first = trim(cells[1])
      second = trim(cells[2])

      if (first == "M" || first == "O" || first == "C") {
        field_rows++
      }

      if (n > 1 && first != "" && second != "") {
        tag_rows++
      }
    }

    END {
      print field_rows, tag_rows, has + 0
    }
  ' "$file"
}

is_parseable_uhb_file() {
  local file="$1"
  [[ -f "$file" ]] || return 1
  read -r field_rows tag_rows has_header <<< "$(uhb_parse_status "$file")"
  [[ "$has_header" == "1" && "$field_rows" -gt 0 && "$tag_rows" -gt 0 ]]
}

extract_common_group_urls() {
  local source_file="$1"
  if [[ ! -f "$source_file" ]]; then
    return 0
  fi

  grep -Eo 'https?://www\.iso20022\.org/[^)]*finmtn[0-9]{2}\.htm' "$source_file" \
    | tr -d '\r' \
    | sort -u \
    | while IFS= read -r link; do
      [[ -z "$link" ]] && continue
      echo "$link"
    done
}

uhb_parseable_for_mt() {
  local mt="$1"
  local primary_file="$UHBS_DIR/finmt${mt}.md"

  if is_parseable_uhb_file "$primary_file"; then
    return 0
  fi

  while IFS= read -r group_url; do
    local group_name
    group_name="$(basename "${group_url%%\?*}")"
    group_name="${group_name%.htm}.md"
    if is_parseable_uhb_file "$UHBS_DIR/$group_name"; then
      return 0
    fi
  done < <(extract_common_group_urls "$primary_file")

  return 1
}

printf -- "Message  Cached  Sample  Schema  Test  Parsed  Declared\n"
printf -- "-------  ------  ------  ------  ----  ------  ------\n"

for uhb in "$UHBS_DIR"/finmt*.md; do
  [[ "$uhb" == *"/uhb-index.md" ]] && continue
  base="$(basename "$uhb")"
  mt="$(echo "$base" | sed -E 's/^finmt([0-9]+)\.md$/\1/')"
  if ! [[ "$mt" =~ ^[0-9]+$ ]]; then
    continue
  fi

  msg="MT${mt}"
  cached="yes"
  sample="no"
  schema="no"
  test="no"
  declared="no"
  parsed="no"

  [[ -f "$SAMPLE_DIR/mt${mt}_sample.fin" ]] && sample="yes"
  if grep -Eq "message: ${msg}" "$SCHEMA_DIR"/*.yaml; then
    schema="yes"
  fi
  if grep -Eq "fn reproduces_${msg}" "$TEST_FILE" || grep -Eq "fn reproduces_mt${mt}" "$TEST_FILE"; then
    test="yes"
  fi
  if grep -Eq "fn reproduces_mt${mt}" "$TEST_FILE"; then
    declared="yes"
  fi
  if uhb_parseable_for_mt "$mt"; then
    parsed="yes"
  fi

  printf -- "MT%-5s %-7s %-7s %-7s %-4s %-7s %-7s\n" \
    "$mt" "$cached" "$sample" "$schema" "$test" "$parsed" "$declared"
done | sort

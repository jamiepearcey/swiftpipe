#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_FILE="$SCRIPT_DIR/../crates/swift-schema/tests/spec_reproduction.rs"

if ! [[ -f "$TEST_FILE" ]]; then
  echo "Test file not found: $TEST_FILE" >&2
  exit 1
fi

collect_declared_cases() {
  grep -E 'message: "MT[0-9]+' "$TEST_FILE" \
    | sed -E 's/.*"(MT[0-9]+)".*/\1/' \
    | sort -u
}

collect_cases() {
  collect_declared_cases
}

case_name_for_mt() {
  local mt="$1"
  local normalized
  normalized="$(printf '%s' "$mt" | tr '[:lower:]' '[:upper:]')"
  normalized="${normalized#MT}"
  normalized="${normalized#MT}"

  collect_declared_cases | grep -Ex "MT${normalized}" || true
}

run_validate_once() {
  cargo run -p swift-cli -- schema validate examples/schemas
  cargo run -p swift-cli -- schema render-validate examples/schemas
  cargo run -p swift-cli -- schema coverage examples/schemas
}

run_spec_case() {
  local case_name="$1"
  echo "== reproduction case: ${case_name} =="
  SPEC_REPRO_CASE="$case_name" cargo test -p swift-schema --test spec_reproduction -- reproduces_single_declared_spec_case --exact
}

CASES=()
while IFS= read -r case_name; do
  CASES+=("$case_name")
done < <(collect_cases)

if [[ "${CASES[*]-}" == "" ]]; then
  echo "No reproduction test cases discovered." >&2
  exit 1
fi

if [[ $# -eq 0 ]]; then
  TARGETS=("${CASES[@]}")
else
  TARGETS=()
  for arg in "$@"; do
    local_case="$(case_name_for_mt "$arg")"
    if [[ -z "$local_case" ]]; then
      if printf '%s\n' "${CASES[@]}" | grep -Exq "$arg"; then
        local_case="$arg"
      else
        echo "Unknown spec case '${arg}'" >&2
        exit 1
      fi
    fi
    TARGETS+=("$local_case")
  done
fi

DEDUPED_TARGETS=()
while IFS= read -r target; do
  DEDUPED_TARGETS+=("$target")
done < <(printf '%s\n' "${TARGETS[@]}" | awk '!seen[$0]++')
TARGETS=("${DEDUPED_TARGETS[@]}")

if [[ ${#TARGETS[@]} -eq 0 ]]; then
  echo "No reproduction targets matched." >&2
  exit 1
fi

if [[ "${RUN_VALIDATE_ONLY:-0}" == "1" ]]; then
  run_validate_once
  exit 0
fi

for case_name in "${TARGETS[@]}"; do
  run_spec_case "$case_name"
  if [[ "${RUN_VALIDATE_AFTER_CASE:-0}" == "1" ]]; then
    run_validate_once
  fi
  echo "== done ${case_name} =="
done

if [[ "${RUN_VALIDATE_AFTER_CASE:-0}" != "1" ]]; then
  run_validate_once
fi

echo "== replay complete =="

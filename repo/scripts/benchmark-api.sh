#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/benchmark-api.sh [options]

Benchmarks the self-hosted swift-api artifact paths. It generates benchmark data
under .swiftpipe-bench/ by default; no large files are committed.

Options:
  --target-bytes BYTES       Total corpus size for prefix benchmark. Supports K/M/G suffixes.
                             Default: 10485760 (10 MiB)
  --object-bytes BYTES       Approximate object size for prefix benchmark. Default: 1048576 (1 MiB)
  --single-upload-bytes BYTES
                             Size of single large upload benchmark payload. Default: disabled
  --listen HOST:PORT         API listen address. Default: 127.0.0.1:18081
  --bench-root PATH          Benchmark working directory. Default: .swiftpipe-bench
  --sample PATH              FIN sample to pad. Default: examples/mt540_sample.fin
  --message-type TYPE        Message type. Default: MT540
  --outputs LIST             Comma-separated output selector. Default: all
  --system-of-record MODE    System-of-record sink: none, file, postgres, sqlserver. Default: none
  --system-record-file PATH  File sink JSONL path when MODE=file.
  --postgres-connection STR  Postgres libpq connection string when MODE=postgres.
  --system-record-schema S   System-of-record schema. Default: swiftpipe
  --sqlserver-connection STR SQL Server connection string when MODE=sqlserver.
  --profile PROFILE          Cargo profile: release or debug. Default: release
  --api-binary PATH          Use this swiftpipe-api binary instead of target/{profile}/swiftpipe-api.
  --skip-build               Do not run cargo build before starting the API.
  --generate-only            Generate corpus/payload files and summary, but do not start the API.
  --persist-raw-text         Pass through to swiftpipe-api and persist full raw FIN text in DuckDB exports.
  --persist-raw-fields       Pass through to swiftpipe-api and persist raw field values in DuckDB exports.
  --keep-server              Leave the API process running after the benchmark.
  --help                     Show this help.

Examples:
  scripts/benchmark-api.sh
  scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M
  scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --single-upload-bytes 500M
  scripts/benchmark-api.sh --target-bytes 500M --object-bytes 4M --generate-only
USAGE
}

json_string() {
  python3 - "$1" <<'PY'
import json, sys
print(json.dumps(sys.argv[1]))
PY
}

parse_bytes() {
  local value="$1"
  case "$value" in
    *[Kk]) echo $((${value%?} * 1024)) ;;
    *[Mm]) echo $((${value%?} * 1024 * 1024)) ;;
    *[Gg]) echo $((${value%?} * 1024 * 1024 * 1024)) ;;
    *) echo "$value" ;;
  esac
}

now_ms() {
  python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

json_file_as_string() {
  local file="$1"
  if [[ -n "$file" && -f "$file" ]]; then
    python3 - "$file" <<'PY'
import json, sys
print(json.dumps(open(sys.argv[1]).read()))
PY
  else
    echo "null"
  fi
}

human_bytes() {
  python3 - "$1" <<'PY'
import sys
n = float(sys.argv[1])
for unit in ["B", "KiB", "MiB", "GiB"]:
    if n < 1024 or unit == "GiB":
        print(f"{n:.2f} {unit}")
        break
    n /= 1024
PY
}

throughput_mib_s() {
  python3 - "$1" "$2" <<'PY'
import sys
bytes_ = float(sys.argv[1])
ms = float(sys.argv[2])
print("0.000" if ms <= 0 else f"{bytes_ / 1024 / 1024 / (ms / 1000):.3f}")
PY
}

target_bytes=$((10 * 1024 * 1024))
object_bytes=$((1024 * 1024))
single_upload_bytes=0
listen="127.0.0.1:18081"
bench_root=".swiftpipe-bench"
sample="examples/mt540_sample.fin"
message_type="MT540"
outputs="all"
system_of_record="none"
system_record_file=""
postgres_connection=""
system_record_schema="swiftpipe"
sqlserver_connection=""
profile="release"
api_binary=""
skip_build=0
generate_only=0
keep_server=0
persist_raw_text=0
persist_raw_fields=0
invocation="scripts/benchmark-api.sh $*"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target-bytes) target_bytes="$(parse_bytes "$2")"; shift 2 ;;
    --object-bytes) object_bytes="$(parse_bytes "$2")"; shift 2 ;;
    --single-upload-bytes) single_upload_bytes="$(parse_bytes "$2")"; shift 2 ;;
    --listen) listen="$2"; shift 2 ;;
    --bench-root) bench_root="$2"; shift 2 ;;
    --sample) sample="$2"; shift 2 ;;
    --message-type) message_type="$2"; shift 2 ;;
    --outputs) outputs="$2"; shift 2 ;;
    --system-of-record) system_of_record="$2"; shift 2 ;;
    --system-record-file) system_record_file="$2"; shift 2 ;;
    --postgres-connection) postgres_connection="$2"; shift 2 ;;
    --system-record-schema) system_record_schema="$2"; shift 2 ;;
    --sqlserver-connection) sqlserver_connection="$2"; shift 2 ;;
    --profile)
      profile="$2"
      if [[ "$profile" != "release" && "$profile" != "debug" ]]; then
        echo "--profile must be release or debug" >&2
        exit 2
      fi
      shift 2
      ;;
    --api-binary) api_binary="$2"; shift 2 ;;
    --skip-build) skip_build=1; shift ;;
    --generate-only) generate_only=1; shift ;;
    --persist-raw-text) persist_raw_text=1; shift ;;
    --persist-raw-fields) persist_raw_fields=1; shift ;;
    --keep-server) keep_server=1; shift ;;
    --help|-h) usage; exit 0 ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ ! -f "$sample" ]]; then
  echo "sample not found: $sample" >&2
  exit 1
fi

mkdir -p "$bench_root"
bench_root="$(cd "$bench_root" && pwd)"
object_root="$bench_root/objects"
work_root="$bench_root/work"
data_root="$bench_root/data"
results_dir="$bench_root/results"
server_log="$bench_root/swiftpipe-api.log"
mkdir -p "$object_root" "$work_root" "$data_root" "$results_dir"

sample_abs="$(cd "$(dirname "$sample")" && pwd)/$(basename "$sample")"
run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
prefix_bucket="swiftpipe-bench-inbox"
prefix_key="runs/$run_id/prefix"
prefix_dir="$object_root/$prefix_bucket/$prefix_key"
output_prefix="s3://swiftpipe-bench-outbox/jobs/$run_id/"
prefix_uri="s3://$prefix_bucket/$prefix_key/"
summary_json="$results_dir/$run_id.json"

rm -rf "$prefix_dir"
mkdir -p "$prefix_dir"

if [[ -z "$api_binary" ]]; then
  if [[ "$profile" == "release" ]]; then
    api_binary="target/release/swiftpipe-api"
  else
    api_binary="target/debug/swiftpipe-api"
  fi
fi

if [[ "$generate_only" -eq 0 && "$skip_build" -eq 0 ]]; then
  if [[ "$profile" == "release" ]]; then
    echo "Building swiftpipe-api release binary..."
    cargo build -p swift-api --release >/dev/null
  else
    echo "Building swiftpipe-api debug binary..."
    cargo build -p swift-api >/dev/null
  fi
fi

if [[ "$generate_only" -eq 0 && ! -x "$api_binary" ]]; then
  echo "API binary not executable: $api_binary" >&2
  echo "Build it first, pass --api-binary, or use --generate-only." >&2
  exit 1
fi

sample_prefix="$data_root/$run_id-sample-prefix.fin"
perl -0pe 's/\n?-}\s*$/\n/' "$sample_abs" >"$sample_prefix"
sample_prefix_bytes="$(wc -c <"$sample_prefix" | tr -d ' ')"
sample_suffix_bytes=4

generate_padded_fin() {
  local output="$1"
  local bytes="$2"
  if [[ "$bytes" -le "$((sample_prefix_bytes + sample_suffix_bytes))" ]]; then
    cp "$sample_abs" "$output"
    return
  fi

  local filler_bytes=$((bytes - sample_prefix_bytes - sample_suffix_bytes))
  cp "$sample_prefix" "$output"
  printf ':70E::SPRO//BENCHMARK-PADDING\n' >>"$output"
  if [[ "$filler_bytes" -gt 29 ]]; then
    yes 'BENCHMARK PADDING 0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ' \
      | head -c "$((filler_bytes - 29))" >>"$output" || true
  fi
  printf '\n-}\n' >>"$output"
}

echo "Generating prefix corpus: $(human_bytes "$target_bytes") target, $(human_bytes "$object_bytes") per object..."
generated_bytes=0
object_count=0
generate_start_ms="$(now_ms)"
while [[ "$generated_bytes" -lt "$target_bytes" ]]; do
  object_count=$((object_count + 1))
  remaining=$((target_bytes - generated_bytes))
  this_size="$object_bytes"
  if [[ "$remaining" -lt "$this_size" ]]; then
    this_size="$remaining"
  fi
  object_path="$prefix_dir/msg-$(printf '%06d' "$object_count").fin"
  generate_padded_fin "$object_path" "$this_size"
  generated_bytes=$((generated_bytes + this_size))
done
generate_end_ms="$(now_ms)"

payload="$data_root/$run_id-prefix-job.json"
cat >"$payload" <<JSON
{
  "input_prefix": "$prefix_uri",
  "include_suffix": ".fin",
  "output_prefix": "$output_prefix",
  "message_type": "$message_type",
  "render_validate": true,
  "outputs": $(python3 - "$outputs" <<'PY'
import json, sys
value = sys.argv[1]
print(json.dumps(["all"] if value == "all" else [part for part in value.split(",") if part]))
PY
)
}
JSON

prefix_curl="curl -fsS -H 'content-type: application/json' --data-binary @$payload http://$listen/api/jobs"

single_file=""
single_status="skipped"
if [[ "$single_upload_bytes" -gt 0 ]]; then
  single_file="$data_root/$run_id-single-upload.fin"
  echo "Generating single upload payload: $(human_bytes "$single_upload_bytes")..."
  generate_padded_fin "$single_file" "$single_upload_bytes"
  single_status="generated"
fi

single_curl=""
if [[ "$single_upload_bytes" -gt 0 ]]; then
  single_curl="curl -fsS -X POST --data-binary @$single_file http://$listen/api/upload?message_type=$message_type$(if [[ "$outputs" != "all" ]]; then printf '&outputs=%s' "$outputs"; fi)"
fi

prefix_ms=0
prefix_response=""
prefix_response_json="null"
single_ms=0
single_response=""
single_response_json="null"
server_pid=""

cleanup() {
  if [[ -n "$server_pid" && "$keep_server" -eq 0 ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

if [[ "$generate_only" -eq 0 ]]; then
  max_upload_bytes=$((single_upload_bytes > target_bytes ? single_upload_bytes : target_bytes))
  max_upload_bytes=$((max_upload_bytes + 16 * 1024 * 1024))

  echo "Starting API on http://$listen ..."
  api_args=(
    --listen "$listen"
    --schema-path examples/schemas
    --object-root "$object_root"
    --work-root "$work_root"
    --max-upload-bytes "$max_upload_bytes"
  )
  if [[ "$persist_raw_text" -eq 1 ]]; then
    api_args+=(--persist-raw-text)
  fi
  if [[ "$persist_raw_fields" -eq 1 ]]; then
    api_args+=(--persist-raw-fields)
  fi
  api_args+=(--system-of-record "$system_of_record")
  if [[ -n "$system_record_file" ]]; then
    api_args+=(--system-record-file "$system_record_file")
  fi
  if [[ -n "$postgres_connection" ]]; then
    api_args+=(--postgres-connection "$postgres_connection")
  fi
  api_args+=(--system-record-schema "$system_record_schema")
  if [[ -n "$sqlserver_connection" ]]; then
    api_args+=(--sqlserver-connection "$sqlserver_connection")
  fi
  "$api_binary" \
    "${api_args[@]}" \
    >"$server_log" 2>&1 &
  server_pid=$!

  for _ in $(seq 1 100); do
    if curl -fsS "http://$listen/" >/dev/null 2>&1; then
      break
    fi
    if ! kill -0 "$server_pid" >/dev/null 2>&1; then
      echo "swiftpipe-api exited before becoming ready. Log:" >&2
      sed -n '1,120p' "$server_log" >&2 || true
      exit 1
    fi
    sleep 0.1
  done
  if ! curl -fsS "http://$listen/" >/dev/null; then
    echo "swiftpipe-api did not become ready on http://$listen. Log:" >&2
    sed -n '1,120p' "$server_log" >&2 || true
    exit 1
  fi

  echo "Running prefix benchmark for $object_count object(s), $(human_bytes "$generated_bytes")..."
  prefix_response="$results_dir/$run_id-prefix-response.json"
  prefix_start_ms="$(now_ms)"
  curl -fsS \
    -H 'content-type: application/json' \
    --data-binary "@$payload" \
    "http://$listen/api/jobs" \
    >"$prefix_response"
  prefix_end_ms="$(now_ms)"
  prefix_ms=$((prefix_end_ms - prefix_start_ms))
  prefix_response_json="$(json_file_as_string "$prefix_response")"

  if [[ "$single_upload_bytes" -gt 0 ]]; then
    echo "Running single upload benchmark..."
    single_response="$results_dir/$run_id-single-upload-response.json"
    single_start_ms="$(now_ms)"
    if curl -fsS \
      -X POST \
      --data-binary "@$single_file" \
      "http://$listen/api/upload?message_type=$message_type$(if [[ "$outputs" != "all" ]]; then printf '&outputs=%s' "$outputs"; fi)" \
      >"$single_response"; then
      single_status="completed"
    else
      single_status="failed"
    fi
    single_end_ms="$(now_ms)"
    single_ms=$((single_end_ms - single_start_ms))
    single_response_json="$(json_file_as_string "$single_response")"
  fi
fi

cat >"$summary_json" <<JSON
{
  "run_id": "$run_id",
  "invocation": $(json_string "$invocation"),
  "listen": "$listen",
  "object_root": "$object_root",
  "work_root": "$work_root",
  "sample": "$sample_abs",
  "message_type": "$message_type",
  "profile": "$profile",
  "api_binary": "$api_binary",
  "generate_only": $generate_only,
  "persist_raw_text": $persist_raw_text,
  "persist_raw_fields": $persist_raw_fields,
  "outputs": $(json_string "$outputs"),
  "system_of_record": $(json_string "$system_of_record"),
  "system_record_file": $(json_string "$system_record_file"),
  "system_record_schema": $(json_string "$system_record_schema"),
  "commands": {
    "start_api": $(json_string "$api_binary --listen $listen --schema-path examples/schemas --object-root $object_root --work-root $work_root --max-upload-bytes $max_upload_bytes$(if [[ "$persist_raw_text" -eq 1 ]]; then printf ' --persist-raw-text'; fi)$(if [[ "$persist_raw_fields" -eq 1 ]]; then printf ' --persist-raw-fields'; fi) --system-of-record $system_of_record$(if [[ -n "$system_record_file" ]]; then printf ' --system-record-file %s' "$system_record_file"; fi) --system-record-schema $system_record_schema$(if [[ -n "$postgres_connection" ]]; then printf ' --postgres-connection %s' "$postgres_connection"; fi)$(if [[ -n "$sqlserver_connection" ]]; then printf ' --sqlserver-connection %s' "$sqlserver_connection"; fi)"),
    "prefix_job": $(json_string "$prefix_curl"),
    "single_upload": $(json_string "$single_curl")
  },
  "prefix": {
    "target_bytes": $target_bytes,
    "generated_bytes": $generated_bytes,
    "object_bytes": $object_bytes,
    "object_count": $object_count,
    "generate_ms": $((generate_end_ms - generate_start_ms)),
    "generate_mib_s": $(throughput_mib_s "$generated_bytes" "$((generate_end_ms - generate_start_ms))"),
    "process_ms": $prefix_ms,
    "throughput_mib_s": $(throughput_mib_s "$generated_bytes" "$prefix_ms"),
    "input_prefix": "$prefix_uri",
    "output_prefix": "$output_prefix",
    "payload_file": "$payload",
    "response_file": "$prefix_response",
    "response_json_string": $prefix_response_json
  },
  "single_upload": {
    "status": "$single_status",
    "bytes": $single_upload_bytes,
    "process_ms": $single_ms,
    "throughput_mib_s": $(throughput_mib_s "$single_upload_bytes" "$single_ms"),
    "input_file": "$single_file",
    "response_file": "$single_response",
    "response_json_string": $single_response_json
  }
}
JSON

echo
echo "Benchmark complete."
echo "Summary: $summary_json"
echo "Server log: $server_log"
echo
python3 - "$summary_json" <<'PY'
import json, sys
summary = json.load(open(sys.argv[1]))
prefix = summary["prefix"]
single = summary["single_upload"]
print(f"prefix: {prefix['generated_bytes']} bytes, {prefix['object_count']} objects, "
      f"{prefix['process_ms']} ms, {prefix['throughput_mib_s']} MiB/s")
if single["status"] != "skipped":
    print(f"single_upload: {single['bytes']} bytes, {single['status']}, "
          f"{single['process_ms']} ms, {single['throughput_mib_s']} MiB/s")
PY

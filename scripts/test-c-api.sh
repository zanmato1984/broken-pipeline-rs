#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

cargo build -p broken-pipeline-capi
cmake -S "$repo_root/crates/broken-pipeline-capi/tests/c" \
  -B "$repo_root/target/c-api-tests" \
  -DBROKEN_PIPELINE_CAPI_LIB="$repo_root/target/debug/libbroken_pipeline_capi.so" \
  -DBROKEN_PIPELINE_CAPI_INCLUDE_DIR="$repo_root/crates/broken-pipeline-capi/include"
cmake --build "$repo_root/target/c-api-tests"
ctest --test-dir "$repo_root/target/c-api-tests" --output-on-failure

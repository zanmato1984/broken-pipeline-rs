#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build_dir="$repo_root/target/c-tests"

cargo build -p broken-pipeline-c
cmake -S "$repo_root/crates/broken-pipeline-c/tests/c" \
  -B "$build_dir" \
  -DBROKEN_PIPELINE_C_LIB="$repo_root/target/debug/libbroken_pipeline_c.so" \
  -DBROKEN_PIPELINE_C_INCLUDE_DIR="$repo_root/crates/broken-pipeline-c/include"
cmake --build "$build_dir"
ctest --test-dir "$build_dir" --output-on-failure

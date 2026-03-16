#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
required_files=(
  README.md
  LICENSE
  .gitignore
  include/broken_pipeline/broken_pipeline_c.h
  docs/overview.md
  docs/runtime-seam.md
  docs/build-seam.md
  docs/ownership.md
  docs/extensions.md
  docs/versioning.md
  docs/conformance.md
  docs/examples/host-provider-sketch.md
)

for rel in "${required_files[@]}"; do
  if [ ! -f "$repo_dir/$rel" ]; then
    echo "missing: $rel" >&2
    exit 1
  fi
done

if command -v rg >/dev/null 2>&1; then
  rg -q "BP_ABI_VERSION_MAJOR" "$repo_dir/include/broken_pipeline/broken_pipeline_c.h"
  rg -q "provider-specific API" "$repo_dir/docs/overview.md"
  rg -q "task/task-group semantics" "$repo_dir/docs/runtime-seam.md"
else
  grep -q "BP_ABI_VERSION_MAJOR" "$repo_dir/include/broken_pipeline/broken_pipeline_c.h"
  grep -q "provider-specific API" "$repo_dir/docs/overview.md"
  grep -q "task/task-group semantics" "$repo_dir/docs/runtime-seam.md"
fi

echo "validation ok"

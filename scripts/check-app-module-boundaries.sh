#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_root="$repo_root/crates/pharness-api/src/app"
failed=0

check_limit() {
  local file="$1"
  local limit="$2"
  local lines
  lines="$(wc -l < "$file" | tr -d ' ')"
  if (( lines > limit )); then
    echo "module size limit exceeded: ${file#"$repo_root/"} has $lines lines (limit $limit)" >&2
    failed=1
  fi
}

check_limit "$app_root/mod.rs" 1500

while IFS= read -r -d '' file; do
  case "$file" in
    "$app_root/tests/"*) check_limit "$file" 4000 ;;
    "$app_root/mod.rs") ;;
    *) check_limit "$file" 3500 ;;
  esac
done < <(find "$app_root" -type f -name '*.rs' -print0)

if rg -n '(^|[[:space:]])(pub[[:space:]]+)?use[[:space:]]+[^;]*::\*' "$app_root" -g '*.rs'; then
  echo "wildcard imports and re-exports are forbidden under crates/pharness-api/src/app" >&2
  failed=1
fi

if find "$app_root" -type f -name 'utils.rs' -print -quit | grep -q .; then
  echo "generic utils.rs modules are forbidden; use a named ownership module" >&2
  failed=1
fi

if ! "$repo_root/scripts/app-module-dependencies.py" --check >/dev/null; then
  failed=1
fi

if ! PYTHONDONTWRITEBYTECODE=1 python3 "$repo_root/scripts/test_app_module_dependencies.py"; then
  failed=1
fi

if (( failed != 0 )); then
  exit 1
fi

echo "app module boundaries are valid"

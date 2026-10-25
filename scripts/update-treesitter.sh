#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
    echo "Usage: $0 <tree-sitter-runtime-version> <tree-sitter-cli-version> [grammar-ref]" >&2
    exit 2
fi

runtime_version="$1"
cli_version="$2"
grammar_ref="${3:-main}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "$script_dir/.." && pwd)"
grammar_dir="$project_root/grammar"
grammar_url='https://codeberg.org/grammar-orchard/tree-sitter-json5-orchard.git'
temp_root="$(mktemp -d)"
trap 'rm -rf "$temp_root"' EXIT

for tool in git cargo; do
    command -v "$tool" >/dev/null 2>&1 || { echo "$tool was not found on PATH." >&2; exit 1; }
done

installed_cli_version=''
if command -v tree-sitter >/dev/null 2>&1; then
    installed_cli_version="$(tree-sitter --version | awk '{print $2}')"
fi
if [[ "$installed_cli_version" != "$cli_version" ]]; then
    cargo install tree-sitter-cli --version "$cli_version" --locked --force
    cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
    export PATH="$cargo_bin:$PATH"
fi
installed_cli_version="$(tree-sitter --version | awk '{print $2}')"
if [[ "$installed_cli_version" != "$cli_version" ]]; then
    echo "Expected Tree-sitter CLI $cli_version, found $installed_cli_version." >&2
    exit 1
fi

git clone "$grammar_url" "$temp_root/upstream"
git -C "$temp_root/upstream" fetch origin "$grammar_ref"
git -C "$temp_root/upstream" checkout --detach FETCH_HEAD
(
    cd "$temp_root/upstream"
    tree-sitter generate
)

upstream="$temp_root/upstream"
for file in src/parser.c src/node-types.json src/tree_sitter/parser.h queries/highlights.scm; do
    [[ -f "$upstream/$file" ]] || { echo "Missing generated grammar file: $file" >&2; exit 1; }
done
grep -Fq 'tree_sitter_json5_orchard' "$upstream/src/parser.c" || {
    echo 'The generated parser does not export tree_sitter_json5_orchard.' >&2
    exit 1
}

cd "$project_root"
cargo add "tree-sitter@=$runtime_version" --package gpui-json5-processor
cargo update -p tree-sitter --precise "$runtime_version"

cp "$upstream/src/parser.c" "$grammar_dir/src/parser.c"
cp "$upstream/src/node-types.json" "$grammar_dir/src/node-types.json"
rm -f "$grammar_dir/src/scanner.c"
if [[ -f "$upstream/src/scanner.c" ]]; then
    cp "$upstream/src/scanner.c" "$grammar_dir/src/scanner.c"
fi
rm -rf "$grammar_dir/src/tree_sitter"
mkdir -p "$grammar_dir/src/tree_sitter"
if [[ -d "$upstream/src/tree_sitter" ]]; then
    cp -a "$upstream/src/tree_sitter/." "$grammar_dir/src/tree_sitter/"
fi
rm -rf "$grammar_dir/queries"
mkdir -p "$grammar_dir/queries"
cp -a "$upstream/queries/." "$grammar_dir/queries/"
for file in grammar.js tree-sitter.json; do
    if [[ -f "$upstream/$file" ]]; then
        cp "$upstream/$file" "$grammar_dir/$file"
    fi
done
if [[ -f "$upstream/src/grammar.json" ]]; then
    cp "$upstream/src/grammar.json" "$grammar_dir/src/grammar.json"
fi
if [[ -f "$upstream/LICENSE" ]]; then
    cp "$upstream/LICENSE" "$grammar_dir/LICENSE"
fi
git -C "$upstream" rev-parse HEAD > "$grammar_dir/UPSTREAM_REVISION"

echo "Updated Tree-sitter runtime to $runtime_version and JSON5 grammar to $grammar_ref."
echo 'Next, run npm run build:wasm from the www directory to verify WASM compatibility.'

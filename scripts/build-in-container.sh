#!/usr/bin/env bash
set -euo pipefail

BUILD_PROFILE="${1:-release}"
case "$BUILD_PROFILE" in
    release) ARTIFACT_DIRECTORY=release ;;
    dev) ARTIFACT_DIRECTORY=debug ;;
    *) echo 'Expected release or dev build profile.' >&2; exit 1 ;;
esac

rustup update nightly --no-self-update
rustup target add wasm32-unknown-unknown --toolchain nightly
wasm-opt --version

build_module() {
    local package="$1" module="$2" output="$3"
    cargo +nightly rustc --locked --profile "$BUILD_PROFILE" \
        -p "$package" --target wasm32-unknown-unknown \
        -- -C link-arg=-zstack-size=8388608
    wasm-bindgen "/target/wasm32-unknown-unknown/$ARTIFACT_DIRECTORY/$module.wasm" \
        --out-dir "$output" --target web --no-typescript

    if [[ "$BUILD_PROFILE" == release ]]; then
        local wasm="$output/${module}_bg.wasm"
        local before
        before="$(wc -c < "$wasm")"
        wasm-opt -Os --strip-debug --strip-producers "$wasm" -o "$wasm.optimized"
        mv -- "$wasm.optimized" "$wasm"
        printf '%s: %s -> %s bytes after wasm-opt\n' "$module" "$before" "$(wc -c < "$wasm")"
    fi
}

build_module json5-web-editor gpui_json5_editor_web /workspace/standalone/www/src/wasm
build_module gpui-json5-processor gpui_json5_processor /workspace/standalone/www/src/worker-wasm

cargo +nightly run --locked --manifest-path /workspace/standalone/tools/sbom/Cargo.toml -- /workspace/standalone

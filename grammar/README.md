# JSON5 grammar

This directory contains a local fork of the
[tree-sitter-json5-orchard crate](https://crates.io/crates/tree-sitter-json5-orchard)
by Antonin Delpeuch ([wetneb](https://github.com/wetneb)). Its upstream source is
[grammar-orchard/tree-sitter-json5-orchard](https://codeberg.org/grammar-orchard/tree-sitter-json5-orchard).
The parser, queries and Rust bindings are bundled here and adapted for this
editor's WebAssembly build. The grammar is licensed under [MIT](LICENSE);
inherited copyright notices are preserved in that file.

## Updating

Use the update scripts from the parent directory.

On Windows:

```shell
.\scripts\update-treesitter.bat <runtime-version> <cli-version> [grammar-ref]
```

On macOS or Linux:

```sh
bash ./scripts/update-treesitter.sh <runtime-version> <cli-version> [grammar-ref]
```

Replace the first two arguments with the desired Rust `tree-sitter` runtime
and `tree-sitter-cli` versions. The optional grammar ref is an upstream branch,
tag or commit and defaults to `main`. Prefer an exact commit for reproducibility,
and choose a CLI that generates a parser ABI supported by the runtime.

The scripts install the selected CLI, clone upstream, check out the requested
ref and regenerate the parser. They update the worker's runtime dependency and
Cargo lockfile, replace the generated C parser, headers, queries and available
grammar metadata, and copy the upstream license. They record the resolved
commit in `UPSTREAM_REVISION`, while preserving this fork's `Cargo.toml` and
`bindings/rust/` files.

Review the resulting diff: generated files and queries are replaced, so any
local changes there may need to be reapplied. Keep the Rust binding compatible
with `tree-sitter-language`, preserve the `tree_sitter_json5_orchard` parser
symbol, and review the WASM header integration in `bindings/rust/build.rs` when
upgrading the runtime.

Then rebuild from `../www/`:

```shell
npm run build
```

The WASM build requires a running Docker or Podman engine; see the
[project build instructions](../README.md).

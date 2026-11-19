# JSON5 Editor

A browser-based JSON editor with support for **JSON5 syntax**, including comments,
trailing commas, single-quoted strings and JavaScript-like object syntax with
unquoted property names. It provides syntax highlighting, folding, search and
replace, and JSON conversion. Files can be saved with their original formatting.

The editor is based on **GPUI**, the Rust UI framework by **Zed Industries**, and
is heavily inspired by **GPUI Kit**, including code copied and adapted from that
project. Most of the application is Rust compiled to **WebAssembly**, using
**wasm-bindgen** to connect it to the browser. Parsing runs in a web worker.

- [GPUI by Zed Industries](https://github.com/zed-industries/zed/tree/main/crates/gpui):
  [Apache-2.0](https://github.com/zed-industries/zed/blob/main/LICENSE-APACHE).
- [GPUI Kit](https://github.com/longbridge/gpui-kit):
  [Apache-2.0](https://github.com/longbridge/gpui-kit/blob/main/LICENSE-APACHE).
- [wasm-bindgen](https://github.com/wasm-bindgen/wasm-bindgen):
  [MIT](https://github.com/wasm-bindgen/wasm-bindgen/blob/main/LICENSE-MIT) or
  [Apache-2.0](https://github.com/wasm-bindgen/wasm-bindgen/blob/main/LICENSE-APACHE).

Additional credits and the generated software bill of materials are available
in the editor's **About** dialog.

The Rust editor crate lives in `apps/editor/`. [`editor/`](editor/README.md)
contains the standalone web app, the shared browser runtime and the npm build
toolchain. The browser extensions in `extensions/editor/` use the same WASM,
document worker, fonts and UI and supply their response stream through the
startup API.

## Build requirements

- Node.js 24+ and npm.
- A running **container engine**: [Podman](https://podman.io/docs/installation)
  or [Docker](https://docs.docker.com/get-started/get-docker/).

Rust, LLVM/Clang, wasm-bindgen and wasm-opt are installed inside the build
container; no local Rust toolchain is needed for a normal build. The first build
needs internet access and takes longer; named volumes cache the toolchain and
compiled dependencies. The project directory is mounted to write the WASM output.

Cargo defaults to `wasm32-unknown-unknown` for the editor and document worker.
The build runs the SBOM generator on the host target.

The build selects the wasm-bindgen CLI version from `Cargo.lock`, keeping it
matched to the Rust dependency. Builds use `--locked`; after updating Rust
dependencies with `cargo update`, rebuild normally to use the matching CLI.

### Container setup

The build automatically uses an available Podman engine, then Docker. To choose
explicitly, set `CONTAINER_ENGINE=podman` or `CONTAINER_ENGINE=docker` in your shell.

## Build and run

From this directory:

```shell
cd editor
npm ci
npm run dev
```

This builds the WASM modules and starts the development server at
<http://localhost:3010>. For a production build:

```shell
npm run build
```

Serve the generated `editor/dist/` directory with a static web server, or use
`npm run preview` locally.

`npm run dev:debug` builds faster, unoptimized WASM.

`npm run build:wasm` rebuilds only WASM. After a JavaScript/CSS-only change.

`npm run build:web` reuses the existing WASM files and needs no container.


## HTTP client

Build the web app from `editor/` with `npm run build:http-client`. Then run the
Salvo server from `http_client/proxy/` with a local Rust toolchain:

```shell
cargo run --locked
```

Open <http://127.0.0.1:8080>. The server serves `http_client/dist/` and forwards
requests through `POST /proxy` using the client's timeout and redirect settings.
To use a different port, run `cargo run --locked -- 9000`.

To work with sample data, run `npm run preview:http-client` from `editor/` and
open <http://localhost:3011/?preview>. The WASM crate lives in `apps/http_client/`.

## Browser extension

After installing the dependencies above, build both extension packages from
the `extensions/editor/` directory:

```shell
cd ../extensions/editor
npm run build:all
```

This produces `extensions/editor/dist/` for Chromium and
`extensions/editor/dist-firefox/` for Firefox.

See the [extension README](extensions/editor/README.md) for loading instructions.

## Release artifacts

- Build the standalone app and both browser extensions first.
- From the project root, run `node scripts/generate-release-artifacts.mjs`,
  `scripts\generate-release-artifacts.bat` on Windows, or
  `sh scripts/generate-release-artifacts.sh` on macOS/Linux. Node.js 24+ is required;
  no additional dependencies are needed.
- The script packages the contents of `extensions/editor/dist/`,
  `extensions/editor/dist-firefox/`, and `editor/dist/` with maximum ZIP compression as
  `chrome-extension-unpacked.zip`, `firefox-extension-unpacked.zip`, and
  `standalone.zip`, respectively, in the project root.
- It also copies `assets/sbom.json` to the project root as `sbom.json`.
  All four generated files are ignored by Git and overwritten on subsequent runs.


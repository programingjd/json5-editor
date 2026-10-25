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

The shared browser editor lives in [`editor/`](editor/README.md). The standalone
app in `www/` and the browser extensions in `extensions/` each have a small
entry point that imports it. Both use the same WASM, document worker, fonts and
UI; the extension supplies its response stream through the startup API.

## Build requirements

- Node or compatible runtime.
- A running **container engine**: [Podman](https://podman.io/docs/installation)
  or [Docker](https://docs.docker.com/get-started/get-docker/).

Rust, LLVM/Clang, wasm-bindgen and wasm-opt are installed inside the build
container; no local Rust toolchain is needed for a normal build. The first build
needs internet access and takes longer; named volumes cache the toolchain and
compiled dependencies. The project directory is mounted to write the WASM output.

### Container setup

The build automatically uses an available Podman engine, then Docker. To choose
explicitly, set `CONTAINER_ENGINE=podman` or `CONTAINER_ENGINE=docker` in your shell.

## Build and run

From this directory:

```shell
cd www
npm ci
npm run dev
```

This builds the WASM modules and starts the development server at
<http://localhost:3010>. For a production build:

```shell
npm run build
```

Serve the generated `www/dist/` directory with a static web server, or use
`npm run preview` locally.

`npm run dev:debug` builds faster, unoptimized WASM.

`npm run build:wasm` rebuilds only WASM. After a JavaScript/CSS-only change.

`npm run build:web` reuses the existing WASM files and needs no container.


## Browser extension

After installing the dependencies above, build both extension packages from 
the `extensions/` directory:

```shell
cd ../extensions
npm run build:all
```

This produces `extensions/dist/` for Chromium and `extensions/dist-firefox/` for
Firefox.

See the [extension README](extensions/README.md) for loading instructions.


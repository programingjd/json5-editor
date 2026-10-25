# Shared browser editor

This module contains the browser runtime used by both the web app and the
browser extensions: GPUI startup, document processing, the parser worker,
font loading, file operations and loading/error surfaces. Rust remains in
the parent project's `src/` and `worker/` crates; their generated WASM bindings
are written to `editor/src/wasm/` and `editor/src/worker-wasm/`.

The public startup API is `startEditor({ initialDocument })` in `src/editor.js`.
The optional document can be a `File`, a response stream, a promise for either,
or `null` to use the editor's example. Response streams supply `name`, `read()`
and `cancel()`; `takePort()` transfers the data channel to the parser worker,
and `close()` releases the control channel. Loading starts after GPUI is ready.

`www/src/main.js` starts the standalone app. `extensions/src/main.js` passes
the extension's response-host adapter to the same API. Both use `index.html`
from this module, with their own launcher selected by the build configuration;
neither depends on the other launcher or its generated output.

`vite.config.mjs` provides their shared build configuration, including the
compatibility adapters for pinned GPUI-generated bindings in `build/`. Build
dependencies are installed once from `www/package-lock.json`. The shared module
has no npm runtime dependencies.

The inline stylesheet in `index.html` selects `--color-scheme` through the
system media query. Setting `data-color-scheme-override="light"` or `"dark"` on `<html>`
overrides that variable. `src/initial-theme.js` restores the saved attribute
before first paint. It is emitted as a separate synchronous script because
Manifest V3 forbids inline JavaScript. `src/color-scheme.js` persists toggle choices and reports
the computed CSS result to GPUI; it does not use JavaScript media queries.
The invisible CSS size signal keeps GPUI synchronized with system changes even
while a manual override is active. Choosing the system scheme clears the
attribute and saved override.

See the [project README](../README.md) for build commands. Existing worker and
document-client tests live beside their implementation here; they are not run
as part of a normal build.

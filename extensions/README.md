# JSON5 Editor browser extension

Builds separate Manifest V3 extensions for Chromium and Firefox. Both package
`../www/dist` and run the same GPUI editor, parser worker, fonts,
folding, diagnostics, search, and save UI.

- `dist/`: Chrome, Edge and compatible Chromium 128+ browsers, using a service
  worker and declarative response-header rules.
- `dist-firefox/`: Firefox Desktop 140+, using module background scripts and
  blocking response-header rules.

Firefox supports both MV2 and MV3, but uses background scripts rather than
Chromium's extension service worker. The Firefox package uses MV3 with its own
permissions, background declaration, add-on ID, and no-data-collection declaration.
There is no Chromium MV2 package.

## Build and load

Windows and POSIX build scripts are available:
`scripts/build.bat` and `scripts/build.sh`.

This rebuilds the parent project (including optimized WASM) once, then packages both
distributions. 

To package an already rebuilt parent project distribution:

```sh
npm run build
```

For Chromium, open `chrome://extensions` or `edge://extensions`, enable
**Developer mode**, choose **Load unpacked**, and select **../extensions/dist**.

For Firefox, open `about:debugging#/runtime/this-firefox`, choose **Load Temporary
Add-on**, and select **../extensions/dist-firefox/manifest.json**. Allow access to
the endpoint's host if Firefox asks. Temporary add-ons are removed on browser
restart; permanent installation requires a signed package.

Edit `manifest.template.json` for shared/Chromium manifest settings and
`scripts/manifests.mjs` for Firefox overrides. Generated files are ignored by Git
and replaced on each build. Each distribution is self-contained; distribute only
the chosen distribution's contents. After rebuilding, reload the extension in
the browser's extension tools, then navigate to a JSON endpoint.
The extension's toolbar button opens the editor with its normal example.

## Response handling

Top-level HTTP/HTTPS navigations are matched by their response Content-Type,
not their URL or file extension. Matching is case-insensitive and accepts MIME
parameters. Supported types:

- `application/json` and `application/json5`
- `text/json` and `text/json5`
- Structured application types ending in `+json` or `+json5`

Fetch/XHR requests, embedded frames, ordinary HTML, and non-JSON downloads are
left alone. File URLs and browser-internal pages are outside this adapter.

Matching JSON/JSON5 document responses are rendered as UTF-8 plain text. As soon
as a document body exists, the content script mounts the editor, observes the
growing response text, and reads newly appended characters in bounded chunks.
It does not wait for `DOMContentLoaded` to start the editor, although the body
may become available only after initial response bytes arrive. Both browsers
use the same original-response DOM reader, without a response filter, network
suspend/resume loop, second request, or separate background-to-content body
transport.

This suppresses the built-in JSON formatter and JSON5 download handling.
The endpoint is not fetched a second time, so authenticated, POST, and
single-use responses retain their original content.
JSON is UTF-8; legacy non-UTF-8 response encodings are not supported by this
adapter. HTTP errors with JSON bodies are displayed and invalid JSON/JSON5 is
opened for the editor's diagnostics rather than rejected by the extension.

The editor fills the response page in an extension-origin iframe. The address
bar and history keep the original endpoint URL, including its query and fragment.
Reload and Back retain their normal browser behavior; reloading requests the
original endpoint again. The extension frame keeps the editor's scripts and
WebGPU context separate from the endpoint, including HTTP endpoints in desktop
Chromium. No navigation to an extension page is made.

The original response filename comes from Content-Disposition, then the URL.
Only decoded text is captured, not wire bytes, original encodings, or transport
compression. The extension does not intercept fetch/XHR streaming API traffic. Responses
served entirely by another service worker may bypass declarative header rules;
rendered JSON documents still have a MIME-based content-script fallback.

### Receiving a response

A private `MessageChannel` carries a pull protocol directly to the document
worker, with one read of at most 64 Ki UTF-16 units outstanding.
A separate control port keeps Abort
available after ownership of the response port moves to the worker.
The worker accumulates
the original UTF-8 source once. A separate incremental lexical formatter produces
indented preview chunks in the worker, retaining string, escape, and comment
state across chunks. This prevents a minified response from becoming one huge
editor line that has to be shaped and wrapped again after every append.
UI updates append formatted preview text after about 100 ms or 128 Ki UTF-16
units. The worker may receive and format ahead while one batch is installing;
about 1 Mi UTF-16 units of buffered preview (plus one formatted chunk) applies
backpressure until that installation is acknowledged. Existing
carets, selection direction, scrolling, and undo history survive each append.

The editor preview stops growing after the first 512 Ki UTF-16 units of
formatted text. Further chunks remain in the document worker and update the
loading counter without GPUI appends or preview acknowledgment waits. This
bounds receipt-time editor layout and memory use; the complete response is
analyzed and installed through cancellable batched preparation at EOF.
Chromium's growing source text element is hidden to avoid laying out the raw
response behind the editor. The browser still owns this DOM; backpressure
bounds extension message queues, not Chromium's network or DOM buffering.

The same centered loading overlay used for file drops shows the response name,
decoded source bytes, and an Abort button. It covers the editor,
gutter, and scrollbar and blocks their mouse and keyboard input. Escape also
activates Abort. For extension responses, Abort terminates editor processing
and downloads the original source as `application/octet-stream`, using its
original filename. If receipt is unfinished, the remaining response is collected
before the browser download starts. The endpoint is never requested again,
including for POST and single-use responses. Both browsers reuse the original
response DOM until editor commit. Opening another file or cancelling after an
error does not start a download.
Abort immediately leaves a blank page at the original URL. The hidden viewer
stays alive only until the browser finishes using the download's Blob URL, then
is removed. If the download cannot start, the viewer shows the error.

During receipt the preview is indented but has no syntax diagnostics or semantic
highlighting. Its formatting is presentation only; the original source used for
saving is unchanged. At EOF, the worker formats and analyzes the complete source
once, installs normal highlighting/folding/diagnostics, and enables editing.
When the final formatted view exactly matches the preview, installation reuses
the editor buffer and its layout, preserving carets and scrolling. Otherwise it
replaces the preview and resets to the beginning, as with a normal file open. 
Syntax errors in an unfinished response are not presented as document errors.

The console reports response completion separately from final analysis and
records receipt time, synchronous preview append time, and analysis time. These
timings distinguish slow transfer, preview layout, and document processing.

## Permissions and storage

- HTTP/HTTPS host access and `webRequest` observe original document MIME headers.
- Chromium's `declarativeNetRequestWithHostAccess` applies document-only header rules.
- Firefox's `webRequestBlocking` changes headers on JSON/JSON5 documents only.
- Firefox's `clipboardWrite` allows copying from HTTP-embedded viewers when
  `navigator.clipboard` is unavailable. Clipboard read permission is not requested.
- `storage` retains small response metadata while the background context sleeps.
- `downloads` hands aborted response bodies to the browser's download manager.

Captured text stays in memory and is not uploaded or stored in IndexedDB.
The parent connects to the known extension-frame origin with a random
per-frame token and a private `MessageChannel` for chunks and cancellation. The editor
validates the token, HTTP(S) origin, parent window when exposed by the browser,
and reply port, then accepts one response stream. This also handles Firefox withholding
`MessageEvent.source` for messages involving privileged extension contexts.
The original text DOM is removed after editor commit, cancellation, or download preparation,
without removing or reloading the frame. Closing or navigating the tab releases
the captured body. Only the viewer's entry HTML is web-accessible; WASM, workers,
fonts, and extension scripts remain private.

## Reusing the parent standalone editor project

The extension injects a packaged bootstrap before the parent's module starts.
It provides `globalThis.json5EditorHost.initialDocument`, a promise for a local
`File` or a startup stream with `name`, `read()`, and `cancel()` methods. A user
Abort calls `cancel({ download: true })`; internal cancellation leaves download
disabled. A stream
may additionally expose `takePort()` to transfer its response port directly to
the worker and `close()` to release its separate cancellation port at completion.
The parent project routes it through the abortable worker loader after the GPUI window
is created, instead of opening the startup example.
The startup event is queued until the application handle is stored, so a fast
parse cannot attempt installation before the editor exists. A regular deployment
of the parent project has no host and keeps its original startup behavior.

The parent project's Vite transform resolves the dependency's fixed `eval("self")`
lookup as `globalThis`; any other expression is rejected with an error.
The parser's worker is
a packaged module, not a generated script. The initial theme script is external.
The extension keeps Manifest V3's JavaScript restrictions and enables only
`wasm-unsafe-eval` for WebAssembly. Packaging rejects inline scripts and generated
asset code containing JavaScript evaluation or `new Function`.

The package copies the parent project's SBOM and font/license assets along with the app.
The application dependency list remains owned by the parent project.

### Browser API references

- [Response-header matching (Chrome 128+)](https://developer.chrome.com/docs/extensions/reference/api/declarativeNetRequest#type-RuleCondition)
- [Manifest V3 content security policy](https://developer.chrome.com/docs/extensions/reference/manifest/content-security-policy)
- [Extension cross-origin isolation](https://developer.chrome.com/docs/extensions/reference/manifest/cross-origin-embedder-policy)
- [Chromium extension frames remain secure inside HTTP pages](https://chromium.googlesource.com/chromium/src/+/139595741ea5a325cfe456f27dc6d140b2c188e5%5E%21/)
- [Firefox background scripts and module support](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background)
- [Firefox blocking response-header changes](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onHeadersReceived)
- [Firefox data-collection declaration](https://extensionworkshop.com/documentation/develop/firefox-builtin-data-consent/)

## Validation

The package build checks that every manifest asset exists and checks the assets'
CSP compatibility. Browser behavior still needs checking after loading the
unpacked extension; no browser integration tests are run by the build.

# JSON5 Editor browser extension

Builds separate Manifest V3 extensions for Chromium and Firefox. Both package
the shared `../editor/` module and run the same GPUI editor, parser worker, fonts,
folding, diagnostics, search, and save UI.

- `dist/`: Chrome, Edge and compatible Chromium 128+ browsers, using a service
  worker and declarative response-header rules.
- `dist-firefox/`: Firefox Desktop 140+, using module background scripts and
  blocking response-header rules.

## Build and load

Build scripts are available:
```shell
scripts/build.bat
```

or
```shell
scripts/build.sh
```

This rebuilds the parent project (including optimized WASM) once, then packages both
distributions. 

Both browser manifests get their version automatically from `[package].version`
in `../Cargo.toml`. Update that version before signing a new release; there is
no separate version to maintain in `manifest.template.json`. Use a numeric
`major.minor.patch` release version supported by both browsers.

To build the extension entry point using the existing WASM modules:

```shell
npm run build
```

For Chromium, open `chrome://extensions` or `edge://extensions`, enable
**Developer mode**, choose **Load unpacked**, and select `../extensions/dist`.

For Firefox, open `about:debugging#/runtime/this-firefox`, choose **Load Temporary
Add-on**, and select **../extensions/dist-firefox/manifest.json**. Allow access to
the endpoint's host if Firefox asks. Temporary add-ons are removed on browser
restart; permanent installation requires a signed package.<br>

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
JSON is UTF-8; legacy non-UTF-8 response encodings are not supported by this
adapter. HTTP errors with JSON bodies are displayed and invalid JSON/JSON5 is
opened and shows invalid syntax errors rather than being rejected by the extension.

The editor fills the response page in an extension-origin iframe. The address
bar and history keep the original endpoint URL.
Reload and Back retain their normal browser behavior; reloading requests the
original endpoint again. The extension frame keeps the editor's scripts and
WebGPU context separate from the endpoint, including HTTP endpoints in desktop
Chromium. No navigation to an extension page is made.

The original response filename comes from Content-Disposition, then the URL.
Only decoded text is captured, not wire bytes, original encodings, or transport
compression. The extension does not intercept fetch/XHR streaming API traffic. Responses
served entirely by another service worker may bypass declarative header rules;
rendered JSON documents still have a MIME-based content-script fallback.

If the user cancels the load, then the file is downloaded as if its content-type
was `binary/octet-stream`.

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

### Browser API references

- [Response-header matching (Chrome 128+)](https://developer.chrome.com/docs/extensions/reference/api/declarativeNetRequest#type-RuleCondition)
- [Manifest V3 content security policy](https://developer.chrome.com/docs/extensions/reference/manifest/content-security-policy)
- [Extension cross-origin isolation](https://developer.chrome.com/docs/extensions/reference/manifest/cross-origin-embedder-policy)
- [Chromium extension frames remain secure inside HTTP pages](https://chromium.googlesource.com/chromium/src/+/139595741ea5a325cfe456f27dc6d140b2c188e5%5E%21/)
- [Firefox background scripts and module support](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/manifest.json/background)
- [Firefox blocking response-header changes](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onHeadersReceived)
- [Firefox data-collection declaration](https://extensionworkshop.com/documentation/develop/firefox-builtin-data-consent/)

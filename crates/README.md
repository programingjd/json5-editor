# Editor-owned GPUI Kit subset

These private, unpublished crates belong to the JSON5 Editor. Build them through
the workspace in `../Cargo.toml`; no GPUI Kit checkout or Git dependency is required.
They are a subset for the web editor, not a general replacement for GPUI Kit.

The local crates use version `0.9.1` for the editor-specific subset.
The source originated from GPUI Kit 0.7.1,
with selected fixes through 0.9.1 backported for search anchoring, diagnostic hover,
caret visibility and blinking, tooltips, button states, scrollbar hit testing,
focus traps and element bounds.
Measured wrapping reuse is integrated into the editor's cancellable layout jobs.
The GPUI engine remains at 0.3.8; these backports do not constitute a framework upgrade.

- `kit`: application imports, pinned GPUI/platform re-exports, initialization and window creation.
- `base`: text editing, source/display positions, folding, selection, undo, search,
  scrollbars, focus, popup geometry, motion and text rendering used by diagnostics.
- `component`: editor/input presentation, toolbar/status bar, buttons, checkboxes,
  menus, tooltips, themes and their supporting controls.
- `assets`: icons referenced by retained controls, asset loading and JetBrains license information.

The application still owns its toolbar, search panel, pseudo dialogs, status
content and document worker. JSON5 analysis uses `worker/` and `grammar/`;
the main editor does not bundle the framework's native syntax parsers.
Unused component families, gallery/examples, upstream tests and their fixtures,
the component macros crate and unused icons are excluded.
Context menus use the browser's GPUI-rendered popup overlay in
`component/src/context_menu.rs`; Windows/macOS native-menu backends are excluded.


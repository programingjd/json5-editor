# Icons

The editor uses [JetBrains Icons](https://intellij-icons.jetbrains.design/),
from the [IntelliJ Community icon sources](https://github.com/JetBrains/intellij-community/tree/master/platform/icons).
The JetBrains icon set is distributed under [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0).
[icons.json](../icons.json) records each bundled icon's source URL, provider and
SHA-256 hash. Paths are relative to the manifest in `assets/`.

The local component crate uses this same source directory. Its retained icons
are listed in `crates/assets/default-icons.txt` relative to the project root;
the web build copies only that list for runtime icon loading.

Keep the SVG copyright headers, [NOTICE](../../NOTICE) and [LICENSE-APACHE](LICENSE-APACHE) when
redistributing these icons. Apache-2.0 section 4 requires a license copy and
retention of applicable notices; a link alone is not a replacement.

`zed-logo.svg` is the Zed Industries logo supplied for the About dialog.
Its source and usage guidelines are on [Zed's brand page](https://zed.dev/brand).
It is a Zed trademark and is not covered by the JetBrains icon license.

import { writeFileSync } from 'node:fs';

// Curated About credits; the SBOM carries the complete dependency graph.
// License links use each repository's default branch.
function github(name, repository, branch, file, license, website = '') {
  return { name, website, license, repository: `https://github.com/${repository}`,
    license_url: `https://github.com/${repository}/blob/${branch}/${file}` };
}
const notices = {
  color_schemes: [{
    ...github('JetBrains Darcula theme', 'JetBrains/intellij-community', 'master', 'LICENSE.txt', 'Apache-2.0'),
    repository: 'https://github.com/JetBrains/intellij-community/blob/master/platform/platform-resources/src/DefaultColorSchemesManager.xml',
  }],
  icons: [{
    ...github('JetBrains Icons', 'JetBrains/intellij-community', 'master', 'LICENSE.txt', 'Apache-2.0', 'https://intellij-icons.jetbrains.design/'),
    repository: 'https://github.com/JetBrains/intellij-community/tree/master/platform/icons',
  }],
  fonts: [
    github('Inter', 'rsms/inter', 'master', 'LICENSE.txt', 'OFL-1.1', 'https://rsms.me/inter/'),
    github('CommitMono', 'eigilnikolajsen/commit-mono', 'main', 'LICENSE-FONT', 'OFL-1.1', 'https://commitmono.com/'),
  ],
  libraries: [
    { ...github('GPUI', 'zed-industries/zed', 'main', 'LICENSE-APACHE', 'Apache-2.0', 'https://gpui.rs/'), attribution: 'by Zed Industries' },
    github('GPUI Kit', 'longbridge/gpui-kit', 'main', 'LICENSE-APACHE', 'Apache-2.0', 'https://gpui-kit.com/'),
    github('TreeSitter', 'tree-sitter/tree-sitter', 'master', 'LICENSE', 'MIT', 'https://tree-sitter.github.io/tree-sitter/'),
    { name: 'Tree-Sitter-JSON5-Orchard', website: '', license: 'MIT',
      repository: 'https://codeberg.org/grammar-orchard/tree-sitter-json5-orchard',
      license_url: 'https://codeberg.org/grammar-orchard/tree-sitter-json5-orchard/src/branch/main/LICENSE' },
    github('wasm-bindgen', 'wasm-bindgen/wasm-bindgen', 'main', 'LICENSE-MIT', 'MIT', 'https://wasm-bindgen.github.io/wasm-bindgen/'),
  ],
};
writeFileSync(new URL('../assets/dependency-notices.json', import.meta.url), JSON.stringify(notices, null, 2) + '\n');
console.log('Generated curated About credits for Theme, Fonts, and Libraries.');

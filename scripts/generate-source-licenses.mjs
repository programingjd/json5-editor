import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { licenseArchive } from './license-archive.mjs';
import { resolveLicenseFallbacks } from './resolve-license-fallbacks.mjs';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const priority = ['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD', 'Unicode-3.0', 'CC0-1.0', 'bzip2-1.0.6'];

// SPDX AND binds more tightly than OR. Choose only between alternatives;
// additional obligations in an AND expression remain part of the selection.
export function selectLicenses(expression) {
  const tokens = expression.replaceAll('/', ' OR ').match(/[A-Za-z0-9.+-]+|[()]/g) ?? [];
  let index = 0;
  const rank = set => Math.min(...[...set].map(id => {
    const at = priority.indexOf(id);
    return at < 0 ? 100 : at;
  }));
  function atom() {
    if (tokens[index] === '(') {
      index++;
      const value = alternatives();
      if (tokens[index++] !== ')') throw new Error(`Invalid SPDX expression: ${expression}`);
      return value;
    }
    const id = tokens[index++];
    if (!id || ['AND', 'OR', 'WITH', ')'].includes(id)) throw new Error(`Unsupported SPDX expression: ${expression}`);
    return new Set([id]);
  }
  function conjunction() {
    let value = atom();
    while (tokens[index] === 'AND') {
      index++;
      value = new Set([...value, ...atom()]);
    }
    return value;
  }
  function alternatives() {
    let value = conjunction();
    while (tokens[index] === 'OR') {
      index++;
      const alternative = conjunction();
      if (rank(alternative) < rank(value)) value = alternative;
    }
    return value;
  }
  const selected = alternatives();
  if (index !== tokens.length) throw new Error(`Unsupported SPDX expression: ${expression}`);
  return [...selected].sort((a, b) => priority.indexOf(a) - priority.indexOf(b));
}

function identifies(content, id) {
  const normalized = content.toLowerCase().replace(/\s+/g, ' ');
  switch (id) {
    case 'MIT': return normalized.includes('permission is hereby granted') && normalized.includes('sublicense');
    case 'Apache-2.0': return normalized.includes('apache license') && normalized.includes('grant of patent license');
    case 'BSD-2-Clause': return normalized.includes('redistribution and use in source and binary forms')
      && !normalized.includes('neither the name') && !normalized.includes('3. neither');
    case 'BSD-3-Clause': return normalized.includes('redistribution and use in source and binary forms')
      && (normalized.includes('neither the name') || normalized.includes('3. neither'));
    case 'Unicode-3.0': return normalized.includes('unicode license v3');
    case 'Zlib': return normalized.includes('origin of this software must not be misrepresented');
    case 'CC0-1.0': return normalized.includes('creative commons') && (normalized.includes('cc0') || normalized.includes('universal'));
    case 'bzip2-1.0.6': return normalized.includes('bzip2') && normalized.includes('redistribution and use in source and binary forms');
    case 'ISC': return normalized.includes('permission to use, copy, modify, and/or distribute') && normalized.includes('with or without fee');
    case '0BSD': return normalized.includes('permission to use, copy, modify, and/or distribute') && !normalized.includes('copyright notice and this permission notice appear');
    default: return false;
  }
}

function safePath(file) {
  const normalized = file.replaceAll('\\', '/');
  if (!normalized || normalized.startsWith('/') || normalized.includes(':')
      || normalized.split('/').some(part => !part || part === '.' || part === '..')) {
    throw new Error(`Unsafe license document path: ${file}`);
  }
  return normalized;
}

export async function generateSourceLicenses() {
  const packages = JSON.parse(await readFile(path.join(root, 'target/build-tools/license-input.json'), 'utf8'));
  const overrides = await resolveLicenseFallbacks(packages, { selectLicenses, identifies });
  const frontend = JSON.parse(await readFile(path.join(root, 'target/build-tools/sbom-input.json'), 'utf8'));
  const sbom = JSON.parse(await readFile(path.join(root, 'assets/sbom.json'), 'utf8'));
  if (frontend.components.length) throw new Error('npm runtime dependencies need license evidence before distribution');
  const crateKeys = new Set(sbom.components.filter(component => component.purl?.startsWith('pkg:cargo/')
      && !['json5-web-editor', 'gpui-json5-processor'].includes(component.name))
    .map(component => `${component.name}@${component.version}`));
  const files = new Map();
  const entries = [];
  const missing = [];
  for (const pkg of packages.sort((a, b) => {
    const left = `${a.name}@${a.version}`;
    const right = `${b.name}@${b.version}`;
    return left < right ? -1 : left > right ? 1 : 0;
  })) {
    const key = `${pkg.name}@${pkg.version}`;
    if (!crateKeys.delete(key)) throw new Error(`License evidence is absent from the SBOM or duplicated: ${key}`);
    if (!pkg.license) throw new Error(`License declaration is missing for ${key}`);
    const selected = selectLicenses(pkg.license);
    const override = overrides[key];
    const directory = safePath(`${pkg.name}-${pkg.version}`);
    const documents = [];
    for (const [origin, evidence] of [['package', pkg.documents], ['fallback', override?.documents ?? []]]) {
      for (const document of evidence) {
        let relative = safePath(document.file);
        let destination = `${directory}/${relative}`;
        if (files.has(destination)) {
          if (files.get(destination) === document.text) continue;
          relative = `_fallback/${relative}`;
          destination = `${directory}/${relative}`;
          if (files.has(destination) && files.get(destination) !== document.text) {
            throw new Error(`Conflicting license evidence: ${destination}`);
          }
        }
        files.set(destination, document.text);
        documents.push({ file: relative, origin, url: document.url,
          sha256: createHash('sha256').update(document.text).digest('hex') });
      }
    }
    for (const license of selected) {
      if (![...pkg.documents, ...(override?.documents ?? [])].some(document => identifies(document.text, license))) {
        missing.push({ package: key, license, repository: pkg.repository });
      }
    }
    // Preserve source-file attribution evidence without rewriting its years or wording.
    if (Object.keys(pkg.source_copyrights).length) {
      files.set(`${directory}/SOURCE-COPYRIGHTS.json`, JSON.stringify(pkg.source_copyrights, null, 2) + '\n');
    }
    entries.push({ name: pkg.name, version: pkg.version, directory,
      source: pkg.repository || override?.repository || `https://crates.io/crates/${pkg.name}/${pkg.version}`,
      cargo_source: pkg.cargo_source ?? 'local workspace',
      published_source: pkg.cargo_source?.startsWith('registry+')
        ? `https://docs.rs/crate/${pkg.name}/${pkg.version}/source/` : undefined,
      revision: pkg.vcs?.git?.sha1 || override?.revision,
      declared_license: pkg.license, selected_licenses: selected,
      evidence_note: override?.evidence_note, documents });
  }
  if (crateKeys.size) throw new Error(`SBOM crates missing license evidence: ${[...crateKeys].join(', ')}`);
  await writeFile(path.join(root, 'target/build-tools/license-missing.json'), JSON.stringify(missing, null, 2) + '\n');
  if (missing.length) throw new Error(`Missing exact license text for ${missing.length} selections; see target/build-tools/license-missing.json`);
  files.set('index.json', JSON.stringify(entries, null, 2) + '\n');
  files.set('README.md', `# Dependency licenses

Original legal documents for the locked Rust dependencies in the bundled SBOM.
Directories use crate_name-version. index.json records sources, revisions,
SHA-256 hashes, license selections and any fallback evidence. Package documents
are preserved verbatim; fallback documents never overwrite different originals.
SOURCE-COPYRIGHTS.json retains attribution lines found in source headers.

The inventory includes build dependencies. First-party editor/worker code,
fonts, icons and npm development dependencies are covered separately or excluded.
OR alternatives prefer MIT, then Apache-2.0; AND obligations remain cumulative.
Other upstream license alternatives are retained for reference.
`);
  const assets = path.join(root, 'assets');
  const destination = path.resolve(assets, 'dependency-notices');
  if (path.dirname(destination) !== path.resolve(assets)) throw new Error('Invalid notices destination');
  await rm(destination, { recursive: true, force: true });
  for (const [filename, content] of files) {
    const target = path.join(destination, ...filename.split('/'));
    await mkdir(path.dirname(target), { recursive: true });
    await writeFile(target, content);
  }
  await writeFile(path.join(assets, 'DEPENDENCY-LICENSES.zip'), licenseArchive(files));

  const noticePath = path.join(root, 'NOTICE');
  const original = await readFile(noticePath, 'utf8');
  const marker = 'SOURCE CODE DEPENDENCIES:';
  const start = original.indexOf(marker);
  if (start < 0) throw new Error('NOTICE is missing SOURCE CODE DEPENDENCIES:');
  const kit = entries.find(entry => entry.name === 'gpui-kit');
  if (!kit) throw new Error('GPUI Kit attribution is missing from the license archive');
  const notice = [original.slice(0, start).trimEnd().replace(
    /^Bundled copyright notice and license: (?:LICENSE-APACHE|(?:assets\/)?DEPENDENCY-LICENSES\.zip[^\r\n]*)$/m,
    `Bundled copyright notice and license: assets/DEPENDENCY-LICENSES.zip (${kit.directory}/LICENSE-APACHE)`),
    '', marker, '',
    'Original dependency licenses, copyright and attribution notices are bundled in',
    'assets/DEPENDENCY-LICENSES.zip. Its index.json identifies each crate, exact version,',
    'source, license selection and collected documents. See the bundled SBOM for the inventory.',
    'The archive includes the locked Rust graph, including build dependencies.', ''];
  // Keep upstream NOTICE content directly accessible in the distribution's NOTICE.
  for (const entry of entries) {
    for (const document of entry.documents.filter(document => /(?:^|\/)(?:NOTICE)(?:$|[._-])/i.test(document.file))) {
      notice.push(`${entry.name} ${entry.version} — upstream ${document.file}`, '',
        files.get(`${entry.directory}/${document.file}`), '');
    }
  }
  await writeFile(noticePath, notice.join('\n'));
  // Retire only the former generated outputs; font/icon licenses stay in place.
  const retired = ['LICENSE-APACHE', 'LICENSE-MIT', 'LICENSE-BSD-2-CLAUSE', 'LICENSE-BSD-3-CLAUSE',
    'LICENSE-BZIP2-1.0.6', 'LICENSE-CC0-1.0', 'LICENSE-UNICODE-3.0', 'LICENSE-ZLIB', 'source-code-licenses.json'];
  for (const file of await readdir(assets)) {
    if (retired.includes(file)) {
      await rm(path.join(assets, file));
    }
  }
  console.log(`Archived ${entries.length} dependency notices (${files.size} files) in DEPENDENCY-LICENSES.zip`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await generateSourceLicenses();
}

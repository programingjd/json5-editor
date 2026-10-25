import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const cache = path.join(root, 'target/build-tools/license-cache');
const legalName = /^(?:licen[cs]e|copying|copyright|notice|unlicense)/i;
const requests = new Map();

// Cache immutable revision URLs, not crate names or moving repository branches.
async function download(url) {
  if (requests.has(url)) return requests.get(url);
  const pending = (async () => {
    const filename = path.join(cache, `${createHash('sha256').update(url).digest('hex')}.json`);
    try {
      const saved = JSON.parse(await readFile(filename, 'utf8'));
      if (saved.url === url && typeof saved.text === 'string') return saved.text;
    } catch (error) {
      if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error;
    }
    console.log(`Fetching license evidence: ${url}`);
    const response = await fetch(url, {
      headers: { 'User-Agent': 'JSON5-Editor-license-inventory' },
      signal: AbortSignal.timeout(30000),
    });
    if (response.status === 404) return null;
    if (!response.ok) throw new Error(`License lookup failed: HTTP ${response.status} ${url}`);
    const text = await response.text();
    if (/^\s*(?:<!doctype html|<html)/i.test(text)) throw new Error(`License lookup returned HTML: ${url}`);
    await mkdir(cache, { recursive: true });
    await writeFile(filename, JSON.stringify({ url, text }) + '\n');
    return text;
  })();
  requests.set(url, pending);
  return pending;
}

function ancestors(location = '') {
  const result = new Set(['']);
  while (location) {
    result.add(location);
    location = path.posix.dirname(location);
    if (location === '.') break;
  }
  return result;
}

async function upstreamDocuments(pkg, revision) {
  const github = pkg.repository?.match(/^https?:\/\/github\.com\/([^/]+)\/([^/#?]+)/i);
  if (!github) return [];
  const repository = `${github[1]}/${github[2].replace(/\.git$/, '')}`;
  const source = await download(`https://api.github.com/repos/${repository}/git/trees/${revision}?recursive=1`);
  if (!source) throw new Error(`Published revision cannot be found for ${pkg.name}@${pkg.version}: ${revision}`);
  const tree = JSON.parse(source);
  if (tree.truncated) throw new Error(`License lookup tree is truncated: ${repository}@${revision}`);
  const locations = ancestors(pkg.vcs?.path_in_vcs);
  const documents = [];
  for (const entry of tree.tree) {
    if (entry.type !== 'blob') continue;
    const folder = path.posix.dirname(entry.path);
    if (!(locations.has(folder === '.' ? '' : folder) && legalName.test(path.posix.basename(entry.path)))
        && !entry.path.split('/').includes('.licenses')) continue;
    const url = `https://raw.githubusercontent.com/${repository}/${revision}/${entry.path.split('/').map(encodeURIComponent).join('/')}`;
    const text = await download(url);
    if (text === null) throw new Error(`Listed upstream license file is missing: ${url}`);
    documents.push({ file: entry.path, url, text });
  }
  return documents;
}

export async function resolveLicenseFallbacks(packages, { selectLicenses, identifies }) {
  const output = {};
  for (const pkg of packages) {
    if (!pkg.license) throw new Error(`License declaration is missing for ${pkg.name}@${pkg.version}`);
    const selected = selectLicenses(pkg.license);
    if (selected.every(id => pkg.documents.some(document => identifies(document.text, id)))) continue;
    const key = `${pkg.name}@${pkg.version}`;
    if (!pkg.cargo_source) throw new Error(`Local crate ${key} must include its own legal files`);
    const revision = pkg.vcs?.git?.sha1;
    if (revision && !/^[a-f0-9]{40}$/i.test(revision)) throw new Error(`Invalid source revision for ${key}`);
    const documents = revision ? await upstreamDocuments(pkg, revision) : [];
    const standards = [];
    for (const id of selected) {
      if ([...pkg.documents, ...documents].some(document => identifies(document.text, id))) continue;
      // Some published crates declare a license without distributing its text.
      // Label standard SPDX text explicitly; never invent an upstream notice.
      const url = `https://raw.githubusercontent.com/spdx/license-list-data/v3.26.0/text/${encodeURIComponent(id)}.txt`;
      const standard = await download(url);
      if (standard === null) throw new Error(`No upstream or SPDX license text available for ${key}: ${id}`);
      const text = standard.split('\n').filter(line => !/^\s*Copyright.*(?:<year>|<copyright holders?>|\[year\]|\[fullname\])/i.test(line)).join('\n');
      if (!identifies(text, id)) throw new Error(`Unrecognized license text for ${key}: ${id}`);
      documents.push({ file: `SPDX-${id}.txt`, url, text });
      standards.push(id);
    }
    output[key] = { repository: pkg.repository, revision, documents,
      evidence_note: standards.length
        ? `Standard SPDX text used for ${standards.join(', ')} because exact package/revision sources did not supply matching text or a supported pinned repository lookup was unavailable. This is not an upstream copyright notice; source attribution evidence is retained separately.`
        : 'Legal documents retrieved from the published Cargo source revision.' };
  }
  await writeFile(path.join(root, 'target/build-tools/license-overrides.json'), JSON.stringify(output, null, 2) + '\n');
  console.log(`Generated license fallbacks for ${Object.keys(output).length} current crate versions`);
  return output;
}

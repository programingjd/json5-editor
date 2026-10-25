import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { responseRules } from '../src/media-types.js';
import { buildTargets } from './manifests.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const standalone = path.resolve(root, '../www');
if (process.argv.slice(2).some(argument => argument !== '--rebuild')) throw new Error('Usage: node scripts/build.mjs [--rebuild]');
if (process.argv.includes('--rebuild')) {
  const npmCli = process.env.npm_execpath;
  const command = npmCli ? process.execPath : process.platform === 'win32' ? process.env.ComSpec ?? 'cmd.exe' : 'npm';
  const args = npmCli ? [npmCli, 'run', 'build']
    : process.platform === 'win32' ? ['/d', '/s', '/c', 'npm run build'] : ['run', 'build'];
  const result = spawnSync(command, args, { cwd: standalone, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
const source = path.join(standalone, 'dist');
let html = await readFile(path.join(source, 'index.html'), 'utf8').catch(() => {
  throw new Error('Build standalone first, or run npm run build:all in standalone/extensions');
});
if (/<script\b(?![^>]*\bsrc=)[^>]*>/i.test(html)) throw new Error('The standalone build contains an inline script blocked by Manifest V3');
if (!html.includes('</head>')) throw new Error('The standalone HTML has no bootstrap insertion point');
html = html.replace('</head>', '  <script src="/src/viewer.js"></script>\n</head>');
for (const name of await readdir(path.join(source, 'assets'))) {
  if (!name.endsWith('.js')) continue;
  const code = await readFile(path.join(source, 'assets', name), 'utf8');
  if (/\beval\s*\(|\bnew\s+Function\s*\(/.test(code)) throw new Error(`Manifest V3 blocks dynamic code in assets/${name}`);
}
const template = JSON.parse(await readFile(path.join(root, 'manifest.template.json'), 'utf8'));
for (const { directory, label, manifest } of buildTargets(template)) {
  const target = path.resolve(root, directory);
  if (path.dirname(target) !== root || !['dist', 'dist-firefox'].includes(path.basename(target))) {
    throw new Error('Unsafe extension output path');
  }
  await rm(target, { recursive: true, force: true });
  await mkdir(target, { recursive: true });
  await cp(source, target, { recursive: true });
  await cp(path.join(root, 'src'), path.join(target, 'src'), { recursive: true });
  if (manifest.declarative_net_request) {
    await mkdir(path.join(target, 'rules'));
    await writeFile(path.join(target, 'rules/json-responses.json'), `${JSON.stringify(responseRules(), null, 2)}\n`);
  }
  await writeFile(path.join(target, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  await writeFile(path.join(target, 'index.html'), html);
  const manifestFiles = [
    ...(manifest.background.service_worker ? [manifest.background.service_worker] : manifest.background.scripts),
    ...manifest.content_scripts.flatMap(script => [...script.js, ...(script.css ?? [])]),
    ...(manifest.declarative_net_request?.rule_resources ?? []).map(rule => rule.path),
    ...manifest.web_accessible_resources.flatMap(resource => resource.resources),
  ];
  for (const file of manifestFiles) {
    if (file.includes('\\') || file.startsWith('/') || file.split('/').includes('..')) {
      throw new Error(`Invalid manifest asset path: ${file}`);
    }
    const asset = await stat(path.join(target, file)).catch(() => null);
    if (!asset?.isFile()) throw new Error(`Missing manifest asset in ${directory}: ${file}`);
  }
  console.log(`Packaged JSON5 Editor ${label} extension: ${target}`);
}

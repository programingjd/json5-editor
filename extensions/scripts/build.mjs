import { cp, mkdir, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { responseRules } from '../src/media-types.js';
import { buildTargets } from './manifests.mjs';
import { renderExtensionIcons } from './icons.mjs';

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
// Build the extension entry point against the shared editor module.
// No standalone HTML or generated JavaScript is modified during packaging.
const require = createRequire(path.join(standalone, 'package.json'));
const { parse: parseToml } = require('smol-toml');
const cargo = parseToml(await readFile(path.resolve(root, '../Cargo.toml'), 'utf8'));
const version = cargo.package?.version;
if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)
    || version.split('.').some(part => Number(part) > 65535)
    || version.split('.').every(part => Number(part) === 0)) {
  throw new Error('Cargo.toml package.version must be a browser-compatible release version (major.minor.patch, each 0–65535, not all zero).');
}
const vitePackage = path.dirname(require.resolve('vite/package.json'));
const result = spawnSync(process.execPath, [path.join(vitePackage, 'bin/vite.js'), 'build',
  '--config', path.join(root, 'vite.config.js')], { cwd: root, stdio: 'inherit' });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const source = path.resolve(root, '../target/extensions-web');
const html = await readFile(path.join(source, 'index.html'), 'utf8');
if (/<script\b(?![^>]*\bsrc=)[^>]*>/i.test(html)) throw new Error('The extension build contains an inline script blocked by Manifest V3');
for (const name of await readdir(path.join(source, 'assets'))) {
  if (!name.endsWith('.js')) continue;
  const code = await readFile(path.join(source, 'assets', name), 'utf8');
  if (/\beval\s*\(|\bnew\s+Function\s*\(/.test(code)) throw new Error(`Manifest V3 blocks dynamic code in assets/${name}`);
}
const template = JSON.parse(await readFile(path.join(root, 'manifest.template.json'), 'utf8'));
template.version = version;
const icons = path.resolve(root, '../target/build-tools/extension-icons');
await renderExtensionIcons(icons);
for (const { directory, label, manifest } of buildTargets(template)) {
  const target = path.resolve(root, directory);
  if (path.dirname(target) !== root || !['dist', 'dist-firefox'].includes(path.basename(target))) {
    throw new Error('Unsafe extension output path');
  }
  await rm(target, { recursive: true, force: true });
  await mkdir(target, { recursive: true });
  await cp(source, target, { recursive: true });
  await cp(icons, path.join(target, 'icons'), { recursive: true });
  await mkdir(path.join(target, 'src'));
  for (const file of ['background.js', 'capture.js', 'media-types.js']) {
    await cp(path.join(root, 'src', file), path.join(target, 'src', file));
  }
  if (manifest.declarative_net_request) {
    await mkdir(path.join(target, 'rules'));
    await writeFile(path.join(target, 'rules/json-responses.json'), `${JSON.stringify(responseRules(), null, 2)}\n`);
  }
  await writeFile(path.join(target, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  const manifestFiles = [
    ...Object.values(manifest.icons ?? {}),
    ...Object.values(manifest.action?.default_icon ?? {}),
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

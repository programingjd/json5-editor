import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(new URL('../www/package.json', import.meta.url));
const { parse: parseToml } = require('smol-toml');

export async function prepareSbom() {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const lock = JSON.parse(await readFile(path.join(root, 'www/package-lock.json'), 'utf8'));
  const cargoLock = parseToml(await readFile(path.join(root, 'Cargo.lock'), 'utf8'));
  const entries = new Map();
  for (const [key, details] of Object.entries(lock.packages)) {
    if (!key || details.link) continue;
    const name = details.name ?? key.split('node_modules/').at(-1);
    const { version, integrity } = details;
    let installed;
    try { installed = JSON.parse(await readFile(path.join(root, 'www', key, 'package.json'), 'utf8')); }
    catch { /* Platform-specific optional packages can be absent on this host. */ }
    const reference = `npm:${key}@${version}`;
    const component = { type: 'library', 'bom-ref': reference, name, version,
      purl: `pkg:npm/${name.split('/').map(encodeURIComponent).join('/')}@${encodeURIComponent(version)}` };
    const license = typeof installed?.license === 'string' ? installed.license : installed?.license?.type;
    if (license) component.licenses = [{ expression: license }];
    const hash = /^(sha256|sha384|sha512)-(.+)$/.exec(integrity ?? '');
    if (hash) component.hashes = [{ alg: hash[1].replace('sha', 'SHA-'), content: Buffer.from(hash[2], 'base64').toString('hex') }];
    entries.set(key, { component, details: details ?? {} });
  }
  function resolveDependency(parent, name) {
    let prefix = parent;
    while (prefix) {
      const candidate = entries.get(`${prefix}/node_modules/${name}`);
      if (candidate) return candidate;
      const nested = prefix.lastIndexOf('/node_modules/');
      prefix = nested >= 0 ? prefix.slice(0, nested) : '';
    }
    return entries.get(`node_modules/${name}`);
  }
  const dependencies = [...entries].map(([key, entry]) => ({ ref: entry.component['bom-ref'],
    dependsOn: [...new Set(Object.keys({ ...entry.details.dependencies, ...entry.details.optionalDependencies })
      .map(name => resolveDependency(key, name)?.component['bom-ref']).filter(Boolean))] }));
  const rootPackage = lock.packages[''];
  const runtime = new Set();
  function visit(reference) {
    if (!reference || runtime.has(reference)) return;
    runtime.add(reference);
    for (const dependency of dependencies.find(item => item.ref === reference)?.dependsOn ?? []) visit(dependency);
  }
  for (const name of Object.keys(rootPackage.dependencies ?? {})) visit(resolveDependency('', name)?.component['bom-ref']);
  const runtimeEntries = [...entries.values()].filter(({ component }) => runtime.has(component['bom-ref']));
  for (const { component } of runtimeEntries) {
    component.scope = 'required';
    component.properties = [{ name: 'gpui-json5:dependency-role', value: 'runtime' }];
  }
  const input = {
    components: runtimeEntries.map(entry => entry.component),
    dependencies: dependencies.filter(item => runtime.has(item.ref))
      .map(item => ({ ...item, dependsOn: item.dependsOn.filter(reference => runtime.has(reference)) })),
    roots: Object.keys(rootPackage.dependencies ?? {})
      .map(name => resolveDependency('', name)?.component['bom-ref']).filter(Boolean),
    cargo_checksums: Object.fromEntries(cargoLock.package.filter(pkg => pkg.checksum)
      .map(pkg => [`${pkg.name}@${pkg.version}`, pkg.checksum])),
  };
  await mkdir(path.join(root, 'target/build-tools'), { recursive: true });
  await writeFile(path.join(root, 'target/build-tools/sbom-input.json'), JSON.stringify(input, null, 2) + '\n');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await prepareSbom();

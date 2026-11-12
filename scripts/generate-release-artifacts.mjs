import { copyFile, lstat, readFile, readdir, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { licenseArchive } from './license-archive.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const archives = [
  ['extensions/dist', 'chrome-extension-unpacked.zip'],
  ['extensions/dist-firefox', 'firefox-extension-unpacked.zip'],
  ['www/dist', 'standalone.zip'],
];

async function collectFiles(directory, prefix = '', files = new Map()) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const filename = path.join(directory, entry.name);
    const name = prefix + entry.name;
    if (entry.isDirectory()) {
      await collectFiles(filename, `${name}/`, files);
    } else if (entry.isFile()) {
      files.set(name, await readFile(filename));
    } else {
      throw new Error(`Unsupported release input: ${filename}`);
    }
  }
  return files;
}

async function main() {
  for (const [source] of archives) {
    if (!(await lstat(path.join(root, source))).isDirectory()) {
      throw new Error(`Expected build directory: ${source}`);
    }
  }
  const sbom = path.join(root, 'assets', 'sbom.json');
  if (!(await lstat(sbom)).isFile()) throw new Error('Expected assets/sbom.json');

  for (const [source, output] of archives) {
    const files = await collectFiles(path.join(root, source));
    await writeFile(path.join(root, output), licenseArchive(files));
    console.log(`Created ${output} (${files.size} files, maximum compression)`);
  }
  await copyFile(sbom, path.join(root, 'sbom.json'));
  console.log('Copied sbom.json');
}

main().catch(error => {
  console.error(`Release artifacts failed: ${error.message}`);
  process.exitCode = 1;
});
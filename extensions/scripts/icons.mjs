import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(new URL('../../www/package.json', import.meta.url));
const { Resvg } = require('@resvg/resvg-js');

export async function renderExtensionIcons(output) {
  await mkdir(output, { recursive: true });
  const svg = await readFile(path.join(root, 'icon.svg'), 'utf8');
  for (const size of [16, 32, 48, 64, 128]) {
    const png = new Resvg(svg, {
      fitTo: { mode: 'width', value: size },
      font: { loadSystemFonts: false },
    }).render().asPng();
    if (png.readUInt32BE(16) !== size || png.readUInt32BE(20) !== size) {
      throw new Error(`Extension icon must be ${size}×${size}`);
    }
    await writeFile(path.join(output, `icon-${size}.png`), png);
  }
  console.log('Rendered extension icons from icon.svg at 16, 32, 48, 64 and 128 px');
}

import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';
import wasm from 'vite-plugin-wasm';
import { viteStaticCopy } from 'vite-plugin-static-copy';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { wasmCsp } from './wasm-csp.js';
import { browserGpuDefaults } from './browser-gpu-defaults.js';
import { browserClipboard } from './browser-clipboard.js';

const root = path.dirname(fileURLToPath(import.meta.url));
const componentIcons = readFileSync(path.resolve(root, '../crates/assets/default-icons.txt'), 'utf8')
  .trim().split(/\r?\n/).map(file => path.posix.basename(file));
const iconsPath = path.resolve(root, '../assets/icons').replaceAll('\\', '/');

export default defineConfig({
  plugins: [
    wasmCsp(),
    browserGpuDefaults(),
    browserClipboard(),
    wasm(),
    viteStaticCopy({
      targets: [
        { src: '../LICENSE-APACHE', dest: '.' },
        ...componentIcons.map(file => ({ src: `${iconsPath}/${file}`, dest: 'assets/icons' })),
        { src: '../assets/icons/LICENSE-APACHE', dest: 'assets/icons' },
        { src: '../assets/icons.json', dest: 'assets' },
        { src: '../NOTICE', dest: '.' },
        { src: '../assets/icons/README.md', dest: 'assets/icons' },
        { src: '../assets/fonts/OFL.txt', dest: 'assets/fonts' },
        { src: '../assets/fonts/README.md', dest: 'assets/fonts' },
        { src: '../assets/DEPENDENCY-LICENSES.zip', dest: 'assets' },
      ],
    }),
    {
      name: 'serve-kit-icons',
      configureServer(server) {
        server.middlewares.use('/assets/icons', async (request, response, next) => {
          const name = path.basename(request.url ?? '');
          const file = path.join(iconsPath, name);
          try {
            const { readFile } = await import('node:fs/promises');
            const contents = await readFile(file);
            response.setHeader('Content-Type', 'image/svg+xml');
            response.end(contents);
          } catch {
            next();
          }
        });
      },
    },
  ],
  base: '/',
  worker: { format: 'es' },
  build: {
    rolldownOptions: {
      output: {
        assetFileNames: asset => asset.names.some(name => /\.(woff2?|otf|ttf)$/i.test(name))
          ? 'assets/fonts/[name]-[hash][extname]' : 'assets/[name]-[hash][extname]',
      },
    },
    target: 'esnext',
    minify: true,
    sourcemap: false,
  },
  server: {
    port: 3010,
    open: true,
    headers: {
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cross-Origin-Opener-Policy': 'same-origin',
    },
  },
  optimizeDeps: { exclude: ['./src/wasm', './src/worker-wasm'] },
});

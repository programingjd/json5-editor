import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { wasmCsp } from './build/wasm-csp.js';
import { browserGpuDefaults } from './build/browser-gpu-defaults.js';
import { browserClipboard } from './build/browser-clipboard.js';

const editorRoot = fileURLToPath(new URL('.', import.meta.url));
const projectRoot = path.resolve(editorRoot, '..');
// Both entry points use the existing, locked npm build toolchain.
const require = createRequire(new URL('../www/package.json', import.meta.url));

export async function createEditorConfig(rootUrl, { outDir = 'dist' } = {}) {
  const wasm = (await import(pathToFileURL(require.resolve('vite-plugin-wasm')).href)).default;
  const { viteStaticCopy } = await import(pathToFileURL(require.resolve('vite-plugin-static-copy')).href);
  const appRoot = fileURLToPath(rootUrl);
  const initialTheme = path.join(editorRoot, 'src/initial-theme.js');
  const asset = file => path.resolve(projectRoot, file).replaceAll('\\', '/');
  const iconsPath = asset('assets/icons');
  const componentIcons = readFileSync(asset('crates/assets/default-icons.txt'), 'utf8')
    .trim().split(/\r?\n/).map(file => path.posix.basename(file));

  return {
    root: editorRoot,
    publicDir: false,
    resolve: { alias: { '/app-entry.js': path.join(appRoot, 'src/main.js') } },
    plugins: [
      {
        name: 'initial-editor-theme',
        configureServer(server) {
          server.middlewares.use('/initial-theme.js', (_, response) => {
            response.setHeader('Content-Type', 'text/javascript');
            response.end(readFileSync(initialTheme));
          });
        },
        generateBundle() {
          this.emitFile({ type: 'asset', fileName: 'initial-theme.js', source: readFileSync(initialTheme) });
        },
      },
      wasmCsp(),
      browserGpuDefaults(),
      browserClipboard(),
      wasm(),
      viteStaticCopy({
        targets: [
          { src: asset('LICENSE-APACHE'), dest: '.' },
          ...componentIcons.map(file => ({ src: `${iconsPath}/${file}`, dest: 'assets/icons' })),
          { src: asset('assets/icons/LICENSE-APACHE'), dest: 'assets/icons' },
          { src: asset('assets/icons.json'), dest: 'assets' },
          { src: asset('NOTICE'), dest: '.' },
          { src: asset('assets/icons/README.md'), dest: 'assets/icons' },
          { src: asset('assets/fonts/OFL.txt'), dest: 'assets/fonts' },
          { src: asset('assets/fonts/README.md'), dest: 'assets/fonts' },
          { src: asset('assets/DEPENDENCY-LICENSES.zip'), dest: 'assets' },
        ],
      }),
      {
        name: 'serve-kit-icons',
        configureServer(server) {
          server.middlewares.use('/assets/icons', async (request, response, next) => {
            const file = path.join(iconsPath, path.basename(request.url ?? ''));
            try {
              const { readFile } = await import('node:fs/promises');
              response.setHeader('Content-Type', 'image/svg+xml');
              response.end(await readFile(file));
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
      outDir: path.resolve(appRoot, outDir),
      emptyOutDir: true,
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
      fs: { allow: [projectRoot] },
      headers: {
        'Cross-Origin-Embedder-Policy': 'require-corp',
        'Cross-Origin-Opener-Policy': 'same-origin',
      },
    },
    optimizeDeps: {
      exclude: ['./src/wasm/gpui_json5_editor_web.js', './src/worker-wasm/gpui_json5_processor.js'],
    },
  };
}

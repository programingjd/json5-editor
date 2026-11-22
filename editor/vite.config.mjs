import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { wasmCsp } from './build/wasm-csp.js';
import { browserGpuDefaults } from './build/browser-gpu-defaults.js';
import { browserClipboard } from './build/browser-clipboard.js';
import { browserInputFocus } from './build/browser-input-focus.js';

const editorRoot = fileURLToPath(new URL('.', import.meta.url));
const projectRoot = path.resolve(editorRoot, '..');
// Both entry points use the existing, locked npm build toolchain.
const require = createRequire(import.meta.url);

export default function editorConfig() {
  return createEditorConfig(new URL('.', import.meta.url));
}

export async function createEditorConfig(rootUrl, {
  outDir = 'dist', wasmModule = 'gpui_json5_editor_web',
} = {}) {
  const wasm = (await import(pathToFileURL(require.resolve('vite-plugin-wasm')).href)).default;
  const { viteStaticCopy } = await import(pathToFileURL(require.resolve('vite-plugin-static-copy')).href);
  const appRoot = fileURLToPath(rootUrl);
  const initialTheme = path.join(editorRoot, 'src/initial-theme.js');
  const asset = file => path.resolve(projectRoot, file).replaceAll('\\', '/');
  const iconsPath = asset('assets/icons');
  const componentIcons = readFileSync(asset('crates/assets/default-icons.txt'), 'utf8')
    .trim().split(/\r?\n/).map(file => path.posix.basename(file));
  const favicon = path.join(appRoot, 'favicon.ico');
  const pageAssets = [
    ['initial-theme.js', initialTheme, 'text/javascript'],
    ['LICENSE-APACHE', asset('LICENSE-APACHE'), 'text/plain; charset=utf-8'],
    ['NOTICE', asset('NOTICE'), 'text/plain; charset=utf-8'],
    ...(existsSync(favicon) ? [['favicon.ico', favicon, 'image/x-icon']] : []),
  ];

  return {
    root: editorRoot,
    publicDir: false,
    resolve: { alias: {
      '/app-entry.js': path.join(appRoot, 'src/main.js'),
      '/gpui-browser-input-focus.js': path.join(editorRoot, 'src/browser-input-focus.js'),
      '/gpui-clipboard.js': path.join(editorRoot, 'src/clipboard.js'),
    } },
    plugins: [
      {
        name: 'editor-page-assets',
        configureServer(server) {
          for (const [fileName, source, contentType] of pageAssets) {
            server.middlewares.use(`/${fileName}`, (_, response) => {
              response.setHeader('Content-Type', contentType);
              response.end(readFileSync(source));
            });
          }
        },
        generateBundle() {
          for (const [fileName, source] of pageAssets) {
            this.emitFile({ type: 'asset', fileName, source: readFileSync(source) });
          }
        },
      },
      wasmCsp(wasmModule),
      browserGpuDefaults(wasmModule),
      browserClipboard(wasmModule),
      browserInputFocus(wasmModule),
      wasm(),
      viteStaticCopy({
        targets: [
          ...componentIcons.map(file => ({ src: `${iconsPath}/${file}`, dest: 'assets/icons' })),
          { src: asset('assets/icons/LICENSE-APACHE'), dest: 'assets/icons' },
          { src: asset('assets/icons.json'), dest: 'assets' },
          { src: asset('assets/icons/README.md'), dest: 'assets/icons' },
          { src: asset('assets/fonts/OFL.txt'), dest: 'assets/fonts' },
          { src: asset('assets/fonts/README.md'), dest: 'assets/fonts' },
          { src: asset('assets/DEPENDENCY-LICENSES.zip'), dest: 'assets' },
        ].map(target => ({
          ...target,
          // Relative glob paths retain forward slashes for v4's directory stripping on Windows.
          src: path.relative(editorRoot, target.src).replaceAll('\\', '/'),
          rename: { stripBase: true },
        })),
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
      exclude: [`./src/wasm/${wasmModule}.js`, './src/worker-wasm/gpui_json5_processor.js'],
    },
  };
}

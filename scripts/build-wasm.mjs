import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';
import { prepareSbom } from './prepare-sbom.mjs';
import { generateSourceLicenses } from './generate-source-licenses.mjs';

const args = process.argv.slice(2);
if (args.length > 1 || args.some(arg => !['--debug', '--release'].includes(arg))) {
  console.error('Usage: npm run build:wasm -- [--release|--debug]');
  process.exit(1);
}
const profile = args[0] === '--debug' ? 'dev' : 'release';
const root = resolve(fileURLToPath(new URL('../', import.meta.url)));
const containerfile = fileURLToPath(new URL('./Containerfile', import.meta.url));
const image = 'localhost/gpui-json5-editor-builder:nightly';
const volumes = ['gpui-json5-editor-cargo-target', 'gpui-json5-editor-cargo-registry', 'gpui-json5-editor-rustup'];

function available(command, args) {
  const result = spawnSync(command, args, { stdio: 'ignore' });
  return !result.error && result.status === 0;
}

function run(command, args) {
  const result = spawnSync(command, args, { stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with exit code ${result.status ?? 'unknown'}`);
}

async function build() {
  const requested = process.env.CONTAINER_ENGINE;
  if (requested && !['podman', 'docker'].includes(requested)) {
    throw new Error('CONTAINER_ENGINE must be podman or docker');
  }
  const engine = (requested ? [requested] : ['podman', 'docker'])
    .find(candidate => available(candidate, ['info']));
  if (!engine) {
    throw new Error('No running container engine. Install Podman or Docker and start it. '
      + 'On Windows/macOS, Podman needs "podman machine init" (once) and "podman machine start". '
      + 'Docker Desktop must use Linux containers. Set CONTAINER_ENGINE to select an engine.');
  }
  console.log(`Building WASM with ${engine} (${profile})`);
  await prepareSbom();
  run(engine, ['build', engine === 'podman' ? '--pull=always' : '--pull',
    '--tag', image, '--file', containerfile, root]);
  for (const volume of volumes) {
    if (!available(engine, ['volume', 'inspect', volume])) run(engine, ['volume', 'create', volume]);
  }
  run(engine, ['run', '--rm',
    '--volume', `${root}:/workspace/standalone`,
    '--volume', `${volumes[0]}:/target`,
    '--volume', `${volumes[1]}:/usr/local/cargo/registry`,
    '--volume', `${volumes[2]}:/usr/local/rustup`,
    '--workdir', '/workspace/standalone', '--env', 'CARGO_TARGET_DIR=/target',
    image, 'bash', '/workspace/standalone/scripts/build-in-container.sh', profile]);
  await generateSourceLicenses();
}

build().catch(error => {
  console.error(`Containerized WASM build failed: ${error.message}`);
  process.exitCode = 1;
});

import { cp, mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const args = process.argv.slice(2);
if (args.length === 1 && ['--help', '-h'].includes(args[0])) {
  console.log('Usage: npm run sign:firefox\nRebuilds the extension, prompts for AMO JWT credentials, and signs for unlisted distribution.\nSigned XPI: standalone/target/firefox-signed/');
  process.exit(0);
}
if (args.length) {
  console.error('Usage: npm run sign:firefox (no arguments; credentials are prompted)');
  process.exit(1);
}

let issuer;
let secret;
try {
  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new Error('Run this script in an interactive terminal to enter signing credentials.');
  }
  const require = createRequire(new URL('../../www/package.json', import.meta.url));
  const { input, password } = await import(pathToFileURL(require.resolve('@inquirer/prompts')).href);
  const build = spawnSync(process.execPath, [path.join(root, 'scripts/build.mjs')], {
    cwd: root, stdio: 'inherit',
  });
  if (build.error) throw build.error;
  if (build.status !== 0) process.exit(build.status ?? 1);

  const manifest = JSON.parse(await readFile(path.join(root, 'dist-firefox/manifest.json'), 'utf8'));
  if (!manifest.browser_specific_settings?.gecko?.id) throw new Error('The Firefox manifest needs a gecko.id before signing.');
  console.log(`Signing ${manifest.name} ${manifest.version} for unlisted distribution on addons.mozilla.org.`);
  console.log('Create JWT credentials at https://addons.mozilla.org/developers/addon/api/key/');
  issuer = (await input({ message: 'JWT issuer:', validate: value => !!value.trim() || 'Enter the JWT issuer.' })).trim();
  secret = (await password({ message: 'JWT secret:', mask: '*', validate: value => !!value.trim() || 'Enter the JWT secret.' })).trim();

  const work = path.resolve(root, '../target/build-tools/firefox-signing');
  const artifactsDir = path.resolve(root, '../target/firefox-signed');
  await mkdir(work, { recursive: true });
  await mkdir(artifactsDir, { recursive: true });
  // web-ext writes upload state in its source directory; keep it out of dist.
  const sourceDir = await mkdtemp(path.join(work, 'source-'));
  await cp(path.join(root, 'dist-firefox'), sourceDir, { recursive: true });
  const temporary = path.join(work, 'temp');
  await mkdir(temporary, { recursive: true });
  for (const key of ['TMPDIR', 'TMP', 'TEMP']) process.env[key] = temporary;
  const { default: webExt } = await import(pathToFileURL(require.resolve('web-ext')).href);
  const pkg = JSON.parse(await readFile(path.join(path.dirname(require.resolve('web-ext')), 'package.json'), 'utf8'));
  const result = await webExt.cmd.sign({
    sourceDir, artifactsDir, channel: 'unlisted',
    apiKey: issuer, apiSecret: secret, webextVersion: pkg.version,
  });
  if (!result?.downloadedFiles?.length) {
    throw new Error('Mozilla has not returned a signed XPI. Check the submission status in the AMO developer dashboard.');
  }
  for (const file of result.downloadedFiles) console.log(`Signed Firefox extension: ${path.resolve(artifactsDir, file)}`);
} catch (error) {
  if (error.name === 'ExitPromptError' || error.name === 'AbortPromptError') {
    console.error('Firefox signing cancelled.');
    process.exitCode = 130;
  } else {
    let message = error.message ?? String(error);
    for (const credential of [issuer, secret]) {
      if (credential) message = message.replaceAll(credential, '[redacted]');
    }
    console.error(`Firefox signing failed: ${message}`);
    process.exitCode = 1;
  }
} finally {
  issuer = undefined;
  secret = undefined;
}

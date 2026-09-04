import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const manifest = join(root, 'src-tauri', 'Cargo.toml');
const binaries = join(root, 'src-tauri', 'binaries');
const debug = process.argv.includes('--debug');
const requested = process.argv.slice(2).find(arg => !arg.startsWith('--'));

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function output(command, args) {
  const result = spawnSync(command, args, { cwd: root, encoding: 'utf8' });
  if (result.error || result.status !== 0) {
    throw result.error || new Error(`${command} exited with ${result.status}`);
  }
  return result.stdout;
}

function hostTriple() {
  const line = output('rustc', ['-vV']).split(/\r?\n/).find(value => value.startsWith('host: '));
  if (!line) throw new Error('rustc did not report a host target triple');
  return line.slice('host: '.length).trim();
}

function sourceName(target) {
  return target.includes('windows') ? 'julong-codex.exe' : 'julong-codex';
}

function sidecarName(target) {
  return `julong-codex-${target}${target.includes('windows') ? '.exe' : ''}`;
}

function build(target) {
  const args = ['build', '--manifest-path', manifest, '--bin', 'julong-codex', '--target', target];
  if (!debug) args.push('--release');
  run('cargo', args);
  return join(root, 'src-tauri', 'target', target, debug ? 'debug' : 'release', sourceName(target));
}

function copySidecar(source, target) {
  mkdirSync(binaries, { recursive: true });
  const destination = join(binaries, sidecarName(target));
  copyFileSync(source, destination);
  if (!target.includes('windows')) chmodSync(destination, 0o755);
  process.stdout.write(`[OK] ${destination}\n`);
}

const target = requested || hostTriple();
if (target === 'universal-apple-darwin') {
  if (debug) throw new Error('Universal sidecar preparation is release-only');
  const intel = build('x86_64-apple-darwin');
  const apple = build('aarch64-apple-darwin');
  mkdirSync(binaries, { recursive: true });
  const destination = join(binaries, sidecarName(target));
  run('lipo', ['-create', intel, apple, '-output', destination]);
  chmodSync(destination, 0o755);
  process.stdout.write(`[OK] ${destination}\n`);
} else {
  copySidecar(build(target), target);
}

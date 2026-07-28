#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function readJson(relativePath) {
  return JSON.parse(readFileSync(resolve(repositoryRoot, relativePath), 'utf8'));
}

function fail(message) {
  console.error(`Release version check failed: ${message}`);
  process.exit(1);
}

const packageVersion = readJson('package.json').version;
const tauriVersion = readJson('src-tauri/tauri.conf.json').version;
const cargoToml = readFileSync(resolve(repositoryRoot, 'src-tauri/Cargo.toml'), 'utf8');
const packageSection = cargoToml.match(/\[package\]([\s\S]*?)(?=\n\[|$)/);
const cargoVersion = packageSection?.[1].match(/^version\s*=\s*"([^"]+)"/m)?.[1];

if (!packageVersion || !tauriVersion || !cargoVersion) {
  fail('could not read all three application versions');
}

const versions = new Map([
  ['package.json', packageVersion],
  ['src-tauri/Cargo.toml', cargoVersion],
  ['src-tauri/tauri.conf.json', tauriVersion],
]);
const uniqueVersions = new Set(versions.values());

if (uniqueVersions.size !== 1) {
  fail(
    [...versions.entries()]
      .map(([file, version]) => `${file}=${version}`)
      .join(', '),
  );
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(packageVersion)) {
  fail(`unsupported version format: ${packageVersion}`);
}

const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;
const expectedTag = `v${packageVersion}`;

if (tag !== expectedTag) {
  fail(`tag ${tag ?? '<missing>'} must equal ${expectedTag}`);
}

console.log(`Release version check passed: ${expectedTag}`);

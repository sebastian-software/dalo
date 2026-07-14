'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { execFile } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const { version: packageVersion } = require('../package.json');
const {
  ensureBinary,
  expectedChecksum,
  formatLauncherError,
  npmInstallChannel,
  normalizeTag,
  targetFor,
  versionFromTag
} = require('../lib/release');

const execFileAsync = promisify(execFile);

async function writeCachedBinary(cacheRoot, version, target) {
  const binary = path.join(cacheRoot, version, target, 'dalo');
  await fs.mkdir(path.dirname(binary), { recursive: true });
  await fs.writeFile(binary, Buffer.alloc(2048, version), { mode: 0o755 });
  return binary;
}

test('maps supported Node platforms to release targets', () => {
  assert.equal(targetFor('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(targetFor('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.equal(targetFor('linux', 'x64', 'musl'), 'x86_64-unknown-linux-musl');
  assert.equal(targetFor('linux', 'arm64', 'musl'), 'aarch64-unknown-linux-musl');
  assert.throws(() => targetFor('linux', 'x64', 'other'), /supported values are gnu and musl/);
  assert.throws(() => targetFor('win32', 'x64'), /unsupported platform/);
});

test('parses release tags and checksum files strictly', () => {
  assert.equal(versionFromTag('dalo-v0.6.0'), '0.6.0');
  assert.equal(versionFromTag('v0.6.0'), '0.6.0');
  assert.equal(normalizeTag('0.6.0'), 'dalo-v0.6.0');
  assert.equal(normalizeTag('v0.6.0'), 'dalo-v0.6.0');
  assert.equal(normalizeTag('latest'), 'latest');
  assert.throws(() => normalizeTag('release-0.6'), /use X\.Y\.Z/);
  const checksums = `${'a'.repeat(64)}  other.tar.gz\n${'b'.repeat(64)} *dalo.tar.gz\n`;
  assert.equal(expectedChecksum(checksums, 'dalo.tar.gz'), 'b'.repeat(64));
  assert.throws(() => expectedChecksum(checksums, 'missing.tar.gz'), /no entry/);
  assert.throws(() => expectedChecksum('not-a-checksum\n', 'dalo.tar.gz'), /malformed/);
});

test('identifies npm and npx launcher executions for update guidance', () => {
  assert.equal(npmInstallChannel(undefined, '/usr/local/lib/node_modules/getdalo/bin/dalo.js'), 'npm');
  assert.equal(npmInstallChannel('exec', '/usr/local/lib/node_modules/getdalo/bin/dalo.js'), 'npx');
  assert.equal(npmInstallChannel(undefined, '/home/user/.npm/_npx/123/node_modules/getdalo/bin/dalo.js'), 'npx');
});

test('uses the npm package version from a warm cache without network access', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const cacheRoot = path.join(temp, 'cache');
  const target = 'x86_64-unknown-linux-gnu';
  const originalFetch = global.fetch;
  const originalVersion = process.env.DALO_VERSION;
  try {
    const binary = await writeCachedBinary(cacheRoot, packageVersion, target);
    delete process.env.DALO_VERSION;
    global.fetch = async () => {
      throw new Error('network should not be used for a warm package-version cache');
    };

    assert.equal(await ensureBinary({ target, cacheRoot }), binary);
  } finally {
    global.fetch = originalFetch;
    if (originalVersion === undefined) delete process.env.DALO_VERSION;
    else process.env.DALO_VERSION = originalVersion;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('falls back to the newest cached binary when an explicit latest lookup fails', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const cacheRoot = path.join(temp, 'cache');
  const target = 'x86_64-unknown-linux-gnu';
  const originalFetch = global.fetch;
  const originalEmitWarning = process.emitWarning;
  const warnings = [];
  try {
    await writeCachedBinary(cacheRoot, '0.9.0', target);
    const newest = await writeCachedBinary(cacheRoot, '0.10.0', target);
    global.fetch = async (_url, options) => {
      assert.ok(options.signal instanceof AbortSignal);
      throw new TypeError('fetch failed', { cause: new Error('getaddrinfo ENOTFOUND api.github.com') });
    };
    process.emitWarning = (warning, options) => warnings.push({ warning, options });

    assert.equal(await ensureBinary({ tag: ' latest ', target, cacheRoot }), newest);
    assert.equal(warnings.length, 1);
    assert.match(warnings[0].warning, /using cached version 0\.10\.0/);
    assert.equal(warnings[0].options.code, 'DALO_CACHE_FALLBACK');
  } finally {
    global.fetch = originalFetch;
    process.emitWarning = originalEmitWarning;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('orders cached prerelease fallbacks by SemVer precedence', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const cacheRoot = path.join(temp, 'cache');
  const target = 'x86_64-unknown-linux-gnu';
  const originalFetch = global.fetch;
  const originalEmitWarning = process.emitWarning;
  try {
    await writeCachedBinary(cacheRoot, '1.0.0-alpha.10', target);
    await writeCachedBinary(cacheRoot, '1.0.0-alpha.Z', target);
    const newest = await writeCachedBinary(cacheRoot, '1.0.0-alpha.beta', target);
    global.fetch = async () => {
      throw new Error('offline');
    };
    process.emitWarning = () => {};

    assert.equal(await ensureBinary({ tag: 'latest', target, cacheRoot }), newest);
  } finally {
    global.fetch = originalFetch;
    process.emitWarning = originalEmitWarning;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('reports available cache versions when an exact download fails', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const cacheRoot = path.join(temp, 'cache');
  const target = 'x86_64-unknown-linux-gnu';
  const originalFetch = global.fetch;
  try {
    await writeCachedBinary(cacheRoot, '0.7.0', target);
    global.fetch = async () => {
      throw new TypeError('fetch failed', { cause: new Error('network unreachable') });
    };

    await assert.rejects(
      ensureBinary({ tag: '0.8.0', target, cacheRoot }),
      /usable cached versions for x86_64-unknown-linux-gnu: 0\.7\.0/
    );
  } finally {
    global.fetch = originalFetch;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('formats network causes and an actionable version hint', () => {
  const error = new TypeError('fetch failed', {
    cause: new Error('getaddrinfo ENOTFOUND api.github.com')
  });

  const message = formatLauncherError(error);

  assert.match(message, /fetch failed: getaddrinfo ENOTFOUND api\.github\.com/);
  assert.match(message, /DALO_VERSION to X\.Y\.Z/);
  assert.match(message, /DALO_VERSION=latest/);
});

test('downloads, verifies, extracts, and caches a matching release archive', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const target = 'x86_64-unknown-linux-gnu';
  const version = '0.6.0';
  const packageName = `dalo-${version}-${target}`;
  const packageDir = path.join(temp, packageName);
  const archive = path.join(temp, `${packageName}.tar.gz`);
  const cacheRoot = path.join(temp, 'cache');
  const originalFetch = global.fetch;
  const originalBaseUrl = process.env.DALO_RELEASE_BASE_URL;
  try {
    await fs.mkdir(packageDir);
    await fs.writeFile(path.join(packageDir, 'dalo'), '#!/bin/sh\necho dalo\n', { mode: 0o755 });
    await execFileAsync('tar', ['-C', temp, '-czf', archive, packageName]);
    const archiveBytes = await fs.readFile(archive);
    const checksum = createHash('sha256').update(archiveBytes).digest('hex');
    process.env.DALO_RELEASE_BASE_URL = 'https://releases.example.test';
    global.fetch = async (url, options) => {
      assert.ok(options.signal instanceof AbortSignal);
      if (url.endsWith('.sha256')) {
        return new Response(`${checksum}  ${path.basename(archive)}\n`, { status: 200 });
      }
      if (url.endsWith(path.basename(archive))) {
        return new Response(archiveBytes, { status: 200 });
      }
      return new Response('', { status: 404 });
    };

    const binary = await ensureBinary({ tag: version, target, cacheRoot });
    assert.equal(await fs.readFile(binary, 'utf8'), '#!/bin/sh\necho dalo\n');
    assert.equal(await ensureBinary({ tag: `v${version}`, target, cacheRoot }), binary);
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('repairs a truncated or non-executable cache entry', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const target = 'x86_64-unknown-linux-gnu';
  const version = '0.6.0';
  const packageName = `dalo-${version}-${target}`;
  const packageDir = path.join(temp, packageName);
  const cacheRoot = path.join(temp, 'cache');
  const archive = path.join(temp, `${packageName}.tar.gz`);
  const originalFetch = global.fetch;
  const originalBaseUrl = process.env.DALO_RELEASE_BASE_URL;
  try {
    await fs.mkdir(packageDir);
    await fs.writeFile(path.join(packageDir, 'dalo'), '#!/bin/sh\necho dalo\n', { mode: 0o755 });
    await execFileAsync('tar', ['-C', temp, '-czf', archive, packageName]);
    const archiveBytes = await fs.readFile(archive);
    const checksum = createHash('sha256').update(archiveBytes).digest('hex');
    const binary = path.join(cacheRoot, version, target, 'dalo');
    await fs.mkdir(path.dirname(binary), { recursive: true });
    await fs.writeFile(binary, 'partial', { mode: 0o644 });
    process.env.DALO_RELEASE_BASE_URL = 'https://releases.example.test';
    global.fetch = async (url) => new Response(
      url.endsWith('.sha256') ? `${checksum}  ${path.basename(archive)}\n` : archiveBytes,
      { status: 200 }
    );
    const repaired = await ensureBinary({ tag: `dalo-v${version}`, target, cacheRoot });
    assert.equal(repaired, binary);
    assert.ok((await fs.stat(binary)).mode & 0o100);
    assert.ok((await fs.stat(binary)).size >= 1024 || (await fs.readFile(binary, 'utf8')).startsWith('#!'));
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

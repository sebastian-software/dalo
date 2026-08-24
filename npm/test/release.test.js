'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { execFile } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const packageManifest = require('../package.json');
const { version: packageVersion } = packageManifest;
const {
  compareVersions,
  detectLinuxLibc,
  ensureBinary,
  expectedChecksum,
  formatLauncherError,
  launcherEnvironment,
  npmInstallChannel,
  normalizeTag,
  targetFor,
  versionFromTag
} = require('../lib/release');

const execFileAsync = promisify(execFile);

test('publishes discovery and supported-platform metadata', () => {
  assert.equal(packageManifest.description, 'npm launcher for Dalo on macOS and Linux');
  assert.equal(packageManifest.homepage, 'https://dalo.sh');
  assert.equal(packageManifest.bugs.url, 'https://github.com/sebastian-software/dalo/issues');
  assert.deepEqual(packageManifest.keywords, ['dalo', 'ai', 'agents', 'skills', 'cli']);
  assert.deepEqual(packageManifest.os, ['darwin', 'linux']);
});

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

test('detects Linux libc from overrides and the runtime report', async () => {
  const originalOverride = process.env.DALO_LINUX_LIBC;
  const originalGetReport = process.report.getReport;
  const originalPath = process.env.PATH;
  try {
    process.env.DALO_LINUX_LIBC = 'musl';
    assert.equal(await detectLinuxLibc(), 'musl');
    process.env.DALO_LINUX_LIBC = 'gnu';
    assert.equal(await detectLinuxLibc(), 'gnu');
    process.env.DALO_LINUX_LIBC = 'unsupported';
    await assert.rejects(detectLinuxLibc(), /supported values are gnu and musl/);

    delete process.env.DALO_LINUX_LIBC;
    process.report.getReport = () => ({ header: { glibcVersionRuntime: '2.39' } });
    process.env.PATH = path.join(os.tmpdir(), 'dalo-no-ldd');
    assert.equal(await detectLinuxLibc(), 'gnu');
  } finally {
    if (originalOverride === undefined) delete process.env.DALO_LINUX_LIBC;
    else process.env.DALO_LINUX_LIBC = originalOverride;
    process.report.getReport = originalGetReport;
    process.env.PATH = originalPath;
  }
});

test('detects musl from ldd output and warns on an unknown libc', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const ldd = path.join(temp, 'ldd');
  const originalOverride = process.env.DALO_LINUX_LIBC;
  const originalGetReport = process.report.getReport;
  const originalPath = process.env.PATH;
  const originalEmitWarning = process.emitWarning;
  const warnings = [];
  try {
    delete process.env.DALO_LINUX_LIBC;
    process.report.getReport = () => ({ header: {} });
    process.env.PATH = temp;

    await fs.writeFile(ldd, '#!/bin/sh\nprintf "musl libc 1.2.5\\n"\n', { mode: 0o755 });
    assert.equal(await detectLinuxLibc(), 'musl');

    await fs.writeFile(ldd, '#!/bin/sh\nprintf "musl loader error\\n" >&2\nexit 1\n', { mode: 0o755 });
    assert.equal(await detectLinuxLibc(), 'musl');

    await fs.writeFile(ldd, '#!/bin/sh\nprintf "unknown libc\\n" >&2\nexit 1\n', { mode: 0o755 });
    process.emitWarning = (warning) => warnings.push(warning);
    assert.equal(await detectLinuxLibc(), 'gnu');
    assert.deepEqual(warnings, [
      'could not detect Linux libc; falling back to GNU (set DALO_LINUX_LIBC=gnu or musl to override)'
    ]);
  } finally {
    if (originalOverride === undefined) delete process.env.DALO_LINUX_LIBC;
    else process.env.DALO_LINUX_LIBC = originalOverride;
    process.report.getReport = originalGetReport;
    process.env.PATH = originalPath;
    process.emitWarning = originalEmitWarning;
    await fs.rm(temp, { recursive: true, force: true });
  }
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

test('orders short version cores without throwing', () => {
  assert.ok(compareVersions('1.0', '1.0.1') < 0);
  assert.equal(compareVersions('1.0', '1.0.0'), 0);
});

test('identifies npm and npx launcher executions for update guidance', () => {
  assert.equal(npmInstallChannel(undefined, '/usr/local/lib/node_modules/getdalo/bin/dalo.js'), 'npm');
  assert.equal(npmInstallChannel('exec', '/usr/local/lib/node_modules/getdalo/bin/dalo.js'), 'npx');
  assert.equal(npmInstallChannel(undefined, '/home/user/.npm/_npx/123/node_modules/getdalo/bin/dalo.js'), 'npx');
});

test('passes only the persistent global npm launcher to the Rust binary', () => {
  const globalLauncher = '/usr/local/bin/dalo';
  const npmEnvironment = launcherEnvironment(
    { npm_command: 'install', DALO_INVOKED_EXECUTABLE: '/stale/dalo' },
    globalLauncher
  );
  assert.equal(npmEnvironment.DALO_INSTALL_CHANNEL, 'npm');
  assert.equal(npmEnvironment.DALO_INVOKED_EXECUTABLE, globalLauncher);

  const npxEnvironment = launcherEnvironment(
    { npm_command: 'exec', DALO_INVOKED_EXECUTABLE: '/stale/dalo' },
    '/home/user/.npm/_npx/123/node_modules/getdalo/bin/dalo.js'
  );
  assert.equal(npxEnvironment.DALO_INSTALL_CHANNEL, 'npx');
  assert.equal(npxEnvironment.DALO_INVOKED_EXECUTABLE, undefined);
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

test('rejects a malformed latest-release response before downloading artifacts', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const cacheRoot = path.join(temp, 'cache');
  const target = 'x86_64-unknown-linux-gnu';
  const originalFetch = global.fetch;
  const requests = [];
  try {
    global.fetch = async (url, options) => {
      requests.push(url);
      assert.ok(options.signal instanceof AbortSignal);
      return new Response(JSON.stringify({ tag_name: 42 }), {
        status: 200,
        headers: { 'content-type': 'application/json' }
      });
    };

    await assert.rejects(
      ensureBinary({ tag: 'latest', target, cacheRoot }),
      /latest GitHub release has no tag name; no usable cached version/
    );
    assert.deepEqual(requests, ['https://api.github.com/repos/sebastian-software/dalo/releases/latest']);
    await assert.rejects(fs.access(cacheRoot), { code: 'ENOENT' });
  } finally {
    global.fetch = originalFetch;
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

test('rejects a mismatched checksum and cleans up without promoting a binary', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const downloadRoot = path.join(temp, 'downloads');
  const target = 'x86_64-unknown-linux-gnu';
  const version = '0.6.0';
  const packageName = `dalo-${version}-${target}`;
  const packageDir = path.join(temp, packageName);
  const archive = path.join(temp, `${packageName}.tar.gz`);
  const cacheRoot = path.join(temp, 'cache');
  const binary = path.join(cacheRoot, version, target, 'dalo');
  const originalFetch = global.fetch;
  const originalBaseUrl = process.env.DALO_RELEASE_BASE_URL;
  const originalTmpdir = process.env.TMPDIR;
  try {
    await fs.mkdir(packageDir);
    await fs.mkdir(downloadRoot);
    await fs.writeFile(path.join(packageDir, 'dalo'), Buffer.alloc(2048, 'x'), { mode: 0o755 });
    await execFileAsync('tar', ['-C', temp, '-czf', archive, packageName]);
    const archiveBytes = await fs.readFile(archive);
    process.env.DALO_RELEASE_BASE_URL = 'https://releases.example.test';
    process.env.TMPDIR = downloadRoot;
    global.fetch = async (url) => new Response(
      url.endsWith('.sha256') ? `${'0'.repeat(64)}  ${path.basename(archive)}\n` : archiveBytes,
      { status: 200 }
    );

    await assert.rejects(
      ensureBinary({ tag: version, target, cacheRoot }),
      /release checksum did not match; refusing to run the downloaded binary/
    );
    await assert.rejects(fs.access(binary), { code: 'ENOENT' });
    assert.deepEqual(await fs.readdir(downloadRoot), []);
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    if (originalTmpdir === undefined) delete process.env.TMPDIR;
    else process.env.TMPDIR = originalTmpdir;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

test('removes a staged binary when final cache promotion fails', async () => {
  const temp = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-test-'));
  const downloadRoot = path.join(temp, 'downloads');
  const target = 'x86_64-unknown-linux-gnu';
  const version = '0.6.0';
  const packageName = `dalo-${version}-${target}`;
  const packageDir = path.join(temp, packageName);
  const archive = path.join(temp, `${packageName}.tar.gz`);
  const cacheRoot = path.join(temp, 'cache');
  const cacheDir = path.join(cacheRoot, version, target);
  const binary = path.join(cacheDir, 'dalo');
  const originalFetch = global.fetch;
  const originalBaseUrl = process.env.DALO_RELEASE_BASE_URL;
  const originalTmpdir = process.env.TMPDIR;
  try {
    await fs.mkdir(packageDir);
    await fs.mkdir(downloadRoot);
    await fs.writeFile(path.join(packageDir, 'dalo'), Buffer.alloc(2048, 'x'), { mode: 0o755 });
    await execFileAsync('tar', ['-C', temp, '-czf', archive, packageName]);
    const archiveBytes = await fs.readFile(archive);
    const checksum = createHash('sha256').update(archiveBytes).digest('hex');
    await fs.mkdir(binary, { recursive: true });
    await fs.chmod(binary, 0o600);
    process.env.DALO_RELEASE_BASE_URL = 'https://releases.example.test';
    process.env.TMPDIR = downloadRoot;
    global.fetch = async (url) => new Response(
      url.endsWith('.sha256') ? `${checksum}  ${path.basename(archive)}\n` : archiveBytes,
      { status: 200 }
    );

    await assert.rejects(
      ensureBinary({ tag: version, target, cacheRoot }),
      (error) => {
        assert.equal(error.cause?.code, 'EISDIR');
        return true;
      }
    );
    assert.equal((await fs.stat(binary)).isDirectory(), true);
    assert.deepEqual(await fs.readdir(cacheDir), ['dalo']);
    assert.deepEqual(await fs.readdir(downloadRoot), []);
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    if (originalTmpdir === undefined) delete process.env.TMPDIR;
    else process.env.TMPDIR = originalTmpdir;
    await fs.rm(temp, { recursive: true, force: true });
  }
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
  const expectedBinary = Buffer.concat([
    Buffer.from('#!/bin/sh\n'),
    Buffer.alloc(2048, '#'),
    Buffer.from('\n')
  ]);
  const originalFetch = global.fetch;
  const originalBaseUrl = process.env.DALO_RELEASE_BASE_URL;
  try {
    await fs.mkdir(packageDir);
    await fs.writeFile(path.join(packageDir, 'dalo'), expectedBinary, { mode: 0o755 });
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
    assert.deepEqual(await fs.readFile(binary), expectedBinary);
    assert.equal((await fs.stat(binary)).mode & 0o111, 0o111);
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

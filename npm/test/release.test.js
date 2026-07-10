'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const { execFile } = require('node:child_process');
const { createHash } = require('node:crypto');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const { ensureBinary, expectedChecksum, targetFor, versionFromTag } = require('../lib/release');

const execFileAsync = promisify(execFile);

test('maps supported Node platforms to release targets', () => {
  assert.equal(targetFor('darwin', 'arm64'), 'aarch64-apple-darwin');
  assert.equal(targetFor('linux', 'x64'), 'x86_64-unknown-linux-gnu');
  assert.throws(() => targetFor('win32', 'x64'), /unsupported platform/);
});

test('parses release tags and checksum files strictly', () => {
  assert.equal(versionFromTag('dalo-v0.6.0'), '0.6.0');
  assert.equal(versionFromTag('v0.6.0'), '0.6.0');
  assert.equal(expectedChecksum('a'.repeat(64) + '  dalo.tar.gz\n'), 'a'.repeat(64));
  assert.throws(() => expectedChecksum('not-a-checksum\n'), /malformed/);
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
    global.fetch = async (url) => {
      if (url.endsWith('.sha256')) {
        return new Response(`${checksum}  ${path.basename(archive)}\n`, { status: 200 });
      }
      if (url.endsWith(path.basename(archive))) {
        return new Response(archiveBytes, { status: 200 });
      }
      return new Response('', { status: 404 });
    };

    const binary = await ensureBinary({ tag: `dalo-v${version}`, target, cacheRoot });
    assert.equal(await fs.readFile(binary, 'utf8'), '#!/bin/sh\necho dalo\n');
    assert.equal(await ensureBinary({ tag: `dalo-v${version}`, target, cacheRoot }), binary);
  } finally {
    global.fetch = originalFetch;
    if (originalBaseUrl === undefined) delete process.env.DALO_RELEASE_BASE_URL;
    else process.env.DALO_RELEASE_BASE_URL = originalBaseUrl;
    await fs.rm(temp, { recursive: true, force: true });
  }
});

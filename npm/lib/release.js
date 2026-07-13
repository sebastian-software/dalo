'use strict';

const { createHash } = require('node:crypto');
const { execFile } = require('node:child_process');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');

const execFileAsync = promisify(execFile);
const REPOSITORY = 'sebastian-software/dalo';

function targetFor(platform = process.platform, arch = process.arch, libc) {
  if (platform === 'linux' && libc !== undefined && libc !== 'gnu' && libc !== 'musl') {
    throw new Error(`invalid DALO_LINUX_LIBC value: ${libc}; supported values are gnu and musl`);
  }
  const linuxLibc = libc || 'gnu';
  const targets = {
    'darwin:x64': 'x86_64-apple-darwin',
    'darwin:arm64': 'aarch64-apple-darwin',
    'linux:x64': `x86_64-unknown-linux-${linuxLibc}`,
    'linux:arm64': `aarch64-unknown-linux-${linuxLibc}`
  };
  const target = targets[`${platform}:${arch}`];
  if (!target) {
    throw new Error(`unsupported platform: ${platform} ${arch}; supported targets are macOS and Linux on x64 or arm64`);
  }
  return target;
}

async function detectLinuxLibc() {
  const override = process.env.DALO_LINUX_LIBC;
  if (override !== undefined) {
    if (override !== 'gnu' && override !== 'musl') {
      throw new Error(`invalid DALO_LINUX_LIBC value: ${override}; supported values are gnu and musl`);
    }
    return override;
  }
  if (process.report?.getReport?.().header?.glibcVersionRuntime) return 'gnu';
  try {
    const { stdout, stderr } = await execFileAsync('ldd', ['--version']);
    return /musl/i.test(`${stdout}\n${stderr}`) ? 'musl' : 'gnu';
  } catch (error) {
    const output = `${error.stdout || ''}\n${error.stderr || ''}`;
    if (/musl/i.test(output)) return 'musl';
    process.emitWarning('could not detect Linux libc; falling back to GNU (set DALO_LINUX_LIBC=gnu or musl to override)');
    return 'gnu';
  }
}

async function targetForCurrentRuntime() {
  return targetFor(process.platform, process.arch,
    process.platform === 'linux' ? await detectLinuxLibc() : undefined);
}

function versionFromTag(tag) {
  return tag.replace(/^dalo-v/, '').replace(/^v/, '');
}

function releaseBaseUrl() {
  return process.env.DALO_RELEASE_BASE_URL || `https://github.com/${REPOSITORY}/releases/download`;
}

async function latestTag() {
  const response = await fetch(`https://api.github.com/repos/${REPOSITORY}/releases/latest`, {
    headers: { accept: 'application/vnd.github+json', 'user-agent': 'dalo-npm-wrapper' }
  });
  if (!response.ok) {
    throw new Error(`could not resolve the latest Dalo release (GitHub returned ${response.status})`);
  }
  const release = await response.json();
  if (!release.tag_name || typeof release.tag_name !== 'string') {
    throw new Error('latest GitHub release has no tag name');
  }
  return release.tag_name;
}

async function fetchFile(url, destination) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`download failed for ${url} (HTTP ${response.status})`);
  }
  await fs.writeFile(destination, Buffer.from(await response.arrayBuffer()), { mode: 0o600 });
}

function expectedChecksum(contents) {
  const match = contents.match(/^([a-fA-F0-9]{64})\s+\*?.+$/m);
  if (!match) {
    throw new Error('release checksum file is malformed');
  }
  return match[1].toLowerCase();
}

async function verifyChecksum(archive, checksumFile) {
  const expected = expectedChecksum(await fs.readFile(checksumFile, 'utf8'));
  const actual = createHash('sha256').update(await fs.readFile(archive)).digest('hex');
  if (actual !== expected) {
    throw new Error('release checksum did not match; refusing to run the downloaded binary');
  }
}

async function ensureBinary({ tag, target, cacheRoot } = {}) {
  const resolvedTag = tag || process.env.DALO_VERSION || await latestTag();
  const resolvedTarget = target || await targetForCurrentRuntime();
  const version = versionFromTag(resolvedTag);
  const packageName = `dalo-${version}-${resolvedTarget}`;
  const binary = path.join(cacheRoot || process.env.DALO_CACHE_DIR || path.join(os.homedir(), '.cache', 'dalo'), version, resolvedTarget, 'dalo');
  try {
    await fs.access(binary, fs.constants.X_OK);
    if ((await fs.stat(binary)).size < 1024) throw new Error('truncated cache entry');
    return binary;
  } catch {
    // Download below.
  }

  const cacheDir = path.dirname(binary);
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-npm-'));
  const stagedBinary = path.join(cacheDir, `.dalo-${process.pid}-${Date.now()}.tmp`);
  const archive = `${packageName}.tar.gz`;
  try {
    const archivePath = path.join(temporary, archive);
    const checksumPath = `${archivePath}.sha256`;
    await fetchFile(`${releaseBaseUrl()}/${resolvedTag}/${archive}`, archivePath);
    await fetchFile(`${releaseBaseUrl()}/${resolvedTag}/${archive}.sha256`, checksumPath);
    await verifyChecksum(archivePath, checksumPath);
    await execFileAsync('tar', ['-xzf', archivePath], { cwd: temporary });
    await fs.mkdir(cacheDir, { recursive: true, mode: 0o700 });
    await fs.copyFile(path.join(temporary, packageName, 'dalo'), stagedBinary);
    await fs.chmod(stagedBinary, 0o755);
    await fs.rename(stagedBinary, binary);
    return binary;
  } finally {
    await fs.rm(stagedBinary, { force: true });
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

module.exports = { detectLinuxLibc, ensureBinary, expectedChecksum, targetFor, targetForCurrentRuntime, verifyChecksum, versionFromTag };

'use strict';

const { createHash } = require('node:crypto');
const { execFile } = require('node:child_process');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const { promisify } = require('node:util');
const { version: PACKAGE_VERSION } = require('../package.json');

const execFileAsync = promisify(execFile);
const REPOSITORY = 'sebastian-software/dalo';
const FETCH_TIMEOUT_MS = 30_000;
const VERSION_PATTERN = /^(?:dalo-v|v)?(\d+\.\d+\.\d+(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)$/;

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
  const normalized = normalizeTag(tag);
  if (normalized === 'latest') {
    throw new Error('`latest` must be resolved before extracting a version');
  }
  return normalized.slice('dalo-v'.length);
}

function normalizeTag(value) {
  const requested = String(value).trim();
  if (requested === 'latest') return requested;
  const match = requested.match(VERSION_PATTERN);
  if (!match) {
    throw new Error(
      `invalid DALO_VERSION value: ${requested || '(empty)'}; use X.Y.Z, vX.Y.Z, dalo-vX.Y.Z, or latest`
    );
  }
  return `dalo-v${match[1]}`;
}

function releaseBaseUrl() {
  return process.env.DALO_RELEASE_BASE_URL || `https://github.com/${REPOSITORY}/releases/download`;
}

async function latestTag() {
  const response = await fetch(`https://api.github.com/repos/${REPOSITORY}/releases/latest`, {
    headers: { accept: 'application/vnd.github+json', 'user-agent': 'dalo-npm-wrapper' },
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS)
  });
  if (!response.ok) {
    throw new Error(`could not resolve the latest Dalo release (GitHub returned ${response.status})`);
  }
  const release = await response.json();
  if (!release.tag_name || typeof release.tag_name !== 'string') {
    throw new Error('latest GitHub release has no tag name');
  }
  return normalizeTag(release.tag_name);
}

async function fetchFile(url, destination) {
  const response = await fetch(url, { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
  if (!response.ok) {
    throw new Error(`download failed for ${url} (HTTP ${response.status})`);
  }
  await fs.writeFile(destination, Buffer.from(await response.arrayBuffer()), { mode: 0o600 });
}

function expectedChecksum(contents, expectedFilename) {
  let validEntry = false;
  for (const line of contents.split(/\r?\n/)) {
    if (!line) continue;
    const match = line.match(/^([a-fA-F0-9]{64})\s+\*?(.+)$/);
    if (!match) continue;
    validEntry = true;
    if (match[2] === expectedFilename) return match[1].toLowerCase();
  }
  if (!validEntry) throw new Error('release checksum file is malformed');
  throw new Error(`release checksum file has no entry for ${expectedFilename}`);
}

async function verifyChecksum(archive, checksumFile) {
  const expected = expectedChecksum(await fs.readFile(checksumFile, 'utf8'), path.basename(archive));
  const actual = createHash('sha256').update(await fs.readFile(archive)).digest('hex');
  if (actual !== expected) {
    throw new Error('release checksum did not match; refusing to run the downloaded binary');
  }
}

async function isUsableBinary(binary) {
  try {
    await fs.access(binary, fs.constants.X_OK);
    return (await fs.stat(binary)).size >= 1024;
  } catch {
    return false;
  }
}

function compareVersions(left, right) {
  const compareNumericIdentifiers = (a, b) => {
    const normalizedA = a.replace(/^0+/, '') || '0';
    const normalizedB = b.replace(/^0+/, '') || '0';
    if (normalizedA.length !== normalizedB.length) return normalizedA.length - normalizedB.length;
    if (normalizedA === normalizedB) return 0;
    return normalizedA < normalizedB ? -1 : 1;
  };
  const parse = (version) => {
    const withoutBuild = version.split('+', 1)[0];
    const separator = withoutBuild.indexOf('-');
    return {
      core: (separator === -1 ? withoutBuild : withoutBuild.slice(0, separator))
        .split('.'),
      prerelease: separator === -1 ? null : withoutBuild.slice(separator + 1)
    };
  };
  const a = parse(left);
  const b = parse(right);
  for (let index = 0; index < 3; index += 1) {
    const comparison = compareNumericIdentifiers(a.core[index], b.core[index]);
    if (comparison !== 0) return comparison;
  }
  if (a.prerelease === null && b.prerelease !== null) return 1;
  if (a.prerelease !== null && b.prerelease === null) return -1;
  if (a.prerelease === null) return 0;
  const aIdentifiers = a.prerelease.split('.');
  const bIdentifiers = b.prerelease.split('.');
  const commonLength = Math.min(aIdentifiers.length, bIdentifiers.length);
  for (let index = 0; index < commonLength; index += 1) {
    const aIdentifier = aIdentifiers[index];
    const bIdentifier = bIdentifiers[index];
    if (aIdentifier === bIdentifier) continue;
    const aNumeric = /^\d+$/.test(aIdentifier);
    const bNumeric = /^\d+$/.test(bIdentifier);
    if (aNumeric && bNumeric) return compareNumericIdentifiers(aIdentifier, bIdentifier);
    if (aNumeric !== bNumeric) return aNumeric ? -1 : 1;
    return aIdentifier < bIdentifier ? -1 : 1;
  }
  return aIdentifiers.length - bIdentifiers.length;
}

async function cachedBinaries(cacheRoot, target) {
  let entries;
  try {
    entries = await fs.readdir(cacheRoot, { withFileTypes: true });
  } catch (error) {
    if (error.code === 'ENOENT') return [];
    throw error;
  }
  const cached = [];
  for (const entry of entries) {
    if (!entry.isDirectory()) continue;
    let version;
    try {
      version = versionFromTag(entry.name);
    } catch {
      continue;
    }
    const binary = path.join(cacheRoot, entry.name, target, 'dalo');
    if (await isUsableBinary(binary)) cached.push({ version, binary });
  }
  cached.sort((a, b) => compareVersions(b.version, a.version));
  return cached;
}

function errorMessageWithCauses(error) {
  let message = error?.message || String(error);
  let cause = error?.cause;
  while (cause) {
    const causeMessage = cause.message || String(cause);
    if (causeMessage && !message.includes(causeMessage)) message += `: ${causeMessage}`;
    cause = cause.cause;
  }
  return message;
}

function withCacheContext(error, cached, target) {
  const context = cached.length > 0
    ? `usable cached versions for ${target}: ${cached.map((entry) => entry.version).join(', ')}`
    : `no usable cached version for ${target}`;
  return new Error(`${error.message}; ${context}`, { cause: error });
}

function formatLauncherError(error) {
  return `${errorMessageWithCauses(error)}\ndalo: hint: check network/proxy access or set DALO_VERSION to X.Y.Z; use DALO_VERSION=latest only for explicit update checks`;
}

async function ensureBinary({ tag, target, cacheRoot } = {}) {
  const resolvedTarget = target || await targetForCurrentRuntime();
  const resolvedCacheRoot = cacheRoot || process.env.DALO_CACHE_DIR || path.join(os.homedir(), '.cache', 'dalo');
  const requestedVersion = tag ?? process.env.DALO_VERSION ?? PACKAGE_VERSION;
  const normalizedRequest = normalizeTag(requestedVersion);
  let resolvedTag;
  if (normalizedRequest === 'latest') {
    try {
      resolvedTag = await latestTag();
    } catch (error) {
      const cached = await cachedBinaries(resolvedCacheRoot, resolvedTarget);
      if (cached.length > 0) {
        process.emitWarning(
          `latest Dalo release lookup failed (${errorMessageWithCauses(error)}); using cached version ${cached[0].version}`,
          { code: 'DALO_CACHE_FALLBACK' }
        );
        return cached[0].binary;
      }
      throw withCacheContext(error, cached, resolvedTarget);
    }
  } else {
    resolvedTag = normalizedRequest;
  }
  const version = versionFromTag(resolvedTag);
  const packageName = `dalo-${version}-${resolvedTarget}`;
  const binary = path.join(resolvedCacheRoot, version, resolvedTarget, 'dalo');
  if (await isUsableBinary(binary)) return binary;

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
  } catch (error) {
    const cached = await cachedBinaries(resolvedCacheRoot, resolvedTarget).catch(() => []);
    throw withCacheContext(error, cached, resolvedTarget);
  } finally {
    await fs.rm(stagedBinary, { force: true });
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

module.exports = {
  detectLinuxLibc,
  ensureBinary,
  expectedChecksum,
  formatLauncherError,
  normalizeTag,
  targetFor,
  targetForCurrentRuntime,
  verifyChecksum,
  versionFromTag
};

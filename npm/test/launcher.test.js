'use strict';

const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const fs = require('node:fs/promises');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { once } = require('node:events');
const { targetFor } = require('../lib/release');
const { version } = require('../package.json');

const launcher = path.join(__dirname, '..', 'bin', 'dalo.js');
const supported = (process.platform === 'linux' && ['x64', 'arm64'].includes(process.arch))
  || (process.platform === 'darwin' && process.arch === 'arm64');

for (const signal of ['SIGTERM', 'SIGINT']) {
  test(`forwards ${signal} and waits for the cached binary to clean up`, { skip: !supported, timeout: 10000 }, async (t) => {
    const cache = await fs.mkdtemp(path.join(os.tmpdir(), 'dalo-launcher-test-'));
    const target = targetFor(process.platform, process.arch, 'gnu');
    const binary = path.join(cache, version, target, 'dalo');
    const ready = path.join(cache, 'ready');
    const cleaned = path.join(cache, 'cleaned');
    await fs.mkdir(path.dirname(binary), { recursive: true });
    const script = `#!${process.execPath}\n` + `
      const fs = require('node:fs');
      process.on('${signal}', () => {
        setTimeout(() => {
          fs.writeFileSync(process.env.CLEANED, 'done');
          process.exit(0);
        }, 100);
      });
      fs.writeFileSync(process.env.READY, String(process.pid));
      setInterval(() => {}, 1000);
    `;
    await fs.writeFile(binary, script + `\n// ${'x'.repeat(1024)}\n`, { mode: 0o755 });
    const child = spawn(process.execPath, [launcher], {
      env: { ...process.env, DALO_CACHE_DIR: cache, DALO_VERSION: version, DALO_LINUX_LIBC: 'gnu', READY: ready, CLEANED: cleaned },
      stdio: 'ignore'
    });
    let nativePid;
    t.after(async () => {
      if (!nativePid) {
        try { nativePid = Number(await fs.readFile(ready, 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
      }
      if (nativePid) {
        try { process.kill(nativePid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      }
      if (child.exitCode === null && child.signalCode === null) {
        child.kill('SIGKILL');
        await closed;
      }
      await fs.rm(cache, { recursive: true, force: true });
    });
    const closed = once(child, 'close');
    const deadline = Date.now() + 5000;
    while (!nativePid && Date.now() < deadline) {
      try { nativePid = Number(await fs.readFile(ready, 'utf8')); } catch (error) {
        if (error.code !== 'ENOENT') throw error;
        if (child.exitCode !== null || child.signalCode !== null) throw new Error('launcher exited before child was ready');
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    }
    assert.ok(nativePid, 'native binary became ready');
    child.kill(signal);
    const [code, exitSignal] = await closed;
    assert.equal(await fs.readFile(cleaned, 'utf8'), 'done');
    assert.equal(exitSignal, null);
    assert.equal(code, 128 + os.constants.signals[signal]);
    assert.throws(() => process.kill(nativePid, 0), { code: 'ESRCH' });
    await fs.unlink(ready);
    nativePid = undefined;
  });
}

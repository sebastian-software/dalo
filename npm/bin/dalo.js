#!/usr/bin/env node

'use strict';

const [major, minor] = process.versions.node.split('.').map(Number);
if (major < 22 || (major === 22 && minor < 19)) {
  console.error('dalo: Node.js 22.19 or newer is required');
  process.exit(1);
}

const { ensureBinary, formatLauncherError, launcherEnvironment } = require('../lib/release');
const { spawn } = require('node:child_process');
const { constants } = require('node:os');

async function main() {
  const binary = await ensureBinary();
  const child = spawn(binary, process.argv.slice(2), {
    stdio: 'inherit',
    env: launcherEnvironment(process.env, process.argv[1])
  });
  let cancellationSignal;
  let spawnFailed = false;
  const forwardSignal = (signal) => {
    cancellationSignal ||= signal;
    if (child.exitCode === null && child.signalCode === null) child.kill(signal);
  };
  const onSigint = () => forwardSignal('SIGINT');
  const onSigterm = () => forwardSignal('SIGTERM');
  process.on('SIGINT', onSigint);
  process.on('SIGTERM', onSigterm);
  child.on('error', (error) => {
    spawnFailed = true;
    console.error(`dalo: could not start downloaded binary: ${error.message}`);
    process.exitCode = 1;
  });
  child.on('close', (code, signal) => {
    process.removeListener('SIGINT', onSigint);
    process.removeListener('SIGTERM', onSigterm);
    const terminationSignal = cancellationSignal || signal;
    if (terminationSignal) process.exitCode = 128 + constants.signals[terminationSignal];
    else if (!spawnFailed && code !== null) process.exitCode = code;
  });
}

main().catch((error) => {
  console.error(`dalo: ${formatLauncherError(error)}`);
  process.exitCode = 1;
});

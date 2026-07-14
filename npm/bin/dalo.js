#!/usr/bin/env node

'use strict';

const { ensureBinary, formatLauncherError, npmInstallChannel } = require('../lib/release');
const { spawn } = require('node:child_process');

async function main() {
  const binary = await ensureBinary();
  const child = spawn(binary, process.argv.slice(2), {
    stdio: 'inherit',
    env: {
      ...process.env,
      DALO_INSTALL_CHANNEL: process.env.DALO_INSTALL_CHANNEL || npmInstallChannel(process.env.npm_command, __filename)
    }
  });
  child.on('error', (error) => {
    console.error(`dalo: could not start downloaded binary: ${error.message}`);
    process.exitCode = 1;
  });
  child.on('exit', (code, signal) => {
    process.exitCode = code ?? (signal ? 1 : 0);
  });
}

main().catch((error) => {
  console.error(`dalo: ${formatLauncherError(error)}`);
  process.exitCode = 1;
});

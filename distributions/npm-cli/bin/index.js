#!/usr/bin/env node
'use strict';

const { spawn } = require('child_process');
const path = require('path');
const { resolveBinaryPath } = require('../lib/binary-target');

function main() {
  const binRoot = path.join(__dirname, '..', 'bin');
  const binary = resolveBinaryPath(binRoot);
  const child = spawn(binary, process.argv.slice(2), {
    stdio: 'inherit',
    env: process.env,
  });

  child.on('error', (err) => {
    console.error(`kortolabs-proxy: ${err.message}`);
    process.exit(1);
  });

  child.on('close', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}

main();

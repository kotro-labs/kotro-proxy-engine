/**
 * Maps host OS/arch to the release asset basename shipped under each package's `bin/`.
 * Keep in sync across vscode-extension, npm-cli, and CI release uploads.
 */

const BINARY_BASENAMES = {
  'darwin-arm64': 'korto-proxy-aarch64-apple-darwin',
  'darwin-x64': 'korto-proxy-x86_64-apple-darwin',
  'linux-x64': 'korto-proxy-x86_64-unknown-linux-gnu',
  'win32-x64': 'korto-proxy-x86_64-pc-windows-msvc.exe',
};

/**
 * @param {NodeJS.Platform} platform
 * @param {string} arch
 * @returns {string}
 */
function binaryBasename(platform, arch) {
  if (platform === 'darwin') {
    return arch === 'arm64' ? BINARY_BASENAMES['darwin-arm64'] : BINARY_BASENAMES['darwin-x64'];
  }
  if (platform === 'linux') {
    return BINARY_BASENAMES['linux-x64'];
  }
  if (platform === 'win32') {
    return BINARY_BASENAMES['win32-x64'];
  }
  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

/**
 * @param {string} binRoot Absolute path to the directory containing release binaries.
 * @param {NodeJS.Platform} [platform]
 * @param {string} [arch]
 * @returns {string}
 */
function resolveBinaryPath(binRoot, platform = process.platform, arch = process.arch) {
  const path = require('path');
  const fs = require('fs');
  const name = binaryBasename(platform, arch);
  const candidate = path.join(binRoot, name);
  if (!fs.existsSync(candidate)) {
    throw new Error(
      `Native binary not found at ${candidate}. Run the release build or place prebuilt assets in bin/.`,
    );
  }
  return candidate;
}

module.exports = { BINARY_BASENAMES, binaryBasename, resolveBinaryPath };

/** Platform → release asset basename. Keep in sync with distributions/shared/binary-target.js */

export function binaryBasename(platform: NodeJS.Platform, arch: string): string {
  if (platform === 'darwin') {
    return arch === 'arm64'
      ? 'korto-proxy-aarch64-apple-darwin'
      : 'korto-proxy-x86_64-apple-darwin';
  }
  if (platform === 'linux') {
    return 'korto-proxy-x86_64-unknown-linux-gnu';
  }
  if (platform === 'win32') {
    return 'korto-proxy-x86_64-pc-windows-msvc.exe';
  }
  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

/** Derive a browser-friendly base URL from KORTO_LISTEN_ADDR. */
export function listenBaseUrl(listenAddr: string): string {
  const trimmed = listenAddr.trim();
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    return trimmed.replace(/\/$/, '');
  }
  if (trimmed.startsWith(':')) {
    return `http://127.0.0.1${trimmed}`;
  }
  if (trimmed.includes(':')) {
    return `http://${trimmed}`;
  }
  return 'http://127.0.0.1:8080';
}

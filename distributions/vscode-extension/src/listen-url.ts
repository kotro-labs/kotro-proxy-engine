/** Derive a browser-friendly base URL from a listen address or URL. */
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

/** Telemetry plane base URL for /metrics and /dashboard. */
export function telemetryBaseUrl(metricsAddr: string): string {
  return listenBaseUrl(metricsAddr);
}

/** Convert editor setting to KORTO_* host:port env form. */
export function addrForEnv(addrOrUrl: string): string {
  const trimmed = addrOrUrl.trim();
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    return new URL(trimmed).host;
  }
  return trimmed;
}

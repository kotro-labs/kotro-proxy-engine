import * as vscode from 'vscode';
import { listenBaseUrl, telemetryBaseUrl } from './listen-url';

export type DashboardSnapshot = {
  cache_hit_rate_5m: number;
  cache_hits_5m: number;
  cache_misses_5m: number;
  compressor_bytes_saved_total: number;
  cache_replay_bytes_total: number;
  estimated_dollars_saved: number;
  recent_requests: Array<{
    route: string;
    cache_status: string;
  }>;
};

const POLL_MS = 5000;
const FETCH_TIMEOUT_MS = 2500;
const IDLE_WARNING_MS = 5 * 60 * 1000; // 5 minutes

function formatBytes(n: number): string {
  if (n >= 1048576) {
    return `${(n / 1048576).toFixed(1)}MB`;
  }
  if (n >= 1024) {
    return `${(n / 1024).toFixed(1)}KB`;
  }
  return `${Math.round(n)}B`;
}

function formatTokens(bytes: number): string {
  const tokens = Math.round(bytes / 4);
  if (tokens >= 1000000) {
    return `${(tokens / 1000000).toFixed(1)}M`;
  }
  if (tokens >= 1000) {
    return `${(tokens / 1000).toFixed(1)}k`;
  }
  return `${tokens}`;
}

function formatDollars(d: number): string {
  return `$${d.toFixed(3)}`;
}

function lastCacheLabel(snapshot: DashboardSnapshot | null): string {
  const recent = snapshot?.recent_requests?.[0];
  if (!recent) {
    return 'ready';
  }

  const llmRoute =
    recent.route === '/v1/chat/completions' || recent.route === '/v1/messages';
  if (!llmRoute) {
    return 'idle';
  }

  if (recent.cache_status === 'hit' || recent.cache_status === 'miss') {
    return recent.cache_status.toUpperCase();
  }

  return 'idle';
}

export class ProxyStatusBar implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
  private timer: ReturnType<typeof setInterval> | undefined;
  private listenAddr: string;
  private telemetryAddr: string;
  private dashboardUrl: string;
  private running = false;
  private firstSeenAt: number | null = null;
  private hasWarnedIdle = false;

  constructor(listenAddr: string, telemetryAddr: string) {
    this.listenAddr = listenAddr;
    this.telemetryAddr = telemetryAddr;
    this.dashboardUrl = `${telemetryBaseUrl(telemetryAddr)}/dashboard`;
    this.item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 90);
    this.item.command = 'kotro.connectCursor';
    this.item.tooltip = 'Kotro Proxy Status';
    this.setOffline();
    this.item.show();
  }

  setListenAddr(listenAddr: string, telemetryAddr: string): void {
    this.listenAddr = listenAddr;
    this.telemetryAddr = telemetryAddr;
    this.dashboardUrl = `${telemetryBaseUrl(telemetryAddr)}/dashboard`;
  }

  markRunning(): void {
    this.running = true;
    if (!this.firstSeenAt) {
      this.firstSeenAt = Date.now();
      this.hasWarnedIdle = false;
    }
    void this.refresh();
    this.startPolling();
  }

  markStopped(): void {
    this.running = false;
    this.firstSeenAt = null;
    this.hasWarnedIdle = false;
    this.stopPolling();
    this.setOffline();
  }

  getDashboardUrl(): string {
    return this.dashboardUrl;
  }

  private startPolling(): void {
    this.stopPolling();
    this.timer = setInterval(() => {
      void this.refresh();
    }, POLL_MS);
  }

  private stopPolling(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = undefined;
    }
  }

  private setOffline(): void {
    this.item.text = '$(circle-slash) Kotro: offline';
    this.item.tooltip = 'Kotro proxy sidecar is not running';
  }

  private async refresh(): Promise<void> {
    if (!this.running) {
      return;
    }

    const telemetry = telemetryBaseUrl(this.telemetryAddr);
    const snapshot = await fetchDashboard(`${telemetry}/api/dashboard`);
    if (snapshot) {
      const label = lastCacheLabel(snapshot);
      const totalBytes = snapshot.compressor_bytes_saved_total + snapshot.cache_replay_bytes_total;
      const tokensSaved = formatTokens(totalBytes);
      const dollarsSaved = formatDollars(snapshot.estimated_dollars_saved);
      const hitRate = `${(snapshot.cache_hit_rate_5m * 100).toFixed(0)}%`;
      
      const requests5m = (snapshot.cache_hits_5m || 0) + (snapshot.cache_misses_5m || 0);
      const idleMs = this.firstSeenAt ? Date.now() - this.firstSeenAt : 0;
      
      let trafficHint = '';
      if (requests5m === 0 && idleMs > IDLE_WARNING_MS) {
        this.item.text = `$(warning) Kotro: disconnected`;
        this.item.tooltip = `Proxy is up, but no traffic detected in 5m.\nCursor Auto bypasses local proxies.\nClick to run "Connect Cursor" setup wizard.`;
        if (!this.hasWarnedIdle) {
          this.hasWarnedIdle = true;
          vscode.window.showWarningMessage('Kotro is running, but no traffic detected. Are you using Cursor Auto? Auto bypasses local proxies.', 'Connect Cursor').then(res => {
            if (res === 'Connect Cursor') {
              vscode.commands.executeCommand('kotro.connectCursor');
            }
          });
        }
        return;
      } else {
        this.item.text = `$(pulse) Kotro: ${label} · ${dollarsSaved} saved`;
        trafficHint = label === 'ready' || label === 'idle'
          ? '\nNo LLM traffic yet — run "Kotro: Verify Cache"'
          : '';
        this.item.tooltip = `Total Saved: ${dollarsSaved} (${tokensSaved} tokens)\nCache Hit Rate (5m): ${hitRate}\nLast result: ${label}${trafficHint}\nClick for connection options`;
      }
      return;
    }

    const healthy = await probeHealth(`${listenBaseUrl(this.listenAddr)}/healthz`);
    if (healthy) {
      this.item.text = '$(sync~spin) Kotro: running';
      this.item.tooltip = 'Proxy is up (metrics API unavailable — update to Go proxy v0.2.0+ for savings panel)';
      return;
    }

    this.setOffline();
  }

  dispose(): void {
    this.stopPolling();
    this.item.dispose();
  }
}

async function fetchDashboard(url: string): Promise<DashboardSnapshot | null> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const res = await fetch(url, {
      signal: controller.signal,
      headers: { 'Cache-Control': 'no-store' },
    });
    if (!res.ok) {
      return null;
    }
    return (await res.json()) as DashboardSnapshot;
  } catch {
    return null;
  } finally {
    clearTimeout(timeout);
  }
}

async function probeHealth(url: string): Promise<boolean> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const res = await fetch(url, {
      signal: controller.signal,
      headers: { 'Cache-Control': 'no-store' },
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timeout);
  }
}

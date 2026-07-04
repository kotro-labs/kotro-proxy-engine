import * as vscode from 'vscode';
import { spawn, ChildProcess } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import { binaryBasename } from './binary-target';
import { ProxyStatusBar } from './status-bar';
import { addrForEnv } from './listen-url';

let sidecarProcess: ChildProcess | null = null;
let statusBar: ProxyStatusBar | null = null;
const output = vscode.window.createOutputChannel('KortoLabs Proxy Engine');

function extensionConfig() {
  const cfg = vscode.workspace.getConfiguration('kortosystems');
  return {
    profile: cfg.get<string>('profile', 'custom'),
    listenAddr: cfg.get<string>('listenAddr', ':8080'),
    metricsAddr: cfg.get<string>('metricsAddr', '127.0.0.1:9090'),
    upstreamUrl: cfg.get<string>('upstreamUrl', 'https://api.openai.com'),
    cacheDb: cfg.get<string>('cacheDb', ''),
    enableCache: cfg.get<boolean>('enableCache', true),
    enableRedaction: cfg.get<boolean>('enableRedaction', true),
    enableCompression: cfg.get<boolean>('enableCompression', true),
    enableMetrics: cfg.get<boolean>('enableMetrics', true),
  };
}

export function activate(context: vscode.ExtensionContext): void {
  output.appendLine('Initializing native proxy gateway core...');

  const settings = extensionConfig();
  statusBar = new ProxyStatusBar(settings.listenAddr, settings.metricsAddr);
  context.subscriptions.push(statusBar);

  context.subscriptions.push(
    vscode.commands.registerCommand('kortosystems.openDashboard', () => {
      const url = statusBar?.getDashboardUrl() ?? 'http://127.0.0.1:9090/dashboard';
      void vscode.env.openExternal(vscode.Uri.parse(url));
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kortosystems.showProxyOutput', () => {
      output.show(true);
    }),
  );

  const binaryName = binaryBasename(process.platform, process.arch);
  const binaryPath = path.join(context.extensionPath, 'bin', binaryName);

  if (!fs.existsSync(binaryPath)) {
    const msg = `KortoLabs binary missing: ${binaryPath}`;
    output.appendLine(msg);
    void vscode.window.showErrorMessage(msg);
    return;
  }

  const cacheDb =
    settings.cacheDb ||
    path.join(context.globalStorageUri.fsPath, 'kortolabs-cache.db');

  fs.mkdirSync(path.dirname(cacheDb), { recursive: true });

  sidecarProcess = spawn(binaryPath, [], {
    env: {
      ...process.env,
      KORTO_PROFILE: settings.profile === 'custom' ? '' : settings.profile,
      KORTO_LISTEN_ADDR: settings.listenAddr,
      KORTO_METRICS_ADDR: addrForEnv(settings.metricsAddr),
      KORTO_UPSTREAM_URL: settings.upstreamUrl,
      KORTO_CACHE_DB: cacheDb,
      KORTO_ENABLE_CACHE: String(settings.enableCache),
      KORTO_ENABLE_REDACTION: String(settings.enableRedaction),
      KORTO_ENABLE_COMPRESSION: String(settings.enableCompression),
      KORTO_ENABLE_METRICS: String(settings.enableMetrics),
      RUST_LOG: process.env.RUST_LOG ?? 'info',
    },
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  sidecarProcess.stdout?.on('data', (chunk: Buffer) => {
    output.appendLine(`[core] ${chunk.toString().trim()}`);
  });

  sidecarProcess.stderr?.on('data', (chunk: Buffer) => {
    output.appendLine(`[stderr] ${chunk.toString().trim()}`);
  });

  sidecarProcess.on('close', (code) => {
    output.appendLine(`Core engine exited with code ${code ?? 'unknown'}`);
    sidecarProcess = null;
    statusBar?.markStopped();
  });

  sidecarProcess.on('error', (err) => {
    output.appendLine(`Failed to start sidecar: ${err.message}`);
    void vscode.window.showErrorMessage(`KortoLabs proxy failed to start: ${err.message}`);
    statusBar?.markStopped();
  });

  context.subscriptions.push(output);
  context.subscriptions.push({
    dispose: () => deactivate(),
  });

  statusBar.markRunning();

  const port = settings.listenAddr.replace(/^:/, '') || '8080';
  const metricsPort = addrForEnv(settings.metricsAddr).split(':').pop() || '9090';
  void vscode.window.showInformationMessage(
    `KortoLabs Proxy is running on port ${port} (telemetry on ${metricsPort}). Open the dashboard from the status bar.`,
  );
}

export function deactivate(): void {
  output.appendLine('Terminating proxy sidecar process...');
  statusBar?.markStopped();
  if (!sidecarProcess) {
    return;
  }
  sidecarProcess.kill('SIGTERM');
  sidecarProcess = null;
}

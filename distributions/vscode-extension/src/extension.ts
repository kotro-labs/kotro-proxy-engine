import * as vscode from 'vscode';
import { spawn, ChildProcess } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import { ProxyStatusBar } from './status-bar';
import { addrForEnv, listenBaseUrl } from './listen-url';
import { verifyCache } from './verify-cache';
import { ensureBinary } from './downloader';
import { runSetupWizard } from './setup-wizard';

let sidecarProcess: ChildProcess | null = null;
let statusBar: ProxyStatusBar | null = null;
const output = vscode.window.createOutputChannel('Kotro Proxy Engine');

function extensionConfig() {
  const cfg = vscode.workspace.getConfiguration('kotrolabs');
  return {
    profile: cfg.get<string>('profile', 'custom'),
    listenAddr: cfg.get<string>('listenAddr', ':8080'),
    metricsAddr: cfg.get<string>('metricsAddr', '127.0.0.1:9090'),
    upstreamUrl: cfg.get<string>('upstreamUrl', 'https://api.openai.com'),
    bridgeToken: cfg.get<string>('bridgeToken', '').trim(),
    upstreamApiKey: cfg.get<string>('upstreamApiKey', '').trim(),
    cacheDb: cfg.get<string>('cacheDb', ''),
    enableCache: cfg.get<boolean>('enableCache', true),
    enableRedaction: cfg.get<boolean>('enableRedaction', true),
    enableCompression: cfg.get<boolean>('enableCompression', true),
    enableShrink: cfg.get<boolean>('enableShrink', true),
    fallbackUrl: cfg.get<string>('fallbackUrl', ''),
    fallbackModel: cfg.get<string>('fallbackModel', ''),
    enableMetrics: cfg.get<boolean>('enableMetrics', true),
    enableVectorCache: cfg.get<boolean>('enableVectorCache', true),
    enableInjectionScan: cfg.get<boolean>('enableInjectionScan', true),
    injectionBlock: cfg.get<boolean>('injectionBlock', false),
  };
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  output.appendLine('Initializing native proxy gateway core...');

  const settings = extensionConfig();
  statusBar = new ProxyStatusBar(settings.listenAddr, settings.metricsAddr);
  context.subscriptions.push(statusBar);

  context.subscriptions.push(
    vscode.commands.registerCommand('kotro.statusBarMenu', async () => {
      await statusBar?.showMenu();
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.openDashboard', () => {
      const url = statusBar?.getDashboardUrl() ?? 'http://127.0.0.1:9090/dashboard';
      void vscode.env.openExternal(vscode.Uri.parse(url));
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.showProxyOutput', () => {
      output.show(true);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.verifyCache', async () => {
      output.show(true);
      output.appendLine('Running cache verification (2 identical streaming requests)...');

      const result = await verifyCache(settings.listenAddr, {
        context,
        upstreamUrl: settings.upstreamUrl,
        bridgeToken: settings.bridgeToken || undefined,
        upstreamApiKey: settings.upstreamApiKey || undefined,
      });
      output.appendLine(result.detail);

      if (result.ok) {
        const pick = await vscode.window.showInformationMessage(
          `Kotro cache verified: ${result.detail}`,
          'Open Dashboard',
        );
        if (pick === 'Open Dashboard') {
          void vscode.commands.executeCommand('kotrolabs.openDashboard');
        }
        statusBar?.markRunning();
        return;
      }

      const pick = await vscode.window.showWarningMessage(
        `Kotro cache verification failed. ${result.detail}`,
        'Open Dashboard',
        'Show Logs',
      );
      if (pick === 'Open Dashboard') {
        void vscode.commands.executeCommand('kotrolabs.openDashboard');
      } else if (pick === 'Show Logs') {
        output.show(true);
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotro.setupWizard', async () => {
      await runSetupWizard(output);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotro.connectCursor', async () => {
      const pick = await vscode.window.showInformationMessage(
        'Cursor Chat needs an HTTPS tunnel (cloud blocks localhost). Set kotrolabs.bridgeToken + kotrolabs.upstreamApiKey, put the bridge token in Cursor’s API key field, and use the tunnel Base URL. Open the setup guide?',
        'Yes, open setup guide',
        'Use Continue.dev instead',
        'Verify Cache',
      );

      if (pick === 'Yes, open setup guide') {
        void vscode.env.openExternal(
          vscode.Uri.parse(
            'https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/guides/CURSOR-FIRST-RUN.md',
          ),
        );
      } else if (pick === 'Use Continue.dev instead') {
        void vscode.commands.executeCommand('kotro.setupContinue');
      } else if (pick === 'Verify Cache') {
        void vscode.commands.executeCommand('kotrolabs.verifyCache');
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotro.setupContinue', async () => {
      // Thin alias — full consent flow lives in Setup Wizard.
      await runSetupWizard(output);
    }),
  );

  const binary = await ensureBinary(context, output);

  if (!binary) {
    const msg = 'Failed to download, verify, or locate Kotro Labs binary.';
    output.appendLine(msg);
    void vscode.window.showErrorMessage(msg);
    return;
  }

  const cacheDb =
    settings.cacheDb || path.join(context.globalStorageUri.fsPath, 'kotro-cache.db');

  fs.mkdirSync(path.dirname(cacheDb), { recursive: true });

  if (settings.bridgeToken && !settings.upstreamApiKey) {
    output.appendLine(
      'Warning: kotrolabs.bridgeToken is set without kotrolabs.upstreamApiKey — upstream LLM calls will fail with 503 until the provider key is set.',
    );
  }

  const sidecarEnv: NodeJS.ProcessEnv = {
    ...process.env,
    KOTRO_PROFILE: settings.profile === 'custom' ? '' : settings.profile,
    KOTRO_LISTEN_ADDR: settings.listenAddr,
    KOTRO_METRICS_ADDR: addrForEnv(settings.metricsAddr),
    KOTRO_UPSTREAM_URL: settings.upstreamUrl,
    KOTRO_CACHE_DB: cacheDb,
    KOTRO_ENABLE_CACHE: String(settings.enableCache),
    KOTRO_ENABLE_REDACTION: String(settings.enableRedaction),
    KOTRO_ENABLE_COMPRESSION: String(settings.enableCompression),
    KOTRO_ENABLE_SHRINK: String(settings.enableShrink),
    KOTRO_FALLBACK_URL: settings.fallbackUrl,
    KOTRO_FALLBACK_MODEL: settings.fallbackModel,
    KOTRO_ENABLE_METRICS: String(settings.enableMetrics),
    KOTRO_ENABLE_VECTOR_CACHE: String(settings.enableVectorCache),
    KOTRO_ENABLE_INJECTION_SCAN: String(settings.enableInjectionScan),
    KOTRO_INJECTION_BLOCK: String(settings.injectionBlock),
    RUST_LOG: process.env.RUST_LOG ?? 'info',
  };
  if (settings.bridgeToken) {
    sidecarEnv.KOTRO_BRIDGE_TOKEN = settings.bridgeToken;
  }
  if (settings.upstreamApiKey) {
    sidecarEnv.KOTRO_UPSTREAM_API_KEY = settings.upstreamApiKey;
  }

  sidecarProcess = spawn(binary.path, [], {
    env: sidecarEnv,
    stdio: ['ignore', 'pipe', 'pipe'],
  });

  let sawAddrInUse = false;
  const GUIDE_PORT =
    'https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/guides/CURSOR-FIRST-RUN.md#5-port-already-in-use-kotro-offline';

  sidecarProcess.stdout?.on('data', (chunk: Buffer) => {
    output.appendLine(`[core] ${chunk.toString().trim()}`);
  });

  sidecarProcess.stderr?.on('data', (chunk: Buffer) => {
    const text = chunk.toString().trim();
    output.appendLine(`[stderr] ${text}`);
    if (/AddrInUse|Address already in use/i.test(text)) {
      sawAddrInUse = true;
    }
  });

  sidecarProcess.on('close', (code) => {
    output.appendLine(`Core engine exited with code ${code ?? 'unknown'}`);
    sidecarProcess = null;
    statusBar?.markStopped();
    if (sawAddrInUse) {
      const listen = settings.listenAddr || ':8080';
      void vscode.window
        .showErrorMessage(
          `Kotro could not bind ${listen} (Address already in use). Free that port or change kotrolabs.listenAddr, then reload the window.`,
          'Open fix guide',
          'Show Proxy Logs',
        )
        .then((choice) => {
          if (choice === 'Open fix guide') {
            void vscode.env.openExternal(vscode.Uri.parse(GUIDE_PORT));
          } else if (choice === 'Show Proxy Logs') {
            output.show(true);
          }
        });
    }
  });

  sidecarProcess.on('error', (err) => {
    output.appendLine(`Failed to start sidecar: ${err.message}`);
    void vscode.window.showErrorMessage(`Kotro Labs proxy failed to start: ${err.message}`);
    statusBar?.markStopped();
  });

  context.subscriptions.push(output);
  context.subscriptions.push({
    dispose: () => deactivate(),
  });

  statusBar.markRunning();

  const proxyBase = `${listenBaseUrl(settings.listenAddr)}/v1`;
  const runningMsg = binary.freshlyDownloaded
    ? `Binary installed and verified. Kotro proxy is running at ${proxyBase}.`
    : `Kotro proxy is running at ${proxyBase}.`;

  void vscode.window
    .showInformationMessage(
      `${runningMsg} Run Setup Wizard to configure Cline / Continue.dev?`,
      'Run Wizard',
      'Later',
      'Verify Cache',
      'Open Dashboard',
    )
    .then((pick) => {
      if (pick === 'Run Wizard') {
        void vscode.commands.executeCommand('kotro.setupWizard');
      } else if (pick === 'Verify Cache') {
        void vscode.commands.executeCommand('kotrolabs.verifyCache');
      } else if (pick === 'Open Dashboard') {
        void vscode.commands.executeCommand('kotrolabs.openDashboard');
      }
    });
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

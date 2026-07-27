import * as vscode from 'vscode';
import { spawn, ChildProcess } from 'child_process';
import * as path from 'path';
import * as fs from 'fs';
import * as os from 'os';
import { ProxyStatusBar } from './status-bar';
import { addrForEnv, listenBaseUrl } from './listen-url';
import { verifyCache } from './verify-cache';
import { ensureBinary } from './downloader';
import { runSetupWizard } from './setup-wizard';

let sidecarProcess: ChildProcess | null = null;
let statusBar: ProxyStatusBar | null = null;
const output = vscode.window.createOutputChannel('Kotro Proxy Engine');

/**
 * Mutating control endpoints (kill switch, approvals) require the local
 * control token, which the proxy writes to `<state dir>/control.token`.
 */
function readControlToken(): string | null {
  const stateDir = process.env.KOTRO_STATE_DIR || path.join(os.homedir(), '.kotro');
  try {
    const token = fs.readFileSync(path.join(stateDir, 'control.token'), 'utf8').trim();
    return token.length > 0 ? token : null;
  } catch {
    return null;
  }
}

/** Metrics/control API base URL from the configured metrics address. */
function controlApiBase(metricsAddr: string): string {
  const addr = metricsAddr || '127.0.0.1:9090';
  const host = addr.startsWith(':') ? `127.0.0.1${addr}` : addr;
  return `http://${host}`;
}

interface PendingApproval {
  server: string;
  tool: string;
  args_hash: string;
  session: string;
  reason: string;
  at: string;
}

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
    enableVectorCache: cfg.get<boolean>('enableVectorCache', false),
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
          'Open Flight Recorder',
          'Open Dashboard',
        );
        if (pick === 'Open Flight Recorder') {
          void vscode.commands.executeCommand('kotrolabs.openFlightRecorder');
        } else if (pick === 'Open Dashboard') {
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

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.openFlightRecorder', () => {
      const base = statusBar?.getDashboardUrl() ?? 'http://127.0.0.1:9090/dashboard';
      const url = base.includes('#') ? base : `${base}#flight-recorder`;
      void vscode.env.openExternal(vscode.Uri.parse(url));
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.toggleKillSwitch', async () => {
      const metricsAddr = settings.metricsAddr || '127.0.0.1:9090';
      const host = metricsAddr.startsWith(':') ? `127.0.0.1${metricsAddr}` : metricsAddr;
      const api = `http://${host}`;
      try {
        const cur = await fetch(`${api}/api/flight-recorder`);
        const data = (await cur.json()) as { kill_switch_engaged?: boolean };
        const next = !data.kill_switch_engaged;
        const token = readControlToken();
        if (!token) {
          void vscode.window.showErrorMessage(
            'Kotro control token not found (~/.kotro/control.token). Start the proxy once to generate it.',
          );
          return;
        }
        const resp = await fetch(`${api}/api/kill-switch`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            'x-kotro-control-token': token,
          },
          body: JSON.stringify({ engaged: next, scope: 'all' }),
        });
        if (!resp.ok) {
          throw new Error(`control API returned ${resp.status}`);
        }
        void vscode.window.showInformationMessage(
          next
            ? 'Kotro kill switch ENGAGED — upstream LLM forwards halted.'
            : 'Kotro kill switch cleared — traffic allowed again.',
        );
        statusBar?.markRunning();
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Kill switch failed (is the proxy running?): ${err instanceof Error ? err.message : String(err)}`,
        );
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.reviewApprovals', async () => {
      const api = controlApiBase(settings.metricsAddr);
      let pending: PendingApproval[] = [];
      try {
        const resp = await fetch(`${api}/api/approvals/pending`);
        if (!resp.ok) throw new Error(`control API returned ${resp.status}`);
        pending = ((await resp.json()) as { pending?: PendingApproval[] }).pending ?? [];
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Could not fetch pending approvals (is the proxy running?): ${err instanceof Error ? err.message : String(err)}`,
        );
        return;
      }
      if (pending.length === 0) {
        void vscode.window.showInformationMessage('Kotro: no tool actions are waiting for approval.');
        return;
      }
      const picks = pending.map((p) => ({
        label: `$(shield) ${p.tool}`,
        description: `${p.server} · ${p.session}`,
        detail: p.reason,
        approval: p,
      }));
      const chosen = await vscode.window.showQuickPick(picks, {
        title: 'Kotro — pending tool approvals',
        placeHolder: 'Select a blocked action to approve (deny = leave it pending)',
        matchOnDetail: true,
      });
      if (!chosen) return;

      const ttlPick = await vscode.window.showQuickPick(
        [
          { label: 'Approve for 5 minutes', ttl: 300 },
          { label: 'Approve for 30 minutes', ttl: 1800 },
          { label: 'Approve for 1 hour', ttl: 3600 },
        ],
        { title: `Approve ${chosen.approval.tool}?`, placeHolder: 'Grant duration' },
      );
      if (!ttlPick) return;

      const token = readControlToken();
      if (!token) {
        void vscode.window.showErrorMessage(
          'Kotro control token not found (~/.kotro/control.token). Start the proxy once to generate it.',
        );
        return;
      }
      try {
        const resp = await fetch(`${api}/api/approvals`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json', 'x-kotro-control-token': token },
          body: JSON.stringify({
            server: chosen.approval.server,
            tool: chosen.approval.tool,
            args_hash: chosen.approval.args_hash,
            session: chosen.approval.session,
            ttl_secs: ttlPick.ttl,
          }),
        });
        if (!resp.ok) throw new Error(`control API returned ${resp.status}`);
        void vscode.window.showInformationMessage(
          `Kotro approved '${chosen.approval.tool}' for ${ttlPick.ttl / 60} min. Re-run the action.`,
        );
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Approval failed: ${err instanceof Error ? err.message : String(err)}`,
        );
      }
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('kotrolabs.showIncidents', async () => {
      const api = controlApiBase(settings.metricsAddr);
      interface SessionRow { session: string; events: number; chain_alerts: number; labels: string[]; }
      let sessions: SessionRow[] = [];
      try {
        const resp = await fetch(`${api}/api/session-graph`);
        if (!resp.ok) throw new Error(`control API returned ${resp.status}`);
        sessions = ((await resp.json()) as { sessions?: SessionRow[] }).sessions ?? [];
      } catch (err) {
        void vscode.window.showErrorMessage(
          `Could not fetch session graph (is the proxy running?): ${err instanceof Error ? err.message : String(err)}`,
        );
        return;
      }
      const flagged = sessions.filter((s) => s.chain_alerts > 0);
      if (flagged.length === 0) {
        void vscode.window.showInformationMessage('Kotro: no cross-plane incidents detected.');
        return;
      }
      const chosen = await vscode.window.showQuickPick(
        flagged.map((s) => ({
          label: `$(alert) ${s.session}`,
          description: `${s.chain_alerts} chain alert(s) · ${s.events} events`,
          detail: s.labels.join(', '),
          session: s.session,
        })),
        { title: 'Kotro — cross-plane incidents', placeHolder: 'Open a flagged session' },
      );
      if (!chosen) return;

      let detail = '';
      try {
        const resp = await fetch(`${api}/api/session-graph?session=${encodeURIComponent(chosen.session)}`);
        const g = (await resp.json()) as { events?: Array<{ kind: string; detail: string }> };
        detail = (g.events ?? [])
          .filter((e) => e.kind === 'chain_alert')
          .map((e) => `• ${e.detail}`)
          .join('\n');
      } catch {
        // fall through to actions with empty detail
      }
      const pick = await vscode.window.showWarningMessage(
        `Kotro incident in ${chosen.session}:\n${detail || 'chain alert recorded'}`,
        { modal: true },
        'Engage Tools Kill Switch',
        'Export Incident Bundle',
      );
      if (pick === 'Engage Tools Kill Switch') {
        const token = readControlToken();
        if (!token) {
          void vscode.window.showErrorMessage('Kotro control token not found (~/.kotro/control.token).');
          return;
        }
        try {
          const resp = await fetch(`${api}/api/kill-switch`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', 'x-kotro-control-token': token },
            body: JSON.stringify({ engaged: true, scope: 'tools' }),
          });
          if (!resp.ok) throw new Error(`control API returned ${resp.status}`);
          void vscode.window.showInformationMessage('Kotro tools kill switch ENGAGED.');
        } catch (err) {
          void vscode.window.showErrorMessage(
            `Kill switch failed: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      } else if (pick === 'Export Incident Bundle') {
        void vscode.env.openExternal(
          vscode.Uri.parse(`${api}/api/flight-recorder/export?session=${encodeURIComponent(chosen.session)}`),
        );
      }
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

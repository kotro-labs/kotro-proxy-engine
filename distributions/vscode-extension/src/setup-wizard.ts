import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

const PROXY_URL = 'http://localhost:8080/v1';

/**
 * User-initiated Setup Wizard. Never call from activate() — consent must be
 * explicit (button / command palette), not a silent permissions grab.
 *
 * Touches (only after confirmation):
 * - VS Code global setting: cline.openaiBaseUrl (if Cline is installed)
 * - ~/.continue/config.json (if Continue.dev config exists)
 * - Cursor: guided instructions only (settings live outside VS Code API)
 */
export async function runSetupWizard(output: vscode.OutputChannel): Promise<void> {
  const homeDir = process.env.HOME || process.env.USERPROFILE || '';
  const continuePath = homeDir ? path.join(homeDir, '.continue', 'config.json') : '';
  const continueExists = continuePath !== '' && fs.existsSync(continuePath);

  const summary = [
    'Kotro Setup Wizard will only change settings you approve.',
    '',
    'Possible changes:',
    `• Cline: set cline.openaiBaseUrl → ${PROXY_URL} (VS Code user settings)`,
    continueExists
      ? `• Continue.dev: add "Kotro Local Proxy" model to ${continuePath}`
      : '• Continue.dev: skipped (no ~/.continue/config.json found)',
    '• Cursor: opens a short BYOK Base URL guide (no automatic file edits)',
    '',
    'Nothing is changed until you click Confirm.',
  ].join('\n');

  const pick = await vscode.window.showInformationMessage(
    summary,
    { modal: true },
    'Confirm',
    'Cancel',
  );
  if (pick !== 'Confirm') {
    output.appendLine('Setup Wizard cancelled by user.');
    return;
  }

  output.appendLine('Running Setup Wizard…');

  // Cline
  try {
    const config = vscode.workspace.getConfiguration();
    await config.update('cline.openaiBaseUrl', PROXY_URL, vscode.ConfigurationTarget.Global);
    output.appendLine(`  - Set cline.openaiBaseUrl = ${PROXY_URL}`);
  } catch (e: unknown) {
    const message = e instanceof Error ? e.message : String(e);
    output.appendLine(`  - Cline setting skipped/failed: ${message}`);
  }

  // Continue.dev
  if (continueExists) {
    try {
      const content = fs.readFileSync(continuePath, 'utf8');
      const continueConfig = JSON.parse(content);
      if (!continueConfig.models) {
        continueConfig.models = [];
      }
      const existing = continueConfig.models.find(
        (m: { title?: string }) => m.title === 'Kotro Local Proxy',
      );
      if (!existing) {
        continueConfig.models.unshift({
          title: 'Kotro Local Proxy',
          provider: 'openai',
          model: 'gpt-4o',
          apiKey: 'KOTRO_PROXY_KEY',
          apiBase: PROXY_URL,
        });
        fs.writeFileSync(continuePath, JSON.stringify(continueConfig, null, 2));
        output.appendLine(`  - Updated ${continuePath}`);
      } else {
        output.appendLine('  - Continue.dev already has Kotro Local Proxy');
      }
    } catch (e: unknown) {
      const message = e instanceof Error ? e.message : String(e);
      output.appendLine(`  - Continue.dev update failed: ${message}`);
      void vscode.window.showErrorMessage(`Continue.dev config update failed: ${message}`);
    }
  }

  const cursor = await vscode.window.showInformationMessage(
    'Cline/Continue (if present) are configured. Configure Cursor BYOK Base URL next?',
    'Cursor guide',
    'Done',
  );
  if (cursor === 'Cursor guide') {
    void vscode.commands.executeCommand('kotro.connectCursor');
  } else {
    void vscode.window.showInformationMessage(
      `Setup complete. Point Cursor at ${PROXY_URL} (Override Base URL ON, named model — not Auto), then run "Kotro: Verify Cache".`,
      'Open setup guide',
    ).then((choice) => {
      if (choice === 'Open setup guide') {
        void vscode.env.openExternal(
          vscode.Uri.parse(
            'https://github.com/kotro-labs/kotro-proxy-engine/blob/main/docs/guides/CURSOR-FIRST-RUN.md',
          ),
        );
      }
    });
  }
}

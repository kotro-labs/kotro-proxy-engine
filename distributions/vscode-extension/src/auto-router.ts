import * as vscode from 'vscode';
import * as path from 'path';
import * as fs from 'fs';

export async function autoConfigureAgents(context: vscode.ExtensionContext, output: vscode.OutputChannel) {
    const hasConfigured = context.globalState.get<boolean>('kotroAutoConfigured', false);
    if (!hasConfigured) {
        const pick = await vscode.window.showInformationMessage(
            'Would you like Kotro to automatically configure Cline and Continue.dev to use the local proxy?',
            'Yes, configure them',
            'No, ask later',
            'Never'
        );

        if (pick === 'Never') {
            await context.globalState.update('kotroAutoConfigured', true);
            return;
        } else if (pick === 'Yes, configure them') {
            await context.globalState.update('kotroAutoConfigured', true);
            
            output.appendLine('Auto-configuring known AI extensions to use Kotro Proxy...');
            const config = vscode.workspace.getConfiguration();
            const proxyUrl = 'http://localhost:8080/v1';

            // 1. Cline configuration
            try {
                await config.update('cline.openaiBaseUrl', proxyUrl, vscode.ConfigurationTarget.Global);
                output.appendLine('  - Successfully configured Cline routing.');
            } catch (e: any) {
                output.appendLine(`  - Failed to configure Cline: ${e.message}`);
            }

            // 2. Continue.dev configuration
            try {
                const homeDir = process.env.HOME || process.env.USERPROFILE;
                if (homeDir) {
                    const configPath = path.join(homeDir, '.continue', 'config.json');
                    if (fs.existsSync(configPath)) {
                        const content = fs.readFileSync(configPath, 'utf8');
                        const continueConfig = JSON.parse(content);
                        if (!continueConfig.models) {
                            continueConfig.models = [];
                        }
                        
                        const existing = continueConfig.models.find((m: any) => m.title === 'Kotro Local Proxy');
                        if (!existing) {
                            continueConfig.models.unshift({
                                title: "Kotro Local Proxy",
                                provider: "openai",
                                model: "gpt-4o",
                                apiKey: "KOTRO_PROXY_KEY",
                                apiBase: proxyUrl
                            });
                            fs.writeFileSync(configPath, JSON.stringify(continueConfig, null, 2));
                            output.appendLine('  - Successfully injected Kotro into Continue.dev config.json.');
                        } else {
                            output.appendLine('  - Continue.dev already configured.');
                        }
                    } else {
                        output.appendLine('  - Continue.dev config not found, skipping.');
                    }
                }
            } catch (e: any) {
                output.appendLine(`  - Failed to configure Continue.dev: ${e.message}`);
            }
        }
    }

    // 3. GitHub Copilot detection and notification (do NOT silently modify)
    const copilot = vscode.extensions.getExtension('GitHub.copilot');
    if (copilot) {
        const hasNotifiedCopilot = context.globalState.get<boolean>('kotroNotifiedCopilot', false);
        if (!hasNotifiedCopilot) {
            void vscode.window.showInformationMessage('GitHub Copilot detected. Kotro does not modify Copilot settings automatically. To route Copilot through Kotro, set "github.copilot.advanced": { "debug.overrideProxyUrl": "http://localhost:8080/v1" }');
            await context.globalState.update('kotroNotifiedCopilot', true);
        }
    }
}

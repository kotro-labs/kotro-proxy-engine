import * as vscode from 'vscode';
import * as https from 'https';
import * as fs from 'fs';
import * as path from 'path';
import { binaryBasename } from './binary-target';

export async function ensureBinary(context: vscode.ExtensionContext, output: vscode.OutputChannel): Promise<string | null> {
    const globalStorage = context.globalStorageUri.fsPath;
    if (!fs.existsSync(globalStorage)) {
        fs.mkdirSync(globalStorage, { recursive: true });
    }

    const binName = binaryBasename(process.platform, process.arch);
    const binPath = path.join(globalStorage, binName);

    // If binary already exists, we skip downloading to avoid startup delay.
    // In a real production scenario we could check version, but this is a POC.
    if (fs.existsSync(binPath)) {
        output.appendLine(`Found existing binary at ${binPath}`);
        return binPath;
    }

    output.appendLine(`Downloading latest Kotro binary (${binName}) from GitHub...`);
    const pick = await vscode.window.showInformationMessage(`Kotro needs to download the proxy binary (~15MB) from GitHub. Proceed?`, 'Proceed', 'Cancel');
    if (pick !== 'Proceed') {
        output.appendLine('User cancelled binary download.');
        return null;
    }

    try {
        const downloadUrl = await getLatestReleaseAssetUrl(binName);
        if (!downloadUrl) {
            throw new Error(`Asset ${binName} not found in latest release.`);
        }
        
        await downloadFile(downloadUrl, binPath, output);
        
        // Make executable (chmod +x)
        if (process.platform !== 'win32') {
            fs.chmodSync(binPath, '755');
        }

        output.appendLine(`Successfully downloaded binary to ${binPath}`);
        return binPath;
    } catch (e: any) {
        output.appendLine(`Failed to download binary: ${e.message}`);
        void vscode.window.showErrorMessage(`Failed to download Kotro binary: ${e.message}`);
        return null;
    }
}

function getLatestReleaseAssetUrl(assetName: string): Promise<string | null> {
    return new Promise((resolve, reject) => {
        const options = {
            hostname: 'api.github.com',
            path: '/repos/kotro-labs/kotro-proxy-engine/releases/latest',
            headers: { 'User-Agent': 'Kotro-VSCode-Extension' }
        };

        https.get(options, (res) => {
            if (res.statusCode === 301 || res.statusCode === 302) {
                // handle redirect if necessary, though api.github.com usually returns 200
            }
            if (res.statusCode !== 200) {
                reject(new Error(`GitHub API returned ${res.statusCode}`));
                return;
            }

            let data = '';
            res.on('data', chunk => data += chunk);
            res.on('end', () => {
                try {
                    const json = JSON.parse(data);
                    const asset = json.assets?.find((a: any) => a.name === assetName);
                    resolve(asset ? asset.browser_download_url : null);
                } catch (e) {
                    reject(e);
                }
            });
        }).on('error', reject);
    });
}

function downloadFile(url: string, dest: string, output: vscode.OutputChannel): Promise<void> {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(dest);

        function doDownload(downloadUrl: string) {
            https.get(downloadUrl, { headers: { 'User-Agent': 'Kotro-VSCode-Extension' } }, (res) => {
                if (res.statusCode === 301 || res.statusCode === 302) {
                    if (res.headers.location) {
                        return doDownload(res.headers.location);
                    }
                }
                if (res.statusCode !== 200) {
                    fs.unlink(dest, () => reject(new Error(`Download failed with status ${res.statusCode}`)));
                    return;
                }

                res.pipe(file);
                file.on('finish', () => {
                    file.close();
                    resolve();
                });
            }).on('error', err => {
                fs.unlink(dest, () => reject(err));
            });
        }
        
        doDownload(url);
    });
}

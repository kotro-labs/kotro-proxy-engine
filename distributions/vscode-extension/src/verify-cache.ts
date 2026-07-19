import * as vscode from 'vscode';
import { listenBaseUrl } from './listen-url';

/** Built into the proxy — never forwarded upstream; no API key required. */
export const LOCAL_VERIFY_MODEL = 'kotro-local-verify';

const SECRET_KEY = 'kotrolabs.verifyApiKey';
const STORE_SETTLE_MS = 200;

type CacheLabel = 'hit' | 'miss' | 'bypass' | 'error';

export type VerifyResult = {
  first: CacheLabel;
  second: CacheLabel;
  ok: boolean;
  detail: string;
};

type PostOnceResult = {
  label: CacheLabel;
  body: string;
  status: number;
};

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function classifyBodyError(body: string): string | undefined {
  const lower = body.toLowerCase();
  if (lower.includes('circuit breaker')) {
    return 'Circuit breaker tripped for this prompt (repeat failed upstream calls). Reload the window or retry — Verify Cache now uses a unique prompt each run.';
  }
  if (
    lower.includes("didn't provide an api key") ||
    lower.includes('invalid api key') ||
    lower.includes('incorrect api key') ||
    lower.includes('authentication') ||
    lower.includes('unauthorized')
  ) {
    return 'Upstream rejected the request (missing/invalid API key). Verify Cache normally uses model kotro-local-verify (no key). If you are on an older proxy binary, set a provider key when prompted, match kotrolabs.upstreamUrl, then retry.';
  }
  if (lower.includes('model') && (lower.includes('not found') || lower.includes('does not exist'))) {
    return 'Upstream does not recognize the model. For keyless verify you need proxy ≥ the build that supports kotro-local-verify, or pass a real provider model + API key.';
  }
  return undefined;
}

async function postOnce(
  url: string,
  model: string,
  prompt: string,
  apiKey?: string,
): Promise<PostOnceResult> {
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (apiKey) {
    headers.Authorization = `Bearer ${apiKey}`;
  }

  const res = await fetch(url, {
    method: 'POST',
    headers,
    body: JSON.stringify({
      model,
      stream: true,
      messages: [{ role: 'user', content: prompt }],
    }),
  });

  const body = await res.text().catch(() => '');
  if (!res.ok) {
    const hint = classifyBodyError(body);
    throw new Error(
      hint ?? `HTTP ${res.status}${body ? `: ${body.slice(0, 160)}` : ''}`,
    );
  }

  const bodyErr = classifyBodyError(body);
  if (bodyErr) {
    throw new Error(bodyErr);
  }

  const cache = res.headers.get('x-kotro-cache')?.toLowerCase();
  if (cache === 'hit') {
    return { label: 'hit', body, status: res.status };
  }
  if (cache === 'bypass') {
    return { label: 'bypass', body, status: res.status };
  }
  return { label: 'miss', body, status: res.status };
}

async function resolveApiKey(
  context: vscode.ExtensionContext | undefined,
): Promise<string | undefined> {
  const fromEnv =
    process.env.OPENAI_API_KEY ||
    process.env.DEEPSEEK_API_KEY ||
    process.env.ANTHROPIC_API_KEY;
  if (fromEnv && fromEnv.trim()) {
    return fromEnv.trim();
  }

  if (!context) {
    return undefined;
  }

  const stored = await context.secrets.get(SECRET_KEY);
  if (stored) {
    return stored;
  }

  const entered = await vscode.window.showInputBox({
    title: 'Kotro Verify Cache — provider API key',
    prompt:
      'Optional fallback when kotro-local-verify is unavailable. Key is stored in Cursor Secret Storage and sent only to localhost Kotro.',
    password: true,
    ignoreFocusOut: true,
    placeHolder: 'sk-… (leave empty to cancel fallback)',
  });
  if (!entered || !entered.trim()) {
    return undefined;
  }
  await context.secrets.store(SECRET_KEY, entered.trim());
  return entered.trim();
}

function fallbackModel(upstreamUrl: string): string {
  if (/deepseek/i.test(upstreamUrl)) {
    return 'deepseek-chat';
  }
  return 'gpt-4o';
}

export async function verifyCache(
  listenAddr: string,
  options?: {
    context?: vscode.ExtensionContext;
    upstreamUrl?: string;
  },
): Promise<VerifyResult> {
  const url = `${listenBaseUrl(listenAddr)}/v1/chat/completions`;
  const prompt = `[kotro-verify] ${Date.now()} What is a semantic cache in one sentence?`;
  const upstreamUrl = options?.upstreamUrl ?? 'https://api.openai.com';

  // 1) Prefer keyless local fixture (proxy supports kotro-local-verify).
  try {
    const firstLocal = await postOnce(url, LOCAL_VERIFY_MODEL, prompt);
    await sleep(STORE_SETTLE_MS);
    const secondLocal = await postOnce(url, LOCAL_VERIFY_MODEL, prompt);

    if (firstLocal.label === 'bypass' || secondLocal.label === 'bypass') {
      return {
        first: firstLocal.label,
        second: secondLocal.label,
        ok: false,
        detail:
          'Cache is bypassed (check kotrolabs.enableCache and use stream:true). ' +
          `Got ${firstLocal.label.toUpperCase()} then ${secondLocal.label.toUpperCase()}.`,
      };
    }

    if (secondLocal.label === 'hit') {
      return {
        first: firstLocal.label,
        second: secondLocal.label,
        ok: true,
        detail: `Cache works (keyless ${LOCAL_VERIFY_MODEL}): ${firstLocal.label.toUpperCase()} then HIT.`,
      };
    }
  } catch (err) {
    // Fall through to provider-backed verify when older binaries forward this model upstream.
    const detail = err instanceof Error ? err.message : String(err);
    if (!/api key|circuit breaker|not found|does not exist|unauthorized/i.test(detail)) {
      // Unexpected local failure — still try provider path below.
    }
  }

  // 2) Provider-backed verify (needs API key matching kotrolabs.upstreamUrl).
  const apiKey = await resolveApiKey(options?.context);
  if (!apiKey) {
    return {
      first: 'error',
      second: 'error',
      ok: false,
      detail:
        `Keyless verify did not return HIT (need a proxy build with model ${LOCAL_VERIFY_MODEL}). ` +
        'Provide a provider API key when prompted, or set OPENAI_API_KEY / DEEPSEEK_API_KEY in the environment Cursor was launched from. ' +
        'Also ensure kotrolabs.upstreamUrl matches that provider.',
    };
  }

  const model = fallbackModel(upstreamUrl);
  const providerPrompt = `[kotro-verify] ${Date.now()} provider-backed cache check`;

  let first: CacheLabel;
  try {
    first = (await postOnce(url, model, providerPrompt, apiKey)).label;
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    return {
      first: 'error',
      second: 'error',
      ok: false,
      detail: `Request 1 failed (${model} via ${upstreamUrl}): ${detail}`,
    };
  }

  await sleep(STORE_SETTLE_MS);

  let second: CacheLabel;
  try {
    second = (await postOnce(url, model, providerPrompt, apiKey)).label;
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    return {
      first,
      second: 'error',
      ok: false,
      detail: `Request 1 was ${first.toUpperCase()}, but request 2 failed: ${detail}`,
    };
  }

  if (first === 'bypass' || second === 'bypass') {
    return {
      first,
      second,
      ok: false,
      detail:
        'Cache is bypassed (check kotrolabs.enableCache and use stream:true). ' +
        `Got ${first.toUpperCase()} then ${second.toUpperCase()}.`,
    };
  }

  if (second === 'hit') {
    return {
      first,
      second,
      ok: true,
      detail: `Cache works (${model}): ${first.toUpperCase()} on first request, HIT on second.`,
    };
  }

  return {
    first,
    second,
    ok: false,
    detail:
      `Expected HIT on second request, got ${second.toUpperCase()} (first was ${first.toUpperCase()}). ` +
      'Common causes: upstream error responses are not cached; wrong API key; kotrolabs.upstreamUrl mismatch; or the first stream never completed with [DONE]. Check Kotro: Show Proxy Logs.',
  };
}

// DOMINION account gateway: no Nous credentials and no shared client-side API key.
// Cloudflare Worker + D1. Credits are integer micro-KRW (one KRW = 1,000 units).
const json = (data, status = 200, headers = {}) => new Response(JSON.stringify(data), {
  status, headers: { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store', ...headers }
});
const bytes = text => new TextEncoder().encode(text);
const b64url = data => btoa(String.fromCharCode(...new Uint8Array(data))).replace(/=/g, '').replace(/\+/g, '-').replace(/\//g, '_');
const unb64 = text => Uint8Array.from(atob(text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - text.length % 4) % 4)), c => c.charCodeAt(0));
const hash = async value => b64url(await crypto.subtle.digest('SHA-256', bytes(value)));
const err = (message, status = 400) => json({ error: message }, status);
const now = () => Math.floor(Date.now() / 1000);
const maxBody = async (request, limit = 150000) => {
  if (Number(request.headers.get('content-length') || 0) > limit) throw new Error('Request too large');
  const text = await request.text();
  if (text.length > limit) throw new Error('Request too large');
  return JSON.parse(text);
};
function cors(origin, env) {
  return origin && origin === env.ALLOWED_ORIGIN ? {
    'access-control-allow-origin': origin,
    'access-control-allow-methods': 'POST, GET, OPTIONS',
    'access-control-allow-headers': 'content-type, authorization',
    'access-control-max-age': '600',
    vary: 'Origin'
  } : {};
}
async function verifyGoogle(idToken, env) {
  const parts = idToken.split('.');
  if (parts.length !== 3) throw new Error('Invalid Google token');
  const header = JSON.parse(new TextDecoder().decode(unb64(parts[0])));
  const claims = JSON.parse(new TextDecoder().decode(unb64(parts[1])));
  if (header.alg !== 'RS256' || !header.kid || claims.aud !== env.GOOGLE_CLIENT_ID ||
      !['accounts.google.com', 'https://accounts.google.com'].includes(claims.iss) ||
      !claims.sub || !Number.isSafeInteger(claims.exp) || claims.exp <= now() ||
      !Number.isSafeInteger(claims.iat) || claims.iat > now() + 60) throw new Error('Invalid Google claims');
  const res = await fetch('https://www.googleapis.com/oauth2/v3/certs');
  if (!res.ok) throw new Error('Google keys unavailable');
  const jwk = (await res.json()).keys.find(k => k.kid === header.kid && k.kty === 'RSA' && k.use === 'sig');
  if (!jwk) throw new Error('Unknown Google signing key');
  const key = await crypto.subtle.importKey('jwk', jwk, { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, false, ['verify']);
  if (!await crypto.subtle.verify('RSASSA-PKCS1-v1_5', key, unb64(parts[2]), bytes(parts[0] + '.' + parts[1])))
    throw new Error('Invalid Google signature');
  return claims;
}
async function account(request, env) {
  const token = /^Bearer ([A-Za-z0-9_-]{32,})$/.exec(request.headers.get('authorization') || '')?.[1];
  if (!token) return null;
  return env.DB.prepare('SELECT a.id, a.credits FROM sessions s JOIN accounts a ON a.id=s.account_id WHERE s.token_hash=? AND s.expires_at>?')
    .bind(await hash(token), now()).first();
}
async function login(request, env) {
  const { credential } = await maxBody(request);
  if (typeof credential !== 'string' || credential.length > 10000) return err('Google credential required');
  const google = await verifyGoogle(credential, env);
  const id = 'google:' + google.sub;
  await env.DB.prepare('INSERT INTO accounts (id,email,credits,created_at) VALUES (?,?,0,?) ON CONFLICT(id) DO UPDATE SET email=excluded.email')
    .bind(id, String(google.email || '').slice(0, 320), now()).run();
  const token = b64url(crypto.getRandomValues(new Uint8Array(32)));
  await env.DB.prepare('INSERT INTO sessions (token_hash,account_id,expires_at) VALUES (?,?,?)')
    .bind(await hash(token), id, now() + 7 * 86400).run();
  return json({ token, expires_in: 7 * 86400, account_id: id });
}

const builtinModels = [
  { id: '@cf/meta/llama-3.1-8b-instruct-fast', name: 'Llama 3.1 8B Fast', free: true, backend: 'workers-ai' },
  { id: '@cf/meta/llama-3.2-3b-instruct', name: 'Llama 3.2 3B', free: true, backend: 'workers-ai' }
];
function freeModels(env) {
  let extras = [];
  try {
    const items = JSON.parse(env.FREE_MODELS_JSON || '[]');
    if (Array.isArray(items)) extras = items.filter(x => typeof x.id === 'string' && /^[a-zA-Z0-9._:/-]{1,120}$/.test(x.id));
  } catch { /* Invalid optional catalog must not hide built-in models. */ }
  const upstreamEnabled = Boolean(env.UPSTREAM_API_KEY && env.UPSTREAM_URL);
  const allowed = extras.filter(x => x.backend === 'workers-ai' ? Boolean(env.AI) : upstreamEnabled);
  const all = [...(env.AI ? builtinModels : []), ...allowed];
  return [...new Map(all.map(x => [x.id, x])).values()];
}
async function upstreamFreeCatalog(env) {
  if (!env.UPSTREAM_API_KEY || !env.UPSTREAM_URL || !env.UPSTREAM_FREE_DISCOVERY) return [];
  const url = env.UPSTREAM_URL.replace(/\/$/, '') + '/models';
  try {
    const response = await fetch(url, {
      headers: { authorization: 'Bearer ' + env.UPSTREAM_API_KEY },
      signal: AbortSignal.timeout(12000)
    });
    if (!response.ok) return [];
    const catalog = await response.json();
    if (!Array.isArray(catalog.data)) return [];
    return catalog.data.filter(m => typeof m.id === 'string' && m.id.endsWith(':free') &&
      m.pricing && Number(m.pricing.prompt) === 0 && Number(m.pricing.completion) === 0)
      .slice(0, 300).map(m => ({ id: m.id, name: m.name || m.id, free: true, backend: 'upstream' }));
  } catch { return []; }
}
// Worker-isolate cache reduces catalog traffic. Expired snapshots are retained
// for transient upstream failures; the scheduled handler refreshes proactively.
let catalogCache = { expires: 0, models: [], refreshedAt: 0, upstreamVerifiedAt: 0 };
const modelFailures = new Map();
const catalogTtl = 15 * 60 * 1000;
function modelAvailable(id) {
  const entry = modelFailures.get(id);
  return !entry || entry.until <= Date.now();
}
function markModelResult(id, ok) {
  if (ok) { modelFailures.delete(id); return; }
  const old = modelFailures.get(id);
  const count = (old?.count || 0) + 1;
  modelFailures.set(id, { count, until: count >= 3 ? Date.now() + 5 * 60 * 1000 : 0 });
}
async function availableModels(env, refresh = false) {
  if (!refresh && catalogCache.expires > Date.now()) return catalogCache.models;
  const upstream = await upstreamFreeCatalog(env);
  // Keep the last known catalog when discovery temporarily fails.
  // Never keep a promotional free tier indefinitely: prices may change.
  const staleSafe = Date.now() - catalogCache.upstreamVerifiedAt < 60 * 60 * 1000;
  const previous = upstream.length ? upstream : staleSafe ? catalogCache.models.filter(m => m.backend === 'upstream') : [];
  const models = [...new Map([...freeModels(env), ...previous].map(m => [m.id, m])).values()];
  catalogCache = { models, expires: Date.now() + catalogTtl, refreshedAt: Date.now(), upstreamVerifiedAt: upstream.length ? Date.now() : catalogCache.upstreamVerifiedAt };
  return models;
}
async function freeProxy(request, env, user) {
  const body = await maxBody(request);
  const model = (await availableModels(env)).find(x => x.id === body.model);
  if (!model || (model.backend !== 'workers-ai' && (!env.UPSTREAM_API_KEY || !env.UPSTREAM_URL))) return err('Free model not available', 503);
  if (!modelAvailable(model.id)) return err('Model temporarily unavailable', 503);
  // Chat clients may request streaming and advertise tools by default.
  // Free text models do not execute tools; compatibility mode ignores tool schemas.
  if (body.n !== undefined && body.n !== 1) return err('Only one completion supported');
  if (!Array.isArray(body.messages) || !body.messages.length ||
      JSON.stringify(body.messages).length > 50000 ||
      (body.max_tokens !== undefined && (!Number.isSafeInteger(body.max_tokens) || body.max_tokens < 1)))
    return err('Invalid or oversized messages');
  const limit = Math.max(1, Math.min(1000, Number(env.FREE_DAILY_LIMIT || 20)));
  const day = new Date().toISOString().slice(0, 10);
  const key = user.id + ':' + day;
  const updated = await env.DB.prepare('INSERT INTO free_quota(account_day,used) VALUES(?,1) ON CONFLICT(account_day) DO UPDATE SET used=used+1 WHERE used<? RETURNING used')
    .bind(key, limit).first();
  if (!updated) return err('Daily free quota exhausted', 429);
  if (model.backend === 'workers-ai') {
    try {
      const answer = await env.AI.run(model.id, { messages: body.messages, max_tokens: Math.min(1024, body.max_tokens || 512) });
      markModelResult(model.id, true);
      const content = typeof answer?.response === 'string' ? answer.response : '';
      if (!content) throw Error('Empty model response');
      markModelResult(model.id, true);
      const completionId = crypto.randomUUID();
      const quotaHeaders = { 'x-dominion-free-remaining': String(Math.max(0, limit - updated.used)) };
      if (body.stream === true) {
        const chunk = (delta, finish_reason = null) => ({
          id: completionId, object: 'chat.completion.chunk', created: now(), model: model.id,
          choices: [{ index: 0, delta, finish_reason }]
        });
        const payload = 'data: ' + JSON.stringify(chunk({ role: 'assistant', content })) + '\n\n'
          + 'data: ' + JSON.stringify(chunk({}, 'stop')) + '\n\n'
          + 'data: [DONE]\n\n';
        return new Response(payload, { status: 200, headers: {
          'content-type': 'text/event-stream; charset=utf-8', 'cache-control': 'no-store', ...quotaHeaders
        } });
      }
      return json({ id: completionId, object: 'chat.completion', model: model.id, choices: [{ index: 0, message: { role: 'assistant', content }, finish_reason: 'stop' }], usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 } }, 200, quotaHeaders);
    } catch { markModelResult(model.id, false); await env.DB.prepare('UPDATE free_quota SET used=MAX(0,used-1) WHERE account_day=?').bind(key).run(); return err('Workers AI inference unavailable', 502); }
  }
  const upstream = await fetch(env.UPSTREAM_URL.replace(/\/$/, '') + '/chat/completions', {
    method: 'POST',
    headers: { authorization: 'Bearer ' + env.UPSTREAM_API_KEY, 'content-type': 'application/json' },
    body: JSON.stringify({ model: model.upstream_model || model.id, messages: body.messages, stream: body.stream === true, max_tokens: Math.min(2048, body.max_tokens || 1024) }),
    signal: AbortSignal.timeout(90000)
  });
  if (!upstream.ok) { if (upstream.status === 404 || upstream.status === 429 || upstream.status >= 500) markModelResult(model.id, false); await env.DB.prepare('UPDATE free_quota SET used=MAX(0,used-1) WHERE account_day=?').bind(key).run(); return err('Free model provider unavailable', 502); }
  markModelResult(model.id, true);
  const headers = new Headers({ 'cache-control': 'no-store', 'x-dominion-free-remaining': String(Math.max(0, limit - updated.used)) });
  headers.set('content-type', upstream.headers.get('content-type') || (body.stream ? 'text/event-stream' : 'application/json'));
  return new Response(upstream.body, { status: upstream.status, headers });
}

async function proxy(request, env, user) {
  if (!env.UPSTREAM_API_KEY || !env.UPSTREAM_URL || !env.MODEL_ID ||
      !env.PRICE_INPUT_MICRO_KRW || !env.PRICE_OUTPUT_MICRO_KRW) return err('Model service not configured', 503);
  const body = await maxBody(request);
  if (body.model !== env.MODEL_ID || !Array.isArray(body.messages) || !body.messages.length)
    return err('Unsupported model or messages');
  // Initial release: one configured model, no tools, no streaming. Fail closed.
  if (body.stream || body.tools || body.tool_choice || body.n || body.response_format)
    return err('Unsupported request options');
  const maxTokens = Math.min(4096, Math.max(1, Number(body.max_tokens || 1024)));
  if (!Number.isSafeInteger(maxTokens)) return err('Invalid max_tokens');
  const inputPrice = Number(env.PRICE_INPUT_MICRO_KRW), outputPrice = Number(env.PRICE_OUTPUT_MICRO_KRW);
  if (![inputPrice, outputPrice].every(n => Number.isSafeInteger(n) && n > 0)) return err('Invalid pricing configuration', 503);
  const encoded = JSON.stringify(body.messages);
  if (encoded.length > 100000) return err('Context too large');
  // Conservative upper bound: charge for all input bytes as tokens, plus max output tokens.
  const reserve = encoded.length * inputPrice + maxTokens * outputPrice;
  if (!Number.isSafeInteger(reserve) || reserve > 1000000000) return err('Request exceeds limit');
  const id = crypto.randomUUID();
  const debited = await env.DB.prepare('UPDATE accounts SET credits=credits-? WHERE id=? AND credits>=? RETURNING credits')
    .bind(reserve, user.id, reserve).first();
  if (!debited) return err('Insufficient prepaid credits', 402);
  await env.DB.prepare('INSERT INTO usage(id,account_id,model,reserved,status,created_at) VALUES(?,?,?,?,?,?)')
    .bind(id, user.id, env.MODEL_ID, reserve, 'reserved', now()).run();
  let charge = 0, status = 'failed', result;
  try {
    const upstream = await fetch(env.UPSTREAM_URL.replace(/\/$/, '') + '/chat/completions', {
      method: 'POST',
      headers: { authorization: 'Bearer ' + env.UPSTREAM_API_KEY, 'content-type': 'application/json' },
      body: JSON.stringify({ model: env.MODEL_ID, messages: body.messages, max_tokens: maxTokens, stream: false }),
      signal: AbortSignal.timeout(90000)
    });
    if (!upstream.ok) throw new Error('Upstream unavailable (' + upstream.status + ')');
    result = await upstream.json();
    const input = result.usage?.prompt_tokens, output = result.usage?.completion_tokens;
    if (!Number.isSafeInteger(input) || !Number.isSafeInteger(output) || input < 0 || output < 0)
      throw new Error('Upstream usage unavailable');
    charge = Math.min(reserve, input * inputPrice + output * outputPrice);
    if (!Number.isSafeInteger(charge)) throw new Error('Invalid usage');
    status = 'completed';
  } catch (e) {
    result = { error: 'Model request failed', detail: String(e.message || e) };
  } finally {
    // Reconcile against the initial reservation; never allow a negative credit balance.
    await env.DB.batch([
      env.DB.prepare('UPDATE accounts SET credits=credits+? WHERE id=?').bind(reserve - charge, user.id),
      env.DB.prepare('UPDATE usage SET charged=?,status=? WHERE id=?').bind(charge, status, id)
    ]);
  }
  return json(result, status === 'completed' ? 200 : 502);
}
async function routeCompletion(request, env, user) {
  const raw = await request.clone().text();
  let body;
  try { body = JSON.parse(raw); } catch { return err('Invalid JSON'); }
  const available = await availableModels(env);
  if (available.some(x => x.id === body.model)) return freeProxy(request, env, user);
  if (body.model === 'dominion/free-auto') {
    const candidates = available.filter(x => x.free).slice(0, 5);
    if (!candidates.length) return err('No healthy free models available', 503);
    for (const candidate of candidates) {
      const attempted = new Request(request, { body: JSON.stringify({ ...body, model: candidate.id }) });
      const response = await freeProxy(attempted, env, user);
      if (response.ok || response.status === 429) return response;
    }
    return err('All free models temporarily unavailable', 503);
  }
  return proxy(request, env, user);
}
export default {
  async scheduled(_event, env, ctx) {
    ctx.waitUntil(availableModels(env, true));
  },
  async fetch(request, env) {
    const origin = request.headers.get('origin');
    const headers = cors(origin, env);
    if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers });
    if (origin && origin !== env.ALLOWED_ORIGIN) return err('Origin not allowed', 403);
    if (new URL(request.url).pathname === '/health' && request.method === 'GET') return json({service:'DOMINION Cloud',status:'online',login_configured:Boolean(env.GOOGLE_CLIENT_ID),models_configured:Boolean(freeModels(env).length)});
    if (!env.DB || !env.GOOGLE_CLIENT_ID || !env.ALLOWED_ORIGIN) return err('Service not configured', 503);
    try {
      const path = new URL(request.url).pathname;
      let response;
      if (path === '/v1/auth/google' && request.method === 'POST') response = await login(request, env);
      else if (path === '/v1/models' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? json({ data: [{ id: 'dominion/free-auto', name: 'DOMINION Free Auto', object: 'model', free: true }, ...(await availableModels(env)).map(x => ({ id: x.id, name: x.name || x.id, object: 'model', free: true }))] }) : err('Unauthorized', 401);
      }
      else if (path === '/v1/me' && request.method === 'GET') {
        const user = await account(request, env);
        response = user ? json({ account_id: user.id, credits_micro_krw: user.credits }) : err('Unauthorized', 401);
      } else if (path === '/v1/chat/completions' && request.method === 'POST') {
        const user = await account(request, env);
        response = user ? await routeCompletion(request, env, user) : err('Unauthorized', 401);
      } else response = err('Not found', 404);
      const out = new Response(response.body, response);
      for (const [k, v] of Object.entries(headers)) out.headers.set(k, v);
      return out;
    } catch (e) {
      return json({ error: 'Request rejected', detail: String(e.message || e) }, 400, headers);
    }
  }
};

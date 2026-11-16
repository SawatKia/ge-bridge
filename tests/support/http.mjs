import http from 'node:http';

export async function sse(res) {
  const text = await res.text();
  return text.split(/\r?\n\r?\n/).filter(Boolean).map((blk) => {
    const lines = blk.split(/\r?\n/);
    const ev = lines.find((l) => l.startsWith('event:'))?.slice(6).trim();
    const data = lines.filter((l) => l.startsWith('data:')).map((l) => l.slice(5).trim()).join('\n');
    return { ev, data };
  });
}

export const j = (s) => { try { return JSON.parse(s); } catch { return null; } };
export const user = (content) => ({ role: 'user', content });

export const oaiTools = [{ type: 'function', function: { name: 'get_weather', description: 'weather', parameters: { type: 'object', properties: { city: { type: 'string' } } } } }];
export const anthTools = [{ name: 'get_weather', description: 'weather', input_schema: { type: 'object', properties: { city: { type: 'string' } } } }];

export function client({ base, token }) {
  const auth = { authorization: `Bearer ${token}`, 'content-type': 'application/json' };
  const anthropic = { 'x-api-key': token, 'anthropic-version': '2023-06-01', 'content-type': 'application/json' };
  const post = (path, body, headers = auth) => fetch(base + path, { method: 'POST', headers, body: JSON.stringify(body) });
  const chat = (extra) => post('/v1/chat/completions', { model: 'gemini-test-flash', messages: [user('say hi')], ...extra });
  const messages = (extra) => post('/v1/messages', { model: 'gemini-test-flash', max_tokens: 256, messages: [user('say hi')], ...extra }, anthropic);
  const health = async () => (await fetch(`${base}/api/health`)).json();
  return { auth, anthropic, post, chat, messages, health };
}

export const rawPost = (url, headers) =>
  new Promise((resolve, reject) => {
    const req = http.request(url, { method: 'POST', headers }, (res) => {
      let body = '';
      res.on('data', (c) => (body += c));
      res.on('end', () => resolve({ status: res.statusCode, body }));
    });
    req.on('error', reject);
    req.end();
  });

export const rawGet = (url, headers) =>
  new Promise((resolve, reject) => {
    http.get(url, { headers }, (res) => {
      res.resume();
      res.on('end', () => resolve(res.statusCode));
    }).on('error', reject);
  });

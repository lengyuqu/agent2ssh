// Windows-native probe for the merged `/shutdown` protocol: a request naming
// another process must be refused and must leave the daemon running, while the
// request naming this daemon stops it. `expect_pid: null` is a bad request
// rather than an unchecked stop.

import net from 'node:net';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const REPO = process.argv[2] ?? 'D:/vibercodeing/agent2ssh/src-tauri';
const DAEMON = path.join(REPO, 'target', 'debug', 'agent2ssh-daemon.exe');

const fail = (msg) => { console.error('FAIL: ' + msg); process.exitCode = 1; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); });
  });
}

function request(port, method, urlPath, token, body) {
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
    const req = http.request({ host: '127.0.0.1', port, method, path: urlPath,
      headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(payload ? { 'Content-Type': 'application/json', 'Content-Length': payload.length } : {}) } },
      (res) => { const c = []; res.on('data', (d) => c.push(d)); res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(c).toString() })); });
    req.on('error', reject);
    if (payload) req.write(payload);
    req.end();
  });
}

async function waitFor(fn, timeoutMs, what) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { if (await fn()) return; } catch { /* retry */ }
    await sleep(100);
  }
  throw new Error(`timeout waiting for ${what}`);
}

async function main() {
  if (!fs.existsSync(DAEMON)) throw new Error(`daemon binary not found: ${DAEMON}`);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'a2s-expect-pid-'));
  const token = `expect-pid-probe-${process.pid}`;
  fs.writeFileSync(path.join(dir, 'daemon.token'), token);

  const port = await freePort();
  const stderr = fs.openSync(path.join(dir, 'daemon.stderr'), 'w');
  const child = spawn(DAEMON, [], {
    env: { ...process.env, AGENT2SSH_CONFIG_DIR: dir, AGENT2SSH_DAEMON_ADDR: `127.0.0.1:${port}`, AGENT2SSH_LOG: 'warn' },
    stdio: ['ignore', 'ignore', stderr],
  });
  let exitInfo = null;
  child.on('exit', (code, signal) => { exitInfo = { code, signal, at: Date.now() }; });

  console.log(`config dir : ${dir}`);
  console.log(`daemon pid : ${child.pid}  addr: 127.0.0.1:${port}`);
  await waitFor(async () => {
    const res = await request(port, 'GET', '/health', null);
    return res.status === 200 && JSON.parse(res.body).pid === child.pid;
  }, 30000, 'daemon /health with matching pid');
  console.log('health     : 200, pid matches');

  const stranger = child.pid + 1;
  const refused = await request(port, 'POST', '/shutdown', token, { expect_pid: stranger });
  console.log(`wrong pid  : HTTP ${refused.status} ${refused.body.trim()}`);
  if (refused.status !== 409) fail(`expected 409 for a request naming pid ${stranger}, got ${refused.status}`);
  if (!refused.body.includes(String(child.pid))) fail('refusal does not name the daemon that answered');
  else console.log('refusal    : names the answering daemon pid');

  const nullField = await request(port, 'POST', '/shutdown', token, { expect_pid: null });
  console.log(`null field : HTTP ${nullField.status} ${nullField.body.trim()}`);
  if (nullField.status !== 400) fail(`expected 400 for expect_pid: null, got ${nullField.status}`);

  const health = await request(port, 'GET', '/health', null);
  if (health.status !== 200 || JSON.parse(health.body).pid !== child.pid) fail('daemon did not survive the refusals');
  else console.log('survives   : daemon still serving after the refusals');

  const started = Date.now();
  const accepted = await request(port, 'POST', '/shutdown', token, { expect_pid: child.pid });
  console.log(`right pid  : HTTP ${accepted.status} ${accepted.body.trim()}`);
  if (accepted.status !== 200) fail(`expected 200 for the daemon's own pid, got ${accepted.status}`);

  await waitFor(async () => exitInfo !== null, 20000, 'daemon exit').catch(() => {});
  const elapsed = Date.now() - started;
  if (!exitInfo) fail('daemon did not exit after an accepted shutdown');
  else console.log(`exit       : code=${exitInfo.code} after ${elapsed} ms`);
  if (fs.existsSync(path.join(dir, 'daemon.pid'))) fail('daemon.pid still present after shutdown');
  else console.log('pid file   : removed');

  fs.closeSync(stderr);
  console.log(`RESULT=${process.exitCode ? 'FAIL' : 'PASS'}`);
}

main().catch((err) => { fail(err && err.stack ? err.stack : String(err)); console.log('RESULT=FAIL'); });

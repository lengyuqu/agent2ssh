// Windows-native probe: an interrupted /exec must answer 503 and audit the
// unknown remote outcome. Rewritten for the merged stop protocol: `/shutdown`
// now carries `expect_pid` in a JSON body instead of using `/shutdown/guarded`
// and the `X-Agent2SSH-Expected-Pid` header.

import net from 'node:net';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const REPO = process.argv[2] ?? 'D:/vibercodeing/agent2ssh/src-tauri';
const DAEMON = path.join(REPO, 'target', 'debug', 'agent2ssh-daemon.exe');
const FIXTURE_PORT = Number(process.env.AGENT2SSH_TEST_SSH_PORT ?? 0);
const KEY = process.env.AGENT2SSH_TEST_SSH_KEY ?? '';
const HOST = process.env.AGENT2SSH_TEST_SSH_HOST ?? '127.0.0.1';
const USER = process.env.AGENT2SSH_TEST_SSH_USER ?? 'root';

const fail = (msg) => { console.error('FAIL: ' + msg); process.exitCode = 1; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => resolve(p)); });
  });
}

function request(port, method, urlPath, token, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
    const req = http.request({ host: '127.0.0.1', port, method, path: urlPath,
      headers: { ...(token ? { Authorization: `Bearer ${token}` } : {}),
        ...(payload ? { 'Content-Type': 'application/json', 'Content-Length': payload.length } : {}),
        ...headers } },
      (res) => { const c = []; res.on('data', (d) => c.push(d)); res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(c).toString() })); });
    req.on('error', reject);
    if (payload) req.write(payload);
    req.end();
  });
}

function rawRequest(port, token, body) {
  const socket = net.connect(port, '127.0.0.1');
  let buffer = '';
  socket.setEncoding('utf8');
  socket.on('data', (chunk) => { buffer += chunk; });
  const payload = JSON.stringify(body);
  socket.write(
    `POST /exec HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer ${token}\r\n` +
    `Content-Type: application/json\r\nContent-Length: ${Buffer.byteLength(payload)}\r\n` +
    `Connection: close\r\n\r\n${payload}`,
  );
  return { socket, get response() { return buffer; } };
}

async function waitFor(fn, timeoutMs, what) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try { if (await fn()) return; } catch { /* retry */ }
    await sleep(200);
  }
  throw new Error(`timeout waiting for ${what}`);
}

async function main() {
  if (!fs.existsSync(DAEMON)) throw new Error(`daemon binary not found: ${DAEMON}`);
  if (!FIXTURE_PORT) throw new Error('AGENT2SSH_TEST_SSH_PORT is required');
  if (!KEY) throw new Error('AGENT2SSH_TEST_SSH_KEY is required');

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'a2s-merged-exec-shutdown-'));
  const token = `merged-exec-shutdown-${process.pid}`;
  fs.writeFileSync(path.join(dir, 'daemon.token'), token);
  fs.writeFileSync(path.join(dir, 'hosts.json'), JSON.stringify({
    schema_version: 1,
    hosts: [{ name: 'fixture', host: HOST, port: FIXTURE_PORT, user: USER, key_path: KEY }],
  }));

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

  const marker = `/tmp/a2s-merged-${Date.now()}`;
  const pending = rawRequest(port, token, {
    host: 'fixture',
    command: `printf ready > ${marker}; sleep 30`,
    force: true,
    timeout_secs: 60,
    reason: 'test change',
    side_effect: 'test effect',
    change_id: 'merge-42',
  });
  console.log(`exec       : started (marker ${marker})`);

  await waitFor(async () => {
    const probe = await request(port, 'POST', '/exec', token, {
      host: 'fixture', command: `test -f ${marker}`, force: true, timeout_secs: 3,
    });
    return probe.status === 200 && JSON.parse(probe.body).exit_code === 0;
  }, 20000, 'remote marker');
  console.log('remote     : marker exists, command still running');

  const started = Date.now();
  const shutdown = await request(port, 'POST', '/shutdown', token, { expect_pid: child.pid });
  console.log(`shutdown   : HTTP ${shutdown.status} ${shutdown.body.trim()}`);

  await waitFor(async () => exitInfo !== null, 20000, 'daemon exit');
  const elapsed = Date.now() - started;
  console.log(`exit       : code=${exitInfo.code} after ${elapsed} ms`);
  if (elapsed >= 7000) fail(`stop took ${elapsed} ms, expected < 7000 ms`);

  const response = pending.response;
  if (!response.startsWith('HTTP/1.1 503')) fail(`expected HTTP 503, got: ${response.slice(0, 120)}`);
  else console.log('response   : HTTP 503');
  if (!response.includes('remote command outcome unknown')) fail('503 body does not explain the unknown outcome');
  else console.log('response   : body explains the unknown remote outcome');
  pending.socket.destroy();

  if (fs.existsSync(path.join(dir, 'daemon.pid'))) fail('daemon.pid still present after shutdown');
  else console.log('pid file   : removed');

  const auditRaw = fs.readFileSync(path.join(dir, 'audit.jsonl'), 'utf8');
  const entries = auditRaw.split('\n').filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
  const interrupted = entries.find((e) => e.change_id === 'merge-42');
  if (!interrupted) fail('no audit entry for change_id merge-42');
  else {
    console.log(`audit      : reason=${JSON.stringify(interrupted.reason)}`);
    console.log(`audit      : side_effect=${JSON.stringify(interrupted.side_effect)} change_id=${JSON.stringify(interrupted.change_id)} exit_code=${JSON.stringify(interrupted.exit_code)}`);
    if (!String(interrupted.reason ?? '').includes('remote command outcome unknown')) fail('audit reason lacks the unknown-outcome note');
    if (!String(interrupted.reason ?? '').includes('test change')) fail('audit reason lacks the caller-supplied reason');
    if (interrupted.side_effect !== 'test effect') fail('audit side_effect not preserved');
    if (interrupted.change_id !== 'merge-42') fail('audit change_id not preserved');
  }

  fs.closeSync(stderr);
  console.log(`RESULT=${process.exitCode ? 'FAIL' : 'PASS'}`);
}

main().catch((err) => { fail(err && err.stack ? err.stack : String(err)); console.log('RESULT=FAIL'); });

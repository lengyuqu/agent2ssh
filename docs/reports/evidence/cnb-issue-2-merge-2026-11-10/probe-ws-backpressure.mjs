// Windows-native backpressure probe for the daemon's /exec/stream shutdown path.
//
// Rewritten for the merged stop protocol (`/shutdown` with `expect_pid`).
// The repository test `stop_exits_when_exec_websocket_peer_stops_reading`
// (src-tauri/tests/daemon_stop_e2e.rs) is `#[cfg(unix)]` because it shrinks
// SO_RCVBUF through libc. This harness reproduces the same scenario with only
// Windows-native pieces (node.exe + Loopback TCP + the real agent2ssh-daemon.exe):
// the peer upgrades to /exec/stream, starts a flooding remote command, then
// stops reading and the daemon is asked to shut down.
//
// Difference from the cfg(unix) test, recorded on purpose: Node exposes no
// SO_RCVBUF knob, so the receive window is the Windows default and the socket
// is filled by volume (`yes`) instead of by shrinking the window.

import net from 'node:net';
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';

const REPO = process.argv[2] ?? 'D:/vibercodeing/agent2ssh';
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
    s.listen(0, '127.0.0.1', () => {
      const p = s.address().port;
      s.close(() => resolve(p));
    });
  });
}

function request(port, method, urlPath, token, body, headers = {}) {
  return new Promise((resolve, reject) => {
    const payload = body === undefined ? undefined : Buffer.from(JSON.stringify(body));
    const req = http.request(
      {
        host: '127.0.0.1', port, method, path: urlPath,
        headers: {
          ...(token ? { Authorization: `Bearer ${token}` } : {}),
          ...(payload ? { 'Content-Type': 'application/json', 'Content-Length': payload.length } : {}),
          ...headers,
        },
      },
      (res) => {
        const chunks = [];
        res.on('data', (c) => chunks.push(c));
        res.on('end', () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      },
    );
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

// Reads exactly one server->client WebSocket frame (server frames are unmasked).
function readFrame(socket, bufferRef) {
  return new Promise((resolve, reject) => {
    const tryParse = () => {
      const buf = bufferRef.value;
      if (buf.length < 2) return null;
      const opcode = buf[0] & 0x0f;
      let len = buf[1] & 0x7f;
      let offset = 2;
      if (len === 126) { if (buf.length < 4) return null; len = buf.readUInt16BE(2); offset = 4; }
      else if (len === 127) { if (buf.length < 10) return null; len = Number(buf.readBigUInt64BE(2)); offset = 10; }
      if (buf.length < offset + len) return null;
      bufferRef.value = buf.subarray(offset + len);
      return { opcode, payload: buf.subarray(offset, offset + len) };
    };
    const onData = (chunk) => {
      bufferRef.value = Buffer.concat([bufferRef.value, chunk]);
      const frame = tryParse();
      if (frame) { cleanup(); resolve(frame); }
    };
    const onError = (err) => { cleanup(); reject(err); };
    const cleanup = () => { socket.off('data', onData); socket.off('error', onError); };
    const frame = tryParse();
    if (frame) { resolve(frame); return; }
    socket.on('data', onData);
    socket.on('error', onError);
  });
}

async function main() {
  if (!fs.existsSync(DAEMON)) throw new Error(`daemon binary not found: ${DAEMON}`);
  if (!FIXTURE_PORT) throw new Error('AGENT2SSH_TEST_SSH_PORT is required');
  if (!KEY) throw new Error('AGENT2SSH_TEST_SSH_KEY is required');

  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'a2s-ws-backpressure-'));
  const token = `backpressure-probe-${process.pid}`;
  fs.writeFileSync(path.join(dir, 'daemon.token'), token);
  const hosts = {
    schema_version: 1,
    hosts: [{ name: 'fixture', host: HOST, port: FIXTURE_PORT, user: USER, key_path: KEY }],
  };
  fs.writeFileSync(path.join(dir, 'hosts.json'), JSON.stringify(hosts));

  const port = await freePort();
  const stderr = fs.openSync(path.join(dir, 'daemon.stderr'), 'w');
  const child = spawn(DAEMON, [], {
    env: {
      ...process.env,
      AGENT2SSH_CONFIG_DIR: dir,
      AGENT2SSH_DAEMON_ADDR: `127.0.0.1:${port}`,
      AGENT2SSH_LOG: 'warn',
    },
    stdio: ['ignore', 'ignore', stderr],
  });
  let exited = false;
  let exitInfo = null;
  child.on('exit', (code, signal) => { exited = true; exitInfo = { code, signal, at: Date.now() }; });

  console.log(`config dir : ${dir}`);
  console.log(`daemon pid : ${child.pid}  addr: 127.0.0.1:${port}`);

  await waitFor(async () => {
    const res = await request(port, 'GET', '/health', null);
    return res.status === 200 && JSON.parse(res.body).pid === child.pid;
  }, 30000, 'daemon /health with matching pid');
  console.log('health     : 200, pid matches');

  // --- upgrade to /exec/stream and start a flooding remote command ---
  const socket = net.connect(port, '127.0.0.1');
  await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
  socket.write(
    'GET /exec/stream HTTP/1.1\r\nHost: localhost\r\n' +
    `Authorization: Bearer ${token}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n` +
    'Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n',
  );
  const headerRef = { value: Buffer.alloc(0) };
  const headerFrame = await new Promise((resolve, reject) => {
    const onData = (chunk) => {
      headerRef.value = Buffer.concat([headerRef.value, chunk]);
      const idx = headerRef.value.indexOf('\r\n\r\n');
      if (idx >= 0) {
        const rest = headerRef.value.subarray(idx + 4);
        socket.off('data', onData);
        resolve({ head: headerRef.value.subarray(0, idx).toString(), rest });
      }
    };
    socket.on('data', onData);
    socket.once('error', reject);
  });
  if (!headerFrame.head.startsWith('HTTP/1.1 101')) throw new Error(`upgrade refused: ${headerFrame.head}`);
  console.log('upgrade    : 101 Switching Protocols');

  const message = Buffer.from(JSON.stringify({
    host: 'fixture',
    command: 'yes aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    force: true,
    timeout_secs: 120,
  }));
  const mask = [1, 2, 3, 4];
  const frame = Buffer.concat([
    Buffer.from([0x81, 0x80 | 126, message.length >> 8, message.length & 0xff]),
    Buffer.from(mask),
    Buffer.from(message.map((b, i) => b ^ mask[i % 4])),
  ]);
  socket.write(frame);

  const bufRef = { value: headerFrame.rest };
  const first = await readFrame(socket, bufRef);
  const firstEvent = JSON.parse(first.payload.toString());
  if (firstEvent.type !== 'stdout') throw new Error(`stream did not start: ${first.payload.toString().slice(0, 200)}`);
  console.log(`stream     : first frame type=${firstEvent.type} (${first.payload.length} bytes)`);

  // --- the peer stops reading while the remote command floods output ---
  socket.pause();
  await sleep(3000);
  console.log('peer       : paused 3000 ms without reading');

  const started = Date.now();
  const shutdown = await request(port, 'POST', '/shutdown', token, { expect_pid: child.pid });
  console.log(`shutdown   : HTTP ${shutdown.status} ${shutdown.body.trim()}`);

  await waitFor(async () => exited, 20000, 'daemon exit').catch(() => {});
  const elapsed = Date.now() - started;
  if (!exited) fail(`daemon did not exit within 20 s (elapsed ${elapsed} ms)`);
  else console.log(`exit       : code=${exitInfo.code} signal=${exitInfo.signal} after ${elapsed} ms`);

  if (elapsed >= 7000) fail(`stop took ${elapsed} ms, expected < 7000 ms`);

  const pidFile = path.join(dir, 'daemon.pid');
  if (fs.existsSync(pidFile)) fail('daemon.pid still present after shutdown');
  else console.log('pid file   : removed');

  const appLog = fs.existsSync(path.join(dir, 'app.log')) ? fs.readFileSync(path.join(dir, 'app.log'), 'utf8') : '';
  const drainLine = appLog.split('\n').filter((l) => l.includes('WebSocket drain deadline expired'));
  if (drainLine.length === 0) fail('app.log has no "WebSocket drain deadline expired"');
  else console.log(`app.log    : ${drainLine.length} drain-deadline record(s)`);

  const auditPath = path.join(dir, 'audit.jsonl');
  const auditRaw = fs.existsSync(auditPath) ? fs.readFileSync(auditPath, 'utf8') : '';
  const entries = auditRaw.split('\n').filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).filter(Boolean);
  const interrupted = entries.filter((e) => JSON.stringify(e).includes('daemon shutdown interrupted stream; remote outcome unknown'));
  if (interrupted.length === 0) fail('audit.jsonl has no interrupted-stream record');
  else console.log(`audit      : ${interrupted.length} interrupted-stream record(s): ${JSON.stringify(interrupted[0]).slice(0, 300)}`);

  socket.destroy();
  fs.closeSync(stderr);
  console.log(`RESULT=${process.exitCode ? 'FAIL' : 'PASS'}`);
}

main().catch((err) => { fail(err && err.stack ? err.stack : String(err)); console.log('RESULT=FAIL'); });

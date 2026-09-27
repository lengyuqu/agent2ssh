// Probe: does `pwsh -Command -` execute statements incrementally while stdin
// stays open (streaming), or only after EOF (buffered)? The resident-session
// sentinel protocol depends on the former.
import { spawn } from 'child_process';

function probe(label, args, writeFn) {
  return new Promise((resolve) => {
    const child = spawn(args[0], args.slice(1), { stdio: ['pipe', 'pipe', 'pipe'] });
    let out = '';
    let err = '';
    const t0 = Date.now();
    child.stdout.on('data', (d) => {
      out += d.toString();
      console.log(`[${label}] +${Date.now() - t0}ms OUT: ${JSON.stringify(d.toString())}`);
    });
    child.stderr.on('data', (d) => {
      err += d.toString();
    });
    child.on('close', (code) => {
      console.log(`[${label}] closed code=${code} err=${JSON.stringify(err.slice(0, 300))}`);
      resolve({ out, err });
    });
    writeFn(child);
    setTimeout(() => {
      console.log(`[${label}] TIMEOUT probe, killing. out so far: ${JSON.stringify(out)}`);
      child.kill();
      resolve({ out, err });
    }, 6000);
  });
}

(async () => {
  // 1. pwsh -Command - : write two commands 500ms apart, stdin stays open.
  await probe('pwsh-command-dash', ['pwsh', '-NoLogo', '-NoProfile', '-NonInteractive', '-Command', '-'], (child) => {
    setTimeout(() => child.stdin.write('echo AAA\n'), 300);
    setTimeout(() => child.stdin.write('echo __SENT__\n'), 800);
  });

  // 2. pwsh plain (no -Command): interactive REPL without a console?
  await probe('pwsh-plain', ['pwsh', '-NoLogo', '-NoProfile'], (child) => {
    setTimeout(() => child.stdin.write('echo BBB\n'), 300);
    setTimeout(() => child.stdin.write('echo __SENT__\n'), 800);
  });

  // 3. cmd /D /Q control (expected: streaming per line).
  await probe('cmd-dq', ['cmd', '/D', '/Q'], (child) => {
    setTimeout(() => child.stdin.write('echo CCC\r\n'), 300);
    setTimeout(() => child.stdin.write('echo __SENT__\r\n'), 800);
  });
})();

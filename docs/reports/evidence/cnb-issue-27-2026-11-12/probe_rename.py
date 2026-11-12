#!/usr/bin/env python3
"""Issue #27 — does `replace_file_atomic` survive the three Windows states the
source comment promises to handle?

* a read-only target (`FILE_ATTRIBUTE_READONLY`),
* a target another process holds open without delete sharing,
* a leftover temp file from an earlier run.

Plus the fail-closed question: when `icacls` itself fails, does the caller keep
the old configuration bytes instead of landing an unprotected file?

Each case gets its own config dir and uses synthetic values only.
"""

import ctypes
import ctypes.wintypes as wintypes
import glob
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import threading
import time
import uuid

GENERIC_READ = 0x80000000
GENERIC_WRITE = 0x40000000
FILE_SHARE_READ = 0x1
FILE_SHARE_WRITE = 0x2
FILE_SHARE_DELETE = 0x4
OPEN_EXISTING = 3
INVALID_HANDLE_VALUE = ctypes.c_void_p(-1).value

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
kernel32.CreateFileW.restype = wintypes.HANDLE
kernel32.CreateFileW.argtypes = [
    wintypes.LPCWSTR, wintypes.DWORD, wintypes.DWORD, wintypes.LPVOID,
    wintypes.DWORD, wintypes.DWORD, wintypes.HANDLE,
]
kernel32.CloseHandle.argtypes = [wintypes.HANDLE]

results = []


def cfg_dir(tag):
    path = os.path.join(os.environ.get("TEMP", "."), "a2s27-ren-" + tag + "-" + uuid.uuid4().hex[:6])
    os.makedirs(path, exist_ok=True)
    return path


def env_for(cfg):
    env = os.environ.copy()
    env["AGENT2SSH_CONFIG_DIR"] = cfg
    env["AGENT2SSH_SECRETS_BACKEND"] = "encrypted"
    env["AGENT2SSH_MASTER_PASSWORD"] = "probe-master-only"
    return env


def cli(exe, cfg, args, stdin=b""):
    started = time.time()
    proc = subprocess.run([exe] + args, input=stdin, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, env=env_for(cfg))
    return proc, round((time.time() - started) * 1000, 1)


def seed(exe, cfg):
    proc, _ = cli(exe, cfg, ["host", "add", "seed", "--host", "127.0.0.1",
                             "--password", "SYNTHETIC_SEED_PASSWORD"])
    assert proc.returncode == 0, proc.stderr
    return os.path.join(cfg, "hosts.json")


def open_no_delete(path):
    handle = kernel32.CreateFileW(
        path, GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,  # deliberately no FILE_SHARE_DELETE
        None, OPEN_EXISTING, 0, None)
    if handle == INVALID_HANDLE_VALUE:
        raise ctypes.WinError(ctypes.get_last_error())
    return handle


def open_with_delete(path):
    handle = kernel32.CreateFileW(
        path, GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        None, OPEN_EXISTING, 0, None)
    if handle == INVALID_HANDLE_VALUE:
        raise ctypes.WinError(ctypes.get_last_error())
    return handle


def record(row):
    results.append(row)
    print(json.dumps(row, ensure_ascii=False))


def case_readonly(exe):
    cfg = cfg_dir("readonly")
    hosts = seed(exe, cfg)
    before = open(hosts, "rb").read()
    os.chmod(hosts, stat.S_IREAD)
    readonly_before = not os.access(hosts, os.W_OK)
    proc, ms = cli(exe, cfg, ["host", "add", "second", "--host", "127.0.0.2",
                              "--password", "SYNTHETIC_SECOND_PASSWORD"])
    after = open(hosts, "rb").read()
    record({
        "case": "readonly-target",
        "readonly_before": readonly_before,
        "returncode": proc.returncode,
        "elapsed_ms": ms,
        "config_changed": after != before,
        "readonly_after": not os.access(hosts, os.W_OK),
        "stderr": proc.stderr.decode("utf-8", "replace").strip(),
    })


def case_locked(exe):
    cfg = cfg_dir("locked")
    hosts = seed(exe, cfg)
    before = open(hosts, "rb").read()
    handle = open_no_delete(hosts)
    try:
        proc, ms = cli(exe, cfg, ["host", "add", "blocked", "--host", "127.0.0.3",
                                  "--password", "SYNTHETIC_BLOCKED_PASSWORD"])
        after = open(hosts, "rb").read()
    finally:
        kernel32.CloseHandle(handle)
    record({
        "case": "open-without-delete-share",
        "returncode": proc.returncode,
        "elapsed_ms": ms,
        "config_unchanged": after == before,
        "stderr": proc.stderr.decode("utf-8", "replace").strip(),
        "retries_visible": "after 6 retries" in proc.stderr.decode("utf-8", "replace"),
    })


def case_locked_then_released(exe):
    """The retry budget has to be long enough for a short-lived reader."""
    cfg = cfg_dir("released")
    hosts = seed(exe, cfg)
    handle = open_no_delete(hosts)

    def release():
        time.sleep(0.3)
        kernel32.CloseHandle(handle)

    thread = threading.Thread(target=release)
    thread.start()
    proc, ms = cli(exe, cfg, ["host", "add", "released", "--host", "127.0.0.4",
                              "--password", "SYNTHETIC_RELEASED_PASSWORD"])
    thread.join()
    record({
        "case": "released-after-300ms",
        "returncode": proc.returncode,
        "elapsed_ms": ms,
        "stderr": proc.stderr.decode("utf-8", "replace").strip()[:200],
    })


def case_share_delete(exe):
    """A reader that allows delete — the shape Rust's own `File::open` uses —
    must not block the replace."""
    cfg = cfg_dir("share")
    hosts = seed(exe, cfg)
    handle = open_with_delete(hosts)
    try:
        proc, ms = cli(exe, cfg, ["host", "add", "reader", "--host", "127.0.0.5",
                                  "--password", "SYNTHETIC_READER_PASSWORD"])
    finally:
        kernel32.CloseHandle(handle)
    record({
        "case": "open-with-delete-share",
        "returncode": proc.returncode,
        "elapsed_ms": ms,
        "stderr": proc.stderr.decode("utf-8", "replace").strip(),
    })


def case_leftovers(exe):
    cfg = cfg_dir("leftover")
    seed(exe, cfg)
    # A stale temp file with an unrelated pid is what a crash leaves behind.
    stale = os.path.join(cfg, "hosts.json.tmp.999999")
    with open(stale, "wb") as handle:
        handle.write(b"SYNTHETIC_STALE_TEMP")
    proc, ms = cli(exe, cfg, ["host", "add", "after-stale", "--host", "127.0.0.6",
                              "--password", "SYNTHETIC_AFTER_STALE"])
    leftovers = [os.path.basename(path) for path in glob.glob(os.path.join(cfg, "hosts.json.tmp.*"))]
    record({
        "case": "stale-temp-file",
        "returncode": proc.returncode,
        "elapsed_ms": ms,
        "stale_still_there": os.path.exists(stale),
        "leftovers": leftovers,
        "stderr": proc.stderr.decode("utf-8", "replace").strip()[:200],
    })


def case_icacls_failure(exe):
    """Point PATH at a binary that always exits non-zero; the save must not land."""
    cfg = cfg_dir("icacls")
    seed(exe, cfg)
    hosts = os.path.join(cfg, "hosts.json")
    before = open(hosts, "rb").read()

    fake = os.path.join(os.environ.get("TEMP", "."), "a2s27-fake-icacls-" + uuid.uuid4().hex[:6])
    os.makedirs(fake, exist_ok=True)
    shutil.copy(os.path.join(os.environ.get("SystemRoot", r"C:\Windows"), "System32", "where.exe"),
                os.path.join(fake, "icacls.exe"))

    env = env_for(cfg)
    env["PATH"] = fake + ";" + env.get("PATH", "")
    proc = subprocess.run([exe, "host", "add", "blocked-acl", "--host", "127.0.0.7",
                           "--password", "SYNTHETIC_ACL_FAILURE"],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    after = open(hosts, "rb").read()
    stray = [os.path.basename(path) for path in glob.glob(os.path.join(cfg, ".hosts.json.tmp*"))]
    record({
        "case": "icacls-fails",
        "returncode": proc.returncode,
        "config_unchanged": after == before,
        "stderr": proc.stderr.decode("utf-8", "replace").strip()[:300],
        "temp_files": stray,
    })


def main():
    exe = os.path.abspath(sys.argv[1])
    outdir = os.path.abspath(sys.argv[2]) if len(sys.argv) > 2 else "."
    for case in (case_readonly, case_locked, case_locked_then_released,
                 case_share_delete, case_leftovers, case_icacls_failure):
        try:
            case(exe)
        except Exception as error:  # noqa: BLE001 - a failed case is a result
            record({"case": case.__name__, "error": repr(error)})
    with open(os.path.join(outdir, "raw-rename-probe.json"), "w", encoding="utf-8") as handle:
        json.dump(results, handle, ensure_ascii=False, indent=2)
    print("wrote", os.path.join(outdir, "raw-rename-probe.json"))


if __name__ == "__main__":
    main()

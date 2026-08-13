import { describe, it, expect } from "vitest";
import { detectPrompt } from "./prompt";

describe("detectPrompt", () => {
  it("detects PowerShell prompt", () => {
    expect(detectPrompt("PS C:\\Users\\alice> ")).toEqual({ end: 18 });
  });

  it("detects Unix user@host prompt", () => {
    const match = detectPrompt("alice@prod-web:~$ ");
    expect(match).not.toBeNull();
    expect(match!.end).toBe("alice@prod-web:~$".length);
  });

  it("detects bracket prompt", () => {
    expect(detectPrompt("[root@host /var/log]# ")).not.toBeNull();
  });

  it("detects starship / oh-my-zsh symbolic prompt", () => {
    expect(detectPrompt("~/project ❯ ")).not.toBeNull();
  });

  it("detects versioned bash prompt", () => {
    expect(detectPrompt("bash-5.2$ ")).not.toBeNull();
  });

  it("detects minimal POSIX prompt", () => {
    expect(detectPrompt("$ ")).toEqual({ end: 1 });
  });

  it("does not match ordinary command output", () => {
    expect(detectPrompt("total 123")).toBeNull();
    expect(detectPrompt("error: connection refused")).toBeNull();
  });
});

/**
 * Shell prompt recognition for terminal presentation features.
 *
 * Shell-family regular expressions, anchored to column zero — not OCR, not a
 * second command-block protocol. Callers must provide a constrained boundary
 * candidate (a block's first logical row, or the current logical cursor row
 * while the tracker is waiting for a prompt) and must never scan arbitrary
 * output history with these patterns.
 */
export interface PromptMatch {
  /** UTF-16 offset immediately after the prompt marker, before command spacing. */
  end: number;
}

const PROMPT_PATTERNS: ReadonlyArray<RegExp> = [
  // PowerShell: "PS C:\\Users\\alice>" and cross-platform "PS /home/alice>".
  /^PS(?:\s+[^>\r\n]{0,200})?>/i,
  // cmd.exe: drive paths and UNC paths.
  /^(?:[A-Za-z]:\\|\\\\)[^>\r\n]{0,200}>/,
  // Unix user@host prompts, optionally preceded by venv/context.
  /^(?:(?:\([^)\r\n]{1,80}\)|\[[^\]\r\n]{1,120}\])\s*)*[^\s@:\r\n]+@[^\s:\r\n]+(?:(?::|\s+)[^#$%>\r\n]{0,160})?[#$%>]/,
  // Traditional bracket prompt: "[root@host path]#" or "[ctx]$".
  /^\[[^\]\r\n]{1,160}\][#$%>]/,
  // macOS' historical default: "host:directory user$".
  /^[A-Za-z0-9._-]{1,80}:[^#$%>\r\n]{0,120}\s+[A-Za-z0-9._-]{1,80}[#$%>]/,
  // Powerline: the final segment separator marks the end of the prompt.
  /^[^\r\n]{0,200}[\uE0B0\uE0B1]/,
  // Starship / oh-my-zsh symbolic prompts.
  /^(?:[^❯➜➤λ\r\n]{1,160}\s)?[❯➜➤λ](?=\s|$)/,
  // Versioned shell prompts such as "bash-5.2$".
  /^[A-Za-z][A-Za-z0-9._-]{0,60}[#$%>](?=\s|$)/,
  // Minimal POSIX prompts.
  /^[#$%>](?=\s|$)/,
];

export function detectPrompt(text: string): PromptMatch | null {
  for (const pattern of PROMPT_PATTERNS) {
    const match = pattern.exec(text);
    if (match) return { end: match[0].length };
  }
  return null;
}

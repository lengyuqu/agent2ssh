import type { IDisposable, IParser } from "@xterm/xterm";

const OSC_CLIPBOARD_ID = 52;
const MAX_DECODED_BYTES = 1024 * 1024;
// Base64 expands every 3 bytes to 4 characters. Allow some whitespace for
// wrapped payloads, but stop normalizing well before xterm's 10 MiB OSC cap.
const MAX_BASE64_CHARS = Math.ceil(MAX_DECODED_BYTES / 3) * 4;
const MAX_ENCODED_INPUT_CHARS = MAX_BASE64_CHARS * 2;
const BASE64_RE = /^[A-Za-z0-9+/]*={0,2}$/;
const BASE64_WHITESPACE_RE = /\s/g;

export interface ClipboardWriter {
  writeText(text: string): Promise<void> | void;
}

function targetsSystemClipboard(selector: string): boolean {
  return selector === "" || selector.includes("c");
}

function normalizeBase64(encoded: string): string | null {
  if (encoded.length > MAX_ENCODED_INPUT_CHARS) return null;

  const compact = encoded.replace(BASE64_WHITESPACE_RE, "");
  if (compact.length > MAX_BASE64_CHARS) return null;
  if (!BASE64_RE.test(compact)) return null;

  const remainder = compact.length % 4;
  if (remainder === 1) return null;
  return compact + "=".repeat((4 - remainder) % 4);
}

function decodeBase64Utf8(encoded: string): string | null {
  const normalized = normalizeBase64(encoded);
  if (normalized === null) return null;

  try {
    const binary = atob(normalized);
    if (binary.length > MAX_DECODED_BYTES) return null;
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

/**
 * Register write-only OSC 52 clipboard support on xterm's streaming parser.
 * xterm owns chunk reassembly and removes the control sequence from rendered
 * output, including when ESC, payload, or terminator arrive in different writes.
 */
export function registerClipboardOscHandler(
  parser: Pick<IParser, "registerOscHandler">,
  clipboard: ClipboardWriter,
): IDisposable {
  return parser.registerOscHandler(OSC_CLIPBOARD_ID, (data) => {
    const separator = data.indexOf(";");
    if (separator < 0) return true;

    const selector = data.slice(0, separator);
    if (!targetsSystemClipboard(selector)) return false;

    const encoded = data.slice(separator + 1);
    // Read queries would exfiltrate local clipboard content to the remote host.
    if (encoded === "?") return true;

    const text = decodeBase64Utf8(encoded);
    if (text === null) return true;

    try {
      void Promise.resolve(clipboard.writeText(text)).catch(() => {});
    } catch {
      // Clipboard permissions and platform support must not break PTY output.
    }
    return true;
  });
}

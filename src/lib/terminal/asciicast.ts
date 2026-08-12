export type CastEvent = { time: number; type: "o" | "r"; data: string };

export type ParsedCast = {
  width: number;
  height: number;
  events: CastEvent[];
};

/** Parse the safe subset of asciicast v2 used by the local replay terminal. */
export function parseAsciicast(content: string): ParsedCast {
  const lines = content.split(/\r?\n/).filter(Boolean);
  if (lines.length === 0) throw new Error("Recording is empty");
  const header = JSON.parse(lines[0]) as Record<string, unknown>;
  if (
    header.version !== 2 ||
    typeof header.width !== "number" ||
    typeof header.height !== "number" ||
    !Number.isInteger(header.width) ||
    !Number.isInteger(header.height) ||
    header.width < 2 ||
    header.height < 1 ||
    header.width > 1000 ||
    header.height > 1000
  ) {
    throw new Error("Unsupported asciicast recording");
  }
  const events: CastEvent[] = [];
  let lastTime = 0;
  for (const line of lines.slice(1)) {
    const event = JSON.parse(line) as unknown;
    if (!Array.isArray(event) || event.length < 3) continue;
    const [time, type, data] = event;
    if (
      typeof time === "number" &&
      Number.isFinite(time) &&
      time >= lastTime &&
      (type === "o" || type === "r") &&
      typeof data === "string"
    ) {
      events.push({ time, type, data });
      lastTime = time;
    }
  }
  return { width: header.width, height: header.height, events };
}

import { describe, expect, it } from "vitest";
import { parseAsciicast } from "./asciicast";

describe("parseAsciicast", () => {
  it("parses output and resize events", () => {
    const cast = parseAsciicast(
      '{"version":2,"width":80,"height":24}\n[0.1,"o","hello"]\n[0.2,"r","100x30"]\n'
    );
    expect(cast).toEqual({
      width: 80,
      height: 24,
      events: [
        { time: 0.1, type: "o", data: "hello" },
        { time: 0.2, type: "r", data: "100x30" },
      ],
    });
  });

  it("rejects unsupported or unsafe dimensions", () => {
    expect(() => parseAsciicast('{"version":1,"width":80,"height":24}\n')).toThrow();
    expect(() => parseAsciicast('{"version":2,"width":100000,"height":24}\n')).toThrow();
  });

  it("drops non-monotonic events", () => {
    const cast = parseAsciicast(
      '{"version":2,"width":80,"height":24}\n[2,"o","a"]\n[1,"o","b"]\n'
    );
    expect(cast.events).toEqual([{ time: 2, type: "o", data: "a" }]);
  });
});

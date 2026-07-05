import { diffLines } from "diff";
import { GitCompare } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import type { ExecMultiResult } from "../types";
import { Card } from "./ui/card";
import { Select } from "./ui/select";

type Props = {
  results: ExecMultiResult[];
};

function outputOf(result: ExecMultiResult | undefined): string {
  if (!result?.result) return "";
  return result.result.stdout || result.result.stderr || "";
}

/** V4-4: line-level diff between two hosts' output from the same multi-exec run,
 *  plus a same/only-in-A/only-in-B summary. Hand-built on `diff` (jsdiff) rather
 *  than a themed third-party diff-viewer component, so it stays token-exact
 *  across all 6 app themes like the rest of the app. */
export default function HostDiffView({ results }: Props) {
  const { t } = useI18n();
  const withOutput = results.filter((r) => r.result);
  const [hostA, setHostA] = useState(withOutput[0]?.host ?? "");
  const [hostB, setHostB] = useState(withOutput[1]?.host ?? withOutput[0]?.host ?? "");

  // Re-running multi-exec against a different host set updates `results` in
  // place (this component doesn't remount) — reset the picker when the
  // previously selected hosts no longer have output in the new run.
  useEffect(() => {
    const names = withOutput.map((r) => r.host);
    if (!names.includes(hostA)) setHostA(names[0] ?? "");
    if (!names.includes(hostB)) setHostB(names[1] ?? names[0] ?? "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [results]);

  const resultA = results.find((r) => r.host === hostA);
  const resultB = results.find((r) => r.host === hostB);

  const { lines, same, onlyA, onlyB } = useMemo(() => {
    const textA = outputOf(resultA);
    const textB = outputOf(resultB);
    const changes = diffLines(textA, textB);
    const rendered: Array<{ key: string; text: string; kind: "same" | "removed" | "added" }> = [];
    let sameCount = 0;
    let onlyACount = 0;
    let onlyBCount = 0;
    changes.forEach((part, partIndex) => {
      const partLines = part.value.split("\n");
      if (partLines[partLines.length - 1] === "") partLines.pop();
      const kind = part.added ? "added" : part.removed ? "removed" : "same";
      partLines.forEach((line, lineIndex) => {
        if (kind === "same") sameCount += 1;
        else if (kind === "removed") onlyACount += 1;
        else onlyBCount += 1;
        rendered.push({ key: `${partIndex}-${lineIndex}`, text: line, kind });
      });
    });
    return { lines: rendered, same: sameCount, onlyA: onlyACount, onlyB: onlyBCount };
  }, [resultA, resultB]);

  if (withOutput.length < 2) return null;

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <GitCompare size={16} className="text-muted-foreground" />
        {t("Compare hosts")}
      </div>
      <div className="grid grid-cols-2 gap-2.5">
        <Select value={hostA} onChange={(e) => setHostA(e.target.value)}>
          {withOutput.map((r) => (
            <option key={r.host} value={r.host}>
              {r.host}
            </option>
          ))}
        </Select>
        <Select value={hostB} onChange={(e) => setHostB(e.target.value)}>
          {withOutput.map((r) => (
            <option key={r.host} value={r.host}>
              {r.host}
            </option>
          ))}
        </Select>
      </div>

      <div className="flex flex-wrap gap-2 text-xs">
        <span className="rounded-full bg-muted px-2.5 py-1 font-medium text-muted-foreground">
          {t("{count} identical lines", { count: same })}
        </span>
        <span className="rounded-full bg-destructive/10 px-2.5 py-1 font-medium text-destructive">
          {t("{count} only in {host}", { count: onlyA, host: hostA })}
        </span>
        <span className="rounded-full bg-success/10 px-2.5 py-1 font-medium text-success">
          {t("{count} only in {host}", { count: onlyB, host: hostB })}
        </span>
      </div>

      <pre className="m-0 max-h-[360px] overflow-auto rounded-md bg-[#0e1620] p-0 font-mono text-[13px] text-[#e6edf3]">
        {lines.length === 0 ? (
          <div className="px-3.5 py-2.5 text-[#8fb0c5]">{t("(no output)")}</div>
        ) : (
          lines.map((line) => (
            <div
              key={line.key}
              className={
                line.kind === "removed"
                  ? "bg-[#4a1f24] px-3.5 text-[#ffb4a6]"
                  : line.kind === "added"
                    ? "bg-[#1f3a2a] px-3.5 text-[#9be3ae]"
                    : "px-3.5"
              }
            >
              {line.kind === "removed" ? "- " : line.kind === "added" ? "+ " : "  "}
              {line.text || " "}
            </div>
          ))
        )}
      </pre>
    </Card>
  );
}

import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, FileUp } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { NetcattyImportReport } from "../types";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { Spinner } from "./ui/state";
import { useToast } from "./ui/toast";

type Props = {
  open: boolean;
  onClose: () => void;
  /** Re-read the host list once something was actually written. */
  onImported: () => void | Promise<void>;
};

type Stage = "pick" | "previewing" | "preview" | "importing" | "done";

/**
 * Import a Netcatty vault backup.
 *
 * The two-phase shape is deliberate: the backend can map the file without
 * writing (`dryRun`), so the user sees exactly what will change — including the
 * rules that cannot be carried over — before anything lands in `hosts.json`.
 * A locked credential store makes the real import fail loudly rather than
 * writing passwords in plaintext, and that error is surfaced verbatim.
 */
export default function NetcattyImportDialog({ open, onClose, onImported }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [stage, setStage] = useState<Stage>("pick");
  const [path, setPath] = useState<string | null>(null);
  const [report, setReport] = useState<NetcattyImportReport | null>(null);
  const [skipCredentials, setSkipCredentials] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reopening starts from a clean slate: a leftover preview from the previous
  // file would look like it describes the one just picked.
  useEffect(() => {
    if (!open) return;
    setStage("pick");
    setPath(null);
    setReport(null);
    setError(null);
  }, [open]);

  async function pickFile() {
    setError(null);
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: "Netcatty backup", extensions: ["json"] }],
      });
      if (typeof picked !== "string") return;
      setPath(picked);
      setStage("previewing");
      const preview = await api.netcattyImport(picked, {
        dryRun: true,
        skipCredentials,
      });
      setReport(preview);
      setStage("preview");
    } catch (err) {
      setError(String(err));
      setStage("pick");
    }
  }

  async function runImport() {
    if (!path) return;
    setStage("importing");
    setError(null);
    try {
      const result = await api.netcattyImport(path, { dryRun: false, skipCredentials });
      setReport(result);
      setStage("done");
      showToast(
        "success",
        t("Imported {hosts} host(s) and {keys} key(s) from Netcatty.", {
          hosts: result.hostsImported.length,
          keys: result.keysWritten.length,
        }),
      );
      await onImported();
    } catch (err) {
      setError(String(err));
      setStage("preview");
    }
  }

  const busy = stage === "importing";

  return (
    <Dialog onClose={busy ? () => {} : onClose} className="w-[560px] max-w-[92vw]">
      <div className="space-y-4">
        <div className="flex items-center gap-2 text-base font-semibold">
          <FileUp size={18} className="text-muted-foreground" />
          {t("Import from Netcatty")}
        </div>

        <p className="text-sm text-muted-foreground">
          {t(
            "Reads a Netcatty vault backup (encrypted or plain JSON) and adds its hosts, private keys and command snippets. Existing hosts are skipped, never overwritten.",
          )}
        </p>

        {stage === "pick" && (
          <div className="space-y-3">
            <Button onClick={pickFile} variant="secondary">
              {t("Choose backup file…")}
            </Button>
            {error && <ErrorBox message={error} />}
          </div>
        )}

        {stage === "previewing" && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> {t("Reading the backup…")}
          </div>
        )}

        {report && stage !== "pick" && stage !== "previewing" && (
          <div className="space-y-3">
            <div className="rounded-lg border border-border bg-muted/40 p-3 text-sm">
              <Summary report={report} />
            </div>
            {report.warnings.length > 0 && (
              <div className="space-y-1 rounded-lg border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
                {report.warnings.map((warning) => (
                  <div key={warning} className="flex gap-1.5">
                    <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                    <span>{warning}</span>
                  </div>
                ))}
              </div>
            )}
            {report.forwardsNotImported.length > 0 && (
              <div className="space-y-1 text-xs">
                <div className="text-muted-foreground">
                  {t("Port forwards are not part of the configuration. Recreate them with:")}
                </div>
                <pre className="max-h-32 overflow-auto rounded-lg border border-border bg-canvas-1 p-2 font-mono text-[11px] leading-relaxed">
                  {report.forwardsNotImported.join("\n")}
                </pre>
              </div>
            )}
            {report.hostsRenamed.length > 0 && (
              <div className="text-xs text-muted-foreground">
                {report.hostsRenamed
                  .map(([from, to]) => `${from} → ${to}`)
                  .join(", ")}
              </div>
            )}
          </div>
        )}

        {error && stage !== "pick" && <ErrorBox message={error} />}

        {stage === "preview" && (
          <>
            <label className="flex items-center gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                checked={skipCredentials}
                onChange={(event) => setSkipCredentials(event.target.checked)}
              />
              {t("Import hosts only — leave passwords and private keys behind")}
            </label>
            <div className="flex justify-end gap-2">
              <Button variant="ghost" onClick={onClose}>
                {t("Cancel")}
              </Button>
              <Button onClick={runImport}>{t("Import")}</Button>
            </div>
          </>
        )}

        {busy && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> {t("Importing…")}
          </div>
        )}

        {stage === "done" && (
          <div className="flex justify-end">
            <Button onClick={onClose}>{t("Done")}</Button>
          </div>
        )}
      </div>
    </Dialog>
  );
}

function Summary({ report }: { report: NetcattyImportReport }) {
  const { t } = useI18n();
  return (
    <div className="space-y-1">
      <div>
        {t("{count} host(s)", { count: report.hostsImported.length })} ·{" "}
        {t("{count} private key(s)", { count: report.keysWritten.length })}
        {report.groupsCreated.length > 0 && (
          <> · {t("{count} group(s)", { count: report.groupsCreated.length })}</>
        )}
        {report.snippetsImported.length > 0 && (
          <> · {t("{count} snippet(s)", { count: report.snippetsImported.length })}</>
        )}
      </div>
      {report.hostsSkippedExisting.length > 0 && (
        <div className="text-muted-foreground">
          {t("{count} host(s) already exist and will be skipped", {
            count: report.hostsSkippedExisting.length,
          })}
        </div>
      )}
      {report.jumpHostsLinked > 0 && (
        <div className="text-muted-foreground">
          {t("{count} host(s) use a jump host", { count: report.jumpHostsLinked })}
        </div>
      )}
      {report.passwordsSealed > 0 && (
        <div className="text-muted-foreground">
          {t("{count} password(s) stored in the encrypted credential store", {
            count: report.passwordsSealed,
          })}
        </div>
      )}
    </div>
  );
}

function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-lg border border-risk-high/40 bg-risk-high/10 p-2 text-xs text-risk-high">
      {message}
    </div>
  );
}

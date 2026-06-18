import {
  BookOpen,
  CheckCircle,
  Edit3,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, Playbook, PlaybookRunResult, RiskLevel } from "../types";
import RiskBadge from "./RiskBadge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { Textarea } from "./ui/textarea";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
};

type PlaybookForm = {
  name: string;
  description: string;
  tags: string;
  risk_override: "" | RiskLevel;
  stepsText: string;
};

const emptyForm: PlaybookForm = {
  name: "",
  description: "",
  tags: "",
  risk_override: "",
  stepsText: "",
};

const fieldCls = "grid gap-1.5 text-xs font-bold text-muted-foreground";

function formFromPlaybook(playbook: Playbook): PlaybookForm {
  return {
    name: playbook.name,
    description: playbook.description,
    tags: playbook.tags.join(", "),
    risk_override: playbook.risk_override ?? "",
    stepsText: playbook.steps.join("\n"),
  };
}

function playbookFromForm(form: PlaybookForm): Playbook {
  return {
    name: form.name.trim(),
    description: form.description.trim(),
    tags: form.tags
      .split(",")
      .map((tag) => tag.trim())
      .filter(Boolean),
    risk_override: form.risk_override || null,
    steps: form.stepsText
      .split("\n")
      .map((step) => step.trim())
      .filter(Boolean),
    advanced_steps: null,
  };
}

function stepCount(playbook: Playbook): number {
  return playbook.advanced_steps?.length || playbook.steps.length;
}

export default function PlaybooksPanel({ hosts }: Props) {
  const { t } = useI18n();
  const [playbooks, setPlaybooks] = useState<Playbook[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editingOriginalName, setEditingOriginalName] = useState<string | null>(null);
  const [form, setForm] = useState<PlaybookForm>(emptyForm);
  const [selectedPlaybook, setSelectedPlaybook] = useState<string | null>(null);
  const [selectedHost, setSelectedHost] = useState("");
  const [force, setForce] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<PlaybookRunResult | null>(null);

  const editing = editingOriginalName !== null;
  const selectedPlaybookDef = useMemo(
    () => playbooks.find((playbook) => playbook.name === selectedPlaybook),
    [playbooks, selectedPlaybook]
  );

  async function refresh() {
    setLoading(true);
    try {
      const list = await api.listPlaybooks();
      setPlaybooks(list);
    } catch {
      setPlaybooks([]);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  function startCreate() {
    setEditingOriginalName("");
    setForm(emptyForm);
    setMessage(null);
    setError(null);
  }

  function startEdit(playbook: Playbook) {
    if (playbook.advanced_steps?.length) {
      setError(
        t("Parameterized playbooks can be run, but this editor only supports simple step lists.")
      );
      return;
    }
    setEditingOriginalName(playbook.name);
    setForm(formFromPlaybook(playbook));
    setMessage(null);
    setError(null);
  }

  function cancelEdit() {
    setEditingOriginalName(null);
    setForm(emptyForm);
  }

  async function handleSave() {
    const next = playbookFromForm(form);
    if (!next.name || next.steps.length === 0) {
      setError(t("Playbook name and at least one step are required"));
      return;
    }
    setSaving(true);
    setError(null);
    setMessage(null);
    try {
      if (
        editingOriginalName &&
        editingOriginalName !== next.name &&
        playbooks.some((playbook) => playbook.name === next.name)
      ) {
        throw new Error(t("A playbook named {name} already exists", { name: next.name }));
      }
      const saved = await api.savePlaybook(next);
      if (editingOriginalName && editingOriginalName !== next.name) {
        await api.deletePlaybook(editingOriginalName);
      }
      setMessage(t("Saved playbook: {name}", { name: saved.name }));
      setEditingOriginalName(null);
      setForm(emptyForm);
      await refresh();
    } catch (err) {
      setError(String(err));
      reportError("playbooks-panel", "save playbook failed", err);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(playbook: Playbook) {
    if (!window.confirm(t("Delete playbook {name}?", { name: playbook.name }))) return;
    setError(null);
    setMessage(null);
    try {
      await api.deletePlaybook(playbook.name);
      if (selectedPlaybook === playbook.name) setSelectedPlaybook(null);
      if (editingOriginalName === playbook.name) cancelEdit();
      setMessage(t("Deleted playbook: {name}", { name: playbook.name }));
      await refresh();
    } catch (err) {
      setError(String(err));
      reportError("playbooks-panel", "delete playbook failed", err, { name: playbook.name });
    }
  }

  function showRunForm(name: string) {
    setSelectedPlaybook(name);
    setResult(null);
    if (hosts.length > 0 && !selectedHost) {
      setSelectedHost(hosts[0].name);
    }
  }

  function hideRunForm() {
    setSelectedPlaybook(null);
    setResult(null);
  }

  async function handleRun() {
    if (!selectedPlaybook || !selectedHost) {
      setError(t("Select a playbook and target host"));
      return;
    }
    setError(null);
    setRunning(true);
    setResult(null);
    try {
      const res = await api.runPlaybook(selectedPlaybook, selectedHost, force);
      setResult(res);
    } catch (err) {
      setError(String(err));
      reportError("playbooks-panel", "run playbook failed", err, { playbook: selectedPlaybook, host: selectedHost });
    } finally {
      setRunning(false);
    }
  }

  return (
    <Card className="space-y-3 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <BookOpen size={16} className="text-muted-foreground" />
        {t("Playbooks")}
        <IconButton
          className="ml-auto"
          title={t("Refresh")}
          onClick={refresh}
          disabled={loading}
        >
          <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
        </IconButton>
        <Button variant="secondary" size="sm" onClick={startCreate}>
          <Plus size={14} />
          {t("New")}
        </Button>
      </div>

      {message && (
        <div className="rounded-md bg-success/12 px-3 py-2 text-sm text-success">{message}</div>
      )}
      {error && (
        <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {editing && (
        <div className="grid gap-3 rounded-lg border border-border bg-muted/40 p-3">
          <div className="grid grid-cols-[minmax(0,1fr)_180px] gap-3 max-md:grid-cols-1">
            <label className={fieldCls}>
              <span>{t("Name")}</span>
              <Input
                value={form.name}
                onChange={(event) => setForm((prev) => ({ ...prev, name: event.target.value }))}
                placeholder="health-check"
              />
            </label>
            <label className={fieldCls}>
              <span>{t("Risk override")}</span>
              <Select
                value={form.risk_override}
                onChange={(event) =>
                  setForm((prev) => ({
                    ...prev,
                    risk_override: event.target.value as PlaybookForm["risk_override"],
                  }))
                }
              >
                <option value="">{t("None")}</option>
                <option value="low">{t("low")}</option>
                <option value="medium">{t("medium")}</option>
                <option value="high">{t("high")}</option>
                <option value="blocked">{t("blocked")}</option>
              </Select>
            </label>
          </div>
          <label className={fieldCls}>
            <span>{t("Description")}</span>
            <Input
              value={form.description}
              onChange={(event) =>
                setForm((prev) => ({ ...prev, description: event.target.value }))
              }
              placeholder={t("Describe what this playbook does")}
            />
          </label>
          <label className={fieldCls}>
            <span>{t("Tags")}</span>
            <Input
              value={form.tags}
              onChange={(event) => setForm((prev) => ({ ...prev, tags: event.target.value }))}
              placeholder="ops, diagnostics"
            />
          </label>
          <label className={fieldCls}>
            <span>{t("Steps")}</span>
            <Textarea
              className="min-h-[150px]"
              value={form.stepsText}
              onChange={(event) =>
                setForm((prev) => ({ ...prev, stepsText: event.target.value }))
              }
              placeholder={"uname -a\ndf -h\nsystemctl status nginx --no-pager"}
              rows={7}
            />
          </label>
          <div className="flex items-center gap-2">
            <Button onClick={handleSave} disabled={saving}>
              {saving ? <Loader2 size={14} className="animate-spin" /> : <Save size={14} />}
              {t("Save")}
            </Button>
            <Button variant="secondary" onClick={cancelEdit} disabled={saving}>
              <X size={14} />
              {t("Cancel")}
            </Button>
          </div>
        </div>
      )}

      {playbooks.length === 0 && !editing && (
        <p className="px-1 py-2 text-sm text-muted-foreground">
          {t("No playbooks configured. Create one here or edit")}{" "}
          <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
            ~/.agent2ssh/playbooks.toml
          </code>
          .
        </p>
      )}

      {playbooks.length > 0 && (
        <div className="space-y-2">
          {playbooks.map((pb) => (
            <div
              key={pb.name}
              className="flex items-center justify-between gap-3.5 rounded-lg border border-border bg-card p-3 max-md:flex-col max-md:items-stretch"
            >
              <div className="min-w-0 flex-1">
                <div className="font-semibold">{pb.name}</div>
                <div className="mt-0.5 text-sm text-muted-foreground">{pb.description}</div>
                <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                  <span>
                    {stepCount(pb)} {t(stepCount(pb) === 1 ? "step" : "steps")}
                  </span>
                  {pb.tags.map((tag) => (
                    <span
                      key={tag}
                      className="rounded bg-primary/10 px-1.5 py-0.5 text-[10px] font-medium text-primary"
                    >
                      {tag}
                    </span>
                  ))}
                  {pb.risk_override && <RiskBadge level={pb.risk_override} />}
                </div>
              </div>
              <div className="flex items-center gap-2 max-md:justify-end">
                <Button variant="secondary" size="sm" onClick={() => startEdit(pb)}>
                  <Edit3 size={13} />
                  {t("Edit")}
                </Button>
                <Button size="sm" onClick={() => showRunForm(pb.name)}>
                  <Play size={13} />
                  {t("Run")}
                </Button>
                <Button variant="destructive" size="sm" onClick={() => handleDelete(pb)}>
                  <Trash2 size={13} />
                  {t("Delete")}
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {selectedPlaybook && (
        <div className="rounded-lg border border-border bg-muted/40 p-3">
          <div className="mb-2.5 flex items-center gap-2 font-semibold">
            {t("Run:")} <span className="font-mono">{selectedPlaybook}</span>
            {selectedPlaybookDef?.risk_override && (
              <RiskBadge level={selectedPlaybookDef.risk_override} />
            )}
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <Select
              className="min-w-[150px] flex-none"
              value={selectedHost}
              onChange={(event) => setSelectedHost(event.target.value)}
            >
              {hosts.length === 0 && <option value="">{t("No hosts")}</option>}
              {hosts.map((host) => (
                <option key={host.name} value={host.name}>
                  {host.name}
                </option>
              ))}
            </Select>
            <label
              className={cn(
                "inline-flex cursor-pointer select-none items-center gap-1.5 font-semibold",
                force ? "text-destructive" : "text-foreground/80"
              )}
            >
              <input
                type="checkbox"
                className="size-4 accent-destructive"
                checked={force}
                onChange={(event) => setForce(event.target.checked)}
              />
              {t("Force")}
            </label>
            <Button disabled={running || !selectedHost} onClick={handleRun}>
              {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
              {running ? t("Running...") : t("Execute")}
            </Button>
            <Button variant="secondary" onClick={hideRunForm}>
              <X size={14} />
              {t("Cancel")}
            </Button>
          </div>
        </div>
      )}

      {result && (
        <div>
          <div className="mb-2 flex items-center justify-between gap-2 font-semibold">
            {result.success ? (
              <span className="inline-flex items-center gap-1.5 text-success">
                <CheckCircle size={16} /> {t("Success")}
              </span>
            ) : (
              <span className="inline-flex items-center gap-1.5 text-destructive">
                <XCircle size={16} /> {t("Failed")}
              </span>
            )}
            <span className="text-sm font-normal text-muted-foreground">
              {result.steps_completed.length}/
              {selectedPlaybookDef ? stepCount(selectedPlaybookDef) : "?"} {t("steps")}
              {" · "}
              {result.total_duration_ms < 1000
                ? `${result.total_duration_ms}ms`
                : `${(result.total_duration_ms / 1000).toFixed(2)}s`}
            </span>
          </div>

          <div className="space-y-1.5">
            {result.steps_completed.map((step) => {
              const ok = step.result && step.result.exit_code === 0;
              return (
                <div
                  key={step.step}
                  className={cn(
                    "rounded-md border p-2.5",
                    ok
                      ? "border-success/30 bg-success/10"
                      : "border-destructive/30 bg-destructive/10"
                  )}
                >
                  <div className="flex flex-wrap items-center gap-2">
                    {ok ? (
                      <CheckCircle size={14} className="text-success" />
                    ) : (
                      <XCircle size={14} className="text-destructive" />
                    )}
                    <span className="flex-1 font-mono text-sm">
                      {step.step + 1}. {step.command}
                    </span>
                    {step.result && (
                      <span className="text-xs text-muted-foreground">
                        exit={step.result.exit_code ?? "n/a"}{" "}
                        {step.result.duration_ms < 1000
                          ? `${step.result.duration_ms}ms`
                          : `${(step.result.duration_ms / 1000).toFixed(2)}s`}
                      </span>
                    )}
                  </div>
                  {step.result?.stdout && (
                    <pre className="m-0 mt-1.5 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-black/5 p-1.5 font-mono text-xs dark:bg-white/5">
                      {step.result.stdout}
                    </pre>
                  )}
                  {step.result?.stderr && (
                    <pre className="m-0 mt-1.5 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-black/5 p-1.5 font-mono text-xs text-destructive dark:bg-white/5">
                      {step.result.stderr}
                    </pre>
                  )}
                  {step.error && (
                    <div className="mt-1.5 text-xs text-destructive">{step.error}</div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </Card>
  );
}

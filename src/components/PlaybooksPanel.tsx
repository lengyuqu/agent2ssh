import {
  BookOpen,
  CheckCircle,
  Edit3,
  FileCode,
  GripVertical,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  Save,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { dump as dumpYaml, load as loadYaml } from "js-yaml";
import { useEffect, useMemo, useRef, useState } from "react";
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
import { EmptyState } from "./ui/state";
import { useToast } from "./ui/toast";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
};

type PlaybookForm = {
  name: string;
  description: string;
  tags: string;
  risk_override: "" | RiskLevel;
  steps: string[];
};

const emptyForm: PlaybookForm = {
  name: "",
  description: "",
  tags: "",
  risk_override: "",
  steps: [""],
};

const fieldCls = "grid gap-1.5 text-xs font-bold text-muted-foreground";

function formFromPlaybook(playbook: Playbook): PlaybookForm {
  return {
    name: playbook.name,
    description: playbook.description,
    tags: playbook.tags.join(", "),
    risk_override: playbook.risk_override ?? "",
    steps: playbook.steps.length > 0 ? [...playbook.steps] : [""],
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
    steps: form.steps.map((step) => step.trim()).filter(Boolean),
    advanced_steps: null,
  };
}

// V4-2: the visual step editor is the source of truth; this YAML view is a
// bidirectional alternate representation for the same fields — editing it and
// applying re-parses back into `steps`/`tags`/etc., it isn't a separate format
// the backend understands (playbooks persist as TOML either way).
type PlaybookYamlShape = {
  name?: unknown;
  description?: unknown;
  tags?: unknown;
  risk_override?: unknown;
  steps?: unknown;
};

function yamlFromForm(form: PlaybookForm): string {
  const doc: Record<string, unknown> = {
    name: form.name,
    description: form.description,
    tags: form.tags
      .split(",")
      .map((tag) => tag.trim())
      .filter(Boolean),
    steps: form.steps.map((step) => step.trim()).filter(Boolean),
  };
  if (form.risk_override) doc.risk_override = form.risk_override;
  return dumpYaml(doc, { lineWidth: -1 });
}

const RISK_LEVELS = new Set(["low", "medium", "high", "blocked"]);

function formFromYaml(text: string): PlaybookForm {
  const parsed = loadYaml(text);
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new Error("Expected a YAML mapping with name/description/tags/steps.");
  }
  const shape = parsed as PlaybookYamlShape;
  const steps = Array.isArray(shape.steps)
    ? shape.steps.filter((s): s is string => typeof s === "string")
    : [];
  const tags = Array.isArray(shape.tags)
    ? shape.tags.filter((t): t is string => typeof t === "string")
    : [];
  const riskOverride =
    typeof shape.risk_override === "string" && RISK_LEVELS.has(shape.risk_override)
      ? (shape.risk_override as RiskLevel)
      : "";
  return {
    name: typeof shape.name === "string" ? shape.name : "",
    description: typeof shape.description === "string" ? shape.description : "",
    tags: tags.join(", "),
    risk_override: riskOverride,
    steps: steps.length > 0 ? steps : [""],
  };
}

function stepCount(playbook: Playbook): number {
  return playbook.advanced_steps?.length || playbook.steps.length;
}

export default function PlaybooksPanel({ hosts }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [playbooks, setPlaybooks] = useState<Playbook[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [editingOriginalName, setEditingOriginalName] = useState<string | null>(null);
  const [form, setForm] = useState<PlaybookForm>(emptyForm);
  const [selectedPlaybook, setSelectedPlaybook] = useState<string | null>(null);
  const [selectedHost, setSelectedHost] = useState("");
  const [force, setForce] = useState(false);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<PlaybookRunResult | null>(null);
  // V4-2: visual step editor (drag-reorder) is the primary UI; YAML is a
  // bidirectional alternate view of the same fields.
  const [editorMode, setEditorMode] = useState<"visual" | "yaml">("visual");
  const [yamlText, setYamlText] = useState("");
  const [yamlError, setYamlError] = useState<string | null>(null);
  const dragIndexRef = useRef<number | null>(null);

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
    setEditorMode("visual");
    setYamlError(null);
  }

  function startEdit(playbook: Playbook) {
    if (playbook.advanced_steps?.length) {
      showToast(
        "error",
        t("Parameterized playbooks can be run, but this editor only supports simple step lists.")
      );
      return;
    }
    setEditingOriginalName(playbook.name);
    setForm(formFromPlaybook(playbook));
    setEditorMode("visual");
    setYamlError(null);
  }

  function cancelEdit() {
    setEditingOriginalName(null);
    setForm(emptyForm);
  }

  function updateStep(index: number, value: string) {
    setForm((prev) => ({ ...prev, steps: prev.steps.map((s, i) => (i === index ? value : s)) }));
  }

  function addStep() {
    setForm((prev) => ({ ...prev, steps: [...prev.steps, ""] }));
  }

  function removeStep(index: number) {
    setForm((prev) => ({
      ...prev,
      steps: prev.steps.length > 1 ? prev.steps.filter((_, i) => i !== index) : [""],
    }));
  }

  function reorderSteps(from: number, to: number) {
    setForm((prev) => {
      const steps = [...prev.steps];
      const [moved] = steps.splice(from, 1);
      steps.splice(to, 0, moved);
      return { ...prev, steps };
    });
  }

  function switchToYaml() {
    setYamlText(yamlFromForm(form));
    setYamlError(null);
    setEditorMode("yaml");
  }

  function applyYamlToForm(): boolean {
    try {
      setForm(formFromYaml(yamlText));
      setYamlError(null);
      return true;
    } catch (err) {
      setYamlError(String(err instanceof Error ? err.message : err));
      return false;
    }
  }

  function switchToVisual() {
    if (applyYamlToForm()) setEditorMode("visual");
  }

  async function handleSave() {
    // In YAML mode, the visual `form` state is stale until applied — parse the
    // textarea one more time so Save always persists what's on screen.
    if (editorMode === "yaml" && !applyYamlToForm()) return;
    const next = playbookFromForm(editorMode === "yaml" ? formFromYaml(yamlText) : form);
    if (!next.name || next.steps.length === 0) {
      showToast("error", t("Playbook name and at least one step are required"));
      return;
    }
    setSaving(true);
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
      showToast("success", t("Saved playbook: {name}", { name: saved.name }));
      setEditingOriginalName(null);
      setForm(emptyForm);
      setEditorMode("visual");
      await refresh();
    } catch (err) {
      showToast("error", String(err));
      reportError("playbooks-panel", "save playbook failed", err);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(playbook: Playbook) {
    if (!window.confirm(t("Delete playbook {name}?", { name: playbook.name }))) return;
    try {
      await api.deletePlaybook(playbook.name);
      if (selectedPlaybook === playbook.name) setSelectedPlaybook(null);
      if (editingOriginalName === playbook.name) cancelEdit();
      showToast("success", t("Deleted playbook: {name}", { name: playbook.name }));
      await refresh();
    } catch (err) {
      showToast("error", String(err));
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
      showToast("error", t("Select a playbook and target host"));
      return;
    }
    setRunning(true);
    setResult(null);
    try {
      const res = await api.runPlaybook(selectedPlaybook, selectedHost, force);
      setResult(res);
    } catch (err) {
      showToast("error", String(err));
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
          <div className="grid gap-1.5">
            <div className="flex items-center justify-between">
              <span className={cn(fieldCls, "text-xs")}>{t("Steps")}</span>
              <div className="inline-flex overflow-hidden rounded-md border border-border">
                <button
                  type="button"
                  onClick={() => setEditorMode("visual")}
                  className={cn(
                    "px-2 py-1 text-xs font-semibold",
                    editorMode === "visual"
                      ? "bg-primary text-primary-foreground"
                      : "bg-card text-muted-foreground hover:text-foreground"
                  )}
                >
                  {t("Steps")}
                </button>
                <button
                  type="button"
                  onClick={switchToYaml}
                  className={cn(
                    "inline-flex items-center gap-1 px-2 py-1 text-xs font-semibold",
                    editorMode === "yaml"
                      ? "bg-primary text-primary-foreground"
                      : "bg-card text-muted-foreground hover:text-foreground"
                  )}
                >
                  <FileCode size={12} />
                  YAML
                </button>
              </div>
            </div>

            {editorMode === "visual" ? (
              <div className="grid gap-1.5">
                {form.steps.map((step, index) => (
                  <div
                    key={index}
                    draggable
                    onDragStart={() => {
                      dragIndexRef.current = index;
                    }}
                    onDragOver={(event) => event.preventDefault()}
                    onDrop={(event) => {
                      event.preventDefault();
                      if (dragIndexRef.current !== null && dragIndexRef.current !== index) {
                        reorderSteps(dragIndexRef.current, index);
                      }
                      dragIndexRef.current = null;
                    }}
                    className="flex items-center gap-1.5 rounded-md border border-border bg-card px-1.5 py-1"
                  >
                    <span className="cursor-grab text-muted-foreground/60" title={t("Drag to reorder")}>
                      <GripVertical size={14} />
                    </span>
                    <span className="w-5 shrink-0 text-center text-xs text-muted-foreground">
                      {index + 1}
                    </span>
                    <Input
                      value={step}
                      onChange={(event) => updateStep(index, event.target.value)}
                      placeholder="uname -a"
                      className="h-8 border-none bg-transparent px-1 shadow-none focus-visible:ring-0"
                    />
                    <IconButton
                      size="sm"
                      title={t("Remove step")}
                      onClick={() => removeStep(index)}
                      disabled={form.steps.length === 1 && !step}
                    >
                      <X size={13} />
                    </IconButton>
                  </div>
                ))}
                <Button variant="outline" size="sm" onClick={addStep} className="justify-center">
                  <Plus size={13} />
                  {t("Add step")}
                </Button>
              </div>
            ) : (
              <div className="grid gap-1.5">
                <Textarea
                  className="min-h-[180px] font-mono text-xs"
                  value={yamlText}
                  onChange={(event) => {
                    setYamlText(event.target.value);
                    setYamlError(null);
                  }}
                  spellCheck={false}
                  rows={10}
                />
                {yamlError && <div className="text-xs text-destructive">{yamlError}</div>}
                <Button variant="secondary" size="sm" onClick={switchToVisual} className="justify-center">
                  {t("Apply YAML to step editor")}
                </Button>
              </div>
            )}
          </div>
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
        <EmptyState
          icon={BookOpen}
          title={t("No playbooks configured")}
          description={t("Create one here or edit ~/.agent2ssh/playbooks.toml directly.")}
        />
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
                    <pre className="m-0 mt-1.5 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-foreground/5 p-1.5 font-mono text-xs">
                      {step.result.stdout}
                    </pre>
                  )}
                  {step.result?.stderr && (
                    <pre className="m-0 mt-1.5 max-h-40 overflow-auto whitespace-pre-wrap break-words rounded bg-foreground/5 p-1.5 font-mono text-xs text-destructive">
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

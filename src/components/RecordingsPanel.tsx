import { useEffect, useRef, useState } from "react";
import { Pause, Play, RefreshCw, RotateCcw, ShieldAlert, Trash2 } from "lucide-react";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { api, reportError } from "../api";
import { useI18n } from "../i18n";
import { parseAsciicast, type CastEvent, type ParsedCast } from "../lib/terminal/asciicast";
import type { RecordingContent, RecordingInfo } from "../types";
import { Button } from "./ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { useToast } from "./ui/toast";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

export default function RecordingsPanel() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const timerRef = useRef<number | null>(null);
  const eventsRef = useRef<CastEvent[]>([]);
  const eventIndexRef = useRef(0);
  const speedRef = useRef(1);
  const [recordings, setRecordings] = useState<RecordingInfo[]>([]);
  const [selected, setSelected] = useState<RecordingContent | null>(null);
  const [recordingEnabled, setRecordingEnabled] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [speed, setSpeed] = useState(1);
  const [progress, setProgress] = useState(0);

  async function refresh() {
    setLoading(true);
    try {
      const [config, items] = await Promise.all([
        api.getRecordingConfig(),
        api.listRecordings(),
      ]);
      setRecordingEnabled(config.enabled);
      setRecordings(items);
    } catch (error) {
      reportError("recordings-panel", "failed to load recordings", error);
      showToast("error", t("Failed to load recordings: {error}", { error: String(error) }));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const terminal = new Terminal({
      cursorBlink: false,
      disableStdin: true,
      convertEol: false,
      scrollback: 10_000,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      fontSize: 13,
      theme: { background: "#0d1117", foreground: "#c9d1d9" },
    });
    terminal.open(container);
    terminal.write(t("Select a recording to replay."));
    terminalRef.current = terminal;
    return () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current);
      terminal.dispose();
      terminalRef.current = null;
    };
  }, [t]);

  function stopPlayback() {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setPlaying(false);
  }

  function resetPlayer(cast: ParsedCast) {
    stopPlayback();
    eventsRef.current = cast.events;
    eventIndexRef.current = 0;
    setProgress(0);
    const terminal = terminalRef.current;
    if (!terminal) return;
    terminal.reset();
    terminal.resize(Math.max(2, cast.width), Math.max(1, cast.height));
  }

  function scheduleNextEvent() {
    const terminal = terminalRef.current;
    const events = eventsRef.current;
    const index = eventIndexRef.current;
    if (!terminal || index >= events.length) {
      timerRef.current = null;
      setPlaying(false);
      setProgress(1);
      return;
    }
    const event = events[index];
    const previousTime = index === 0 ? 0 : events[index - 1].time;
    const delay = Math.max(0, ((event.time - previousTime) * 1000) / speedRef.current);
    timerRef.current = window.setTimeout(() => {
      eventIndexRef.current = index + 1;
      setProgress(events.length === 0 ? 1 : (index + 1) / events.length);
      if (event.type === "r") {
        const match = /^(\d+)x(\d+)$/.exec(event.data);
        if (match) terminal.resize(Number(match[1]), Number(match[2]));
        scheduleNextEvent();
      } else {
        terminal.write(event.data, scheduleNextEvent);
      }
    }, delay);
  }

  function togglePlayback() {
    if (playing) {
      stopPlayback();
      return;
    }
    if (eventIndexRef.current >= eventsRef.current.length && selected) {
      resetPlayer(parseAsciicast(selected.content));
    }
    setPlaying(true);
    scheduleNextEvent();
  }

  async function selectRecording(info: RecordingInfo) {
    setBusyId(info.id);
    try {
      const content = await api.readRecording(info.id);
      const cast = parseAsciicast(content.content);
      setSelected(content);
      resetPlayer(cast);
    } catch (error) {
      reportError("recordings-panel", "failed to read recording", error, { id: info.id });
      showToast("error", t("Failed to read recording: {error}", { error: String(error) }));
    } finally {
      setBusyId(null);
    }
  }

  async function toggleRecordingEnabled() {
    const enabled = !recordingEnabled;
    try {
      const config = await api.setRecordingConfig({ enabled });
      setRecordingEnabled(config.enabled);
      showToast(
        "success",
        t(config.enabled ? "Terminal recording enabled for new sessions." : "Terminal recording disabled.")
      );
    } catch (error) {
      reportError("recordings-panel", "failed to update recording config", error);
      showToast("error", t("Failed to update recording setting: {error}", { error: String(error) }));
    }
  }

  async function removeRecording(info: RecordingInfo) {
    if (
      !window.confirm(
        t("Permanently delete this sensitive terminal recording from {host}?", { host: info.host })
      )
    ) {
      return;
    }
    setBusyId(info.id);
    try {
      await api.deleteRecording(info.id);
      if (selected?.info.id === info.id) {
        setSelected(null);
        stopPlayback();
        terminalRef.current?.reset();
        terminalRef.current?.write(t("Select a recording to replay."));
      }
      await refresh();
      showToast("success", t("Recording deleted."));
    } catch (error) {
      reportError("recordings-panel", "failed to delete recording", error, { id: info.id });
      showToast("error", t("Failed to delete recording: {error}", { error: String(error) }));
    } finally {
      setBusyId(null);
    }
  }

  function changeSpeed(nextSpeed: number) {
    speedRef.current = nextSpeed;
    setSpeed(nextSpeed);
  }

  return (
    <div className="grid gap-4">
      <Card className="border-warning/40 bg-warning/5">
        <CardHeader>
          <CardTitle className="text-warning">
            <ShieldAlert size={18} /> {t("Sensitive terminal recordings")}
          </CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3 text-sm">
          <p className="text-muted-foreground">
            {t(
              "Recordings capture raw terminal output and may contain passwords, tokens, personal data, or command results. They remain local and are never included in WebDAV sync."
            )}
          </p>
          <label className="flex items-center gap-2 font-medium">
            <input
              type="checkbox"
              checked={recordingEnabled}
              onChange={() => void toggleRecordingEnabled()}
              className="size-4 accent-primary"
            />
            {t("Record new terminal sessions (off by default)")}
          </label>
          <p className="text-xs text-muted-foreground">
            {t("Changing this setting affects new terminal sessions only.")}
          </p>
        </CardContent>
      </Card>

      <div className="grid grid-cols-[minmax(280px,0.38fr)_minmax(0,0.62fr)] gap-4 max-lg:grid-cols-1">
        <Card>
          <CardHeader className="flex-row items-center justify-between">
            <CardTitle>{t("Recordings")}</CardTitle>
            <Button variant="outline" size="sm" onClick={() => void refresh()} disabled={loading}>
              <RefreshCw className={loading ? "animate-spin" : ""} /> {t("Refresh")}
            </Button>
          </CardHeader>
          <CardContent className="grid max-h-[560px] gap-2 overflow-y-auto">
            {!loading && recordings.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted-foreground">{t("No terminal recordings.")}</p>
            ) : (
              recordings.map((recording) => (
                <div
                  key={recording.id}
                  className={`rounded-lg border p-3 ${selected?.info.id === recording.id ? "border-primary bg-primary/5" : "border-border"}`}
                >
                  <button
                    type="button"
                    className="w-full text-left"
                    disabled={busyId !== null}
                    onClick={() => void selectRecording(recording)}
                  >
                    <div className="truncate font-semibold">{recording.host}</div>
                    <div className="mt-1 text-xs text-muted-foreground">
                      {new Date(recording.createdAt).toLocaleString()} · {recording.durationSeconds.toFixed(1)}s · {formatBytes(recording.sizeBytes)}
                    </div>
                  </button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="mt-2 text-destructive"
                    disabled={busyId !== null}
                    onClick={() => void removeRecording(recording)}
                  >
                    <Trash2 /> {t("Delete")}
                  </Button>
                </div>
              ))
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="gap-3">
            <CardTitle>{selected ? selected.info.host : t("Replay")}</CardTitle>
            <div className="flex flex-wrap items-center gap-2">
              <Button onClick={togglePlayback} disabled={!selected || eventsRef.current.length === 0}>
                {playing ? <Pause /> : <Play />} {t(playing ? "Pause" : "Play")}
              </Button>
              <Button
                variant="outline"
                onClick={() => selected && resetPlayer(parseAsciicast(selected.content))}
                disabled={!selected}
              >
                <RotateCcw /> {t("Restart")}
              </Button>
              <label className="ml-auto flex items-center gap-2 text-sm">
                {t("Speed")}
                <select
                  value={speed}
                  onChange={(event) => changeSpeed(Number(event.target.value))}
                  className="h-9 rounded-md border border-input bg-background px-2"
                >
                  {[0.5, 1, 2, 4].map((value) => (
                    <option key={value} value={value}>{value}×</option>
                  ))}
                </select>
              </label>
            </div>
            <div className="h-1.5 overflow-hidden rounded bg-muted">
              <div className="h-full bg-primary transition-[width]" style={{ width: `${progress * 100}%` }} />
            </div>
          </CardHeader>
          <CardContent>
            <div ref={containerRef} className="terminal-surface h-[470px] overflow-auto rounded-lg bg-[#0d1117] p-2" />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

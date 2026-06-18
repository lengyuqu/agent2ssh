import { Plus, TerminalSquare, X } from "lucide-react";
import { useState } from "react";
import { useI18n } from "../i18n";
import type { HostProfile } from "../types";
import TerminalView from "./TerminalView";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Select } from "./ui/select";
import { cn } from "../lib/utils";

type Props = {
  hosts: HostProfile[];
  initialHost?: string;
};

type Tab = { id: string; host: string };

let counter = 0;
function nextId(): string {
  counter += 1;
  return `term-${counter}-${Date.now()}`;
}

export default function TerminalPanel({ hosts, initialHost = "" }: Props) {
  const { t } = useI18n();
  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [newHost, setNewHost] = useState(initialHost || hosts[0]?.name || "");

  function openTab() {
    if (!newHost) return;
    const id = nextId();
    setTabs((prev) => [...prev, { id, host: newHost }]);
    setActiveId(id);
  }

  function closeTab(id: string) {
    setTabs((prev) => {
      const next = prev.filter((tab) => tab.id !== id);
      setActiveId((current) =>
        current === id ? (next.length > 0 ? next[next.length - 1].id : null) : current
      );
      return next;
    });
  }

  return (
    <Card className="flex h-[72vh] flex-col overflow-hidden p-0">
      <div className="flex items-center gap-1 overflow-x-auto border-b border-border bg-muted/40 px-2 py-1.5">
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveId(tab.id)}
            className={cn(
              "inline-flex shrink-0 items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs font-medium transition-colors",
              tab.id === activeId
                ? "border-primary/40 bg-primary/10 text-primary"
                : "border-transparent text-muted-foreground hover:bg-muted"
            )}
          >
            <TerminalSquare size={12} />
            <span className="max-w-[140px] truncate">{tab.host}</span>
            <span
              role="button"
              tabIndex={-1}
              onClick={(e) => {
                e.stopPropagation();
                closeTab(tab.id);
              }}
              className="rounded p-0.5 hover:bg-black/10 dark:hover:bg-white/10"
              aria-label={t("Close")}
            >
              <X size={11} />
            </span>
          </button>
        ))}
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          <Select
            value={newHost}
            onChange={(e) => setNewHost(e.target.value)}
            className="h-7 w-[150px] text-xs"
          >
            {hosts.length === 0 && <option value="">{t("No hosts")}</option>}
            {hosts.map((host) => (
              <option key={host.name} value={host.name}>
                {host.name}
              </option>
            ))}
          </Select>
          <Button size="sm" onClick={openTab} disabled={!newHost} className="h-7">
            <Plus size={13} />
            {t("New terminal")}
          </Button>
        </div>
      </div>

      <div className="relative flex-1 bg-[#0e1620]">
        {tabs.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-sm text-[#8fb0c5]">
            {t("Open a terminal to a host to get started.")}
          </div>
        )}
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className="absolute inset-0"
            style={{
              visibility: tab.id === activeId ? "visible" : "hidden",
              zIndex: tab.id === activeId ? 1 : 0,
            }}
          >
            <TerminalView host={tab.host} />
          </div>
        ))}
      </div>
    </Card>
  );
}

import { Edit3, Network, Plus, Save, Trash2, X } from "lucide-react";
import { FormEvent, useMemo, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, ProxyProfile, ProxyProtocol } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { Dialog } from "./ui/dialog";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

const emptyForm = {
  id: "",
  name: "",
  protocol: "http" as ProxyProtocol,
  host: "",
  port: 8080,
  username: "",
  password: "",
};

type Props = {
  proxies: ProxyProfile[];
  hosts: HostProfile[];
  onChanged: () => void | Promise<void>;
};

function proxyIdFromName(name: string): string {
  const id = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return id || `proxy-${Date.now()}`;
}

function formFromProxy(proxy: ProxyProfile) {
  return {
    id: proxy.id,
    name: proxy.name,
    protocol: proxy.protocol,
    host: proxy.host,
    port: proxy.port,
    username: proxy.username ?? "",
    password: proxy.password ?? "",
  };
}

export default function ProxyPanel({ proxies, hosts, onChanged }: Props) {
  const { t } = useI18n();
  const [form, setForm] = useState(emptyForm);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<ProxyProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const hostsByProxy = useMemo(() => {
    const counts = new Map<string, number>();
    for (const host of hosts) {
      if (host.proxy_id) {
        counts.set(host.proxy_id, (counts.get(host.proxy_id) ?? 0) + 1);
      }
    }
    return counts;
  }, [hosts]);

  function resetForm() {
    setForm(emptyForm);
    setEditingId(null);
    setError(null);
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setSaving(true);
    try {
      const id = editingId ?? proxyIdFromName(form.id || form.name);
      const payload: ProxyProfile = {
        id,
        name: form.name.trim(),
        protocol: form.protocol,
        host: form.host.trim(),
        port: Number(form.port),
        username: form.username.trim() || null,
        password: form.password || null,
      };
      await api.saveProxy(payload);
      resetForm();
      await onChanged();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(proxy: ProxyProfile) {
    setError(null);
    try {
      await api.deleteProxy(proxy.id);
      if (editingId === proxy.id) resetForm();
      setConfirmDelete(null);
      await onChanged();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="grid grid-cols-[minmax(0,1fr)_360px] items-start gap-[18px] max-lg:grid-cols-1">
      <Card className="space-y-4 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <Network size={16} className="text-muted-foreground" />
          {t("Proxy profiles")}
          <Badge variant="secondary" className="ml-1 font-medium">
            {t("{count} proxies", { count: proxies.length })}
          </Badge>
        </div>

        <div className="grid gap-2">
          {proxies.map((proxy) => {
            const assignedHosts = hostsByProxy.get(proxy.id) ?? 0;
            return (
              <div
                key={proxy.id}
                className="grid grid-cols-[minmax(0,1fr)_auto_auto] items-stretch overflow-hidden rounded-lg border border-border bg-card"
              >
                <div className="min-w-0 px-3 py-2.5">
                  <div className="flex flex-wrap items-center gap-2">
                    <strong className="truncate font-semibold">{proxy.name}</strong>
                    <Badge variant="secondary">{proxy.protocol.toUpperCase()}</Badge>
                    {assignedHosts > 0 && (
                      <span className="text-xs text-muted-foreground">
                        {t("{count} hosts", { count: assignedHosts })}
                      </span>
                    )}
                  </div>
                  <div className="mt-1 break-all text-xs text-muted-foreground">
                    {proxy.host}:{proxy.port}
                    {proxy.username && ` · ${t("auth enabled")}`}
                  </div>
                </div>
                <button
                  className="flex items-center justify-center border-l border-border px-3 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                  title={t("Edit {name}", { name: proxy.name })}
                  onClick={() => {
                    setEditingId(proxy.id);
                    setForm(formFromProxy(proxy));
                    setError(null);
                  }}
                >
                  <Edit3 size={14} />
                </button>
                <button
                  className="flex items-center justify-center border-l border-border px-3 text-muted-foreground transition-colors hover:bg-muted hover:text-destructive"
                  title={t("Delete {name}", { name: proxy.name })}
                  onClick={() => setConfirmDelete(proxy)}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            );
          })}
          {proxies.length === 0 && (
            <div className="px-3 py-3 text-sm text-muted-foreground">
              {t("No proxy profiles configured")}
            </div>
          )}
        </div>
      </Card>

      <Card className="space-y-4 p-4">
        <div className="flex items-center gap-2 font-semibold">
          <Plus size={16} className="text-muted-foreground" />
          {editingId ? t("Edit Proxy") : t("Add Proxy")}
          {editingId && (
            <IconButton className="ml-auto" type="button" title={t("Cancel")} onClick={resetForm}>
              <X size={14} />
            </IconButton>
          )}
        </div>

        {error && (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        <form className="grid gap-3.5" onSubmit={handleSubmit}>
          <label className={labelCls}>
            {t("Name")}
            <Input
              required
              value={form.name}
              onChange={(event) => setForm({ ...form, name: event.target.value })}
              placeholder="office-proxy"
            />
          </label>
          {!editingId && (
            <label className={labelCls}>
              {t("ID")}
              <Input
                value={form.id}
                onChange={(event) => setForm({ ...form, id: event.target.value })}
                placeholder={t("Generated from name")}
              />
            </label>
          )}
          <label className={labelCls}>
            {t("Protocol")}
            <Select
              value={form.protocol}
              onChange={(event) =>
                setForm({ ...form, protocol: event.target.value as ProxyProtocol })
              }
            >
              <option value="http">HTTP CONNECT</option>
              <option value="socks5">SOCKS5</option>
            </Select>
          </label>
          <div className="grid grid-cols-[1fr_110px] gap-2.5">
            <label className={labelCls}>
              {t("Proxy host")}
              <Input
                required
                value={form.host}
                onChange={(event) => setForm({ ...form, host: event.target.value })}
                placeholder="127.0.0.1"
              />
            </label>
            <label className={labelCls}>
              {t("Port")}
              <Input
                required
                type="number"
                min={1}
                max={65535}
                value={form.port}
                onChange={(event) => setForm({ ...form, port: Number(event.target.value) })}
              />
            </label>
          </div>
          <div className="grid grid-cols-2 gap-2.5 max-sm:grid-cols-1">
            <label className={labelCls}>
              {t("Username")}
              <Input
                value={form.username}
                onChange={(event) => setForm({ ...form, username: event.target.value })}
              />
            </label>
            <label className={labelCls}>
              {t("Password")}
              <Input
                type="password"
                value={form.password}
                onChange={(event) => setForm({ ...form, password: event.target.value })}
              />
            </label>
          </div>
          <Button variant="secondary" type="submit" className="w-full" disabled={saving}>
            <Save size={16} />
            {saving ? t("Saving...") : editingId ? t("Update proxy") : t("Save proxy")}
          </Button>
        </form>
      </Card>

      {confirmDelete && (
        <Dialog onClose={() => setConfirmDelete(null)} className="max-w-sm">
          <p className="mb-2">{t("Delete proxy {name}?", { name: confirmDelete.name })}</p>
          <p className="rounded-md bg-warning/10 px-2.5 py-2 text-sm text-warning">
            {t("Hosts using this proxy will switch back to direct connections.")}
          </p>
          <div className="mt-4 flex justify-end gap-2.5">
            <Button variant="secondary" onClick={() => setConfirmDelete(null)}>
              {t("Cancel")}
            </Button>
            <Button variant="destructive" onClick={() => handleDelete(confirmDelete)}>
              {t("Delete")}
            </Button>
          </div>
        </Dialog>
      )}
    </div>
  );
}

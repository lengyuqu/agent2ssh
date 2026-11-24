import { Edit3, Network, Plus, Save, Trash2 } from "lucide-react";
import { memo, FormEvent, useMemo, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import { labelCls } from "../lib/ui-classes";
import type { HostProfile, ProxyProfile, ProxyProtocol } from "../types";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { ConfirmDialog } from "./ui/dialog";
import { HostChipList } from "./ui/host-chips";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { SlideOverDrawer } from "./ui/slide-over-drawer";
import { EmptyState, InlineAlert } from "./ui/state";
import { useToast } from "./ui/toast";

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
  /** Jump to a host that uses this proxy. */
  onSelectHost?: (hostName: string) => void;
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

function ProxyPanel({ proxies, hosts, onChanged, onSelectHost }: Props) {
  const { t } = useI18n();
  const { showToast } = useToast();
  const [form, setForm] = useState(emptyForm);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<ProxyProfile | null>(null);
  const [saving, setSaving] = useState(false);

  const hostsByProxy = useMemo(() => {
    const names = new Map<string, string[]>();
    for (const host of hosts) {
      if (!host.proxy_id) continue;
      const list = names.get(host.proxy_id) ?? [];
      list.push(host.name);
      names.set(host.proxy_id, list);
    }
    return names;
  }, [hosts]);

  function resetForm() {
    setForm(emptyForm);
    setEditingId(null);
  }

  function closeDrawer() {
    setDrawerOpen(false);
    resetForm();
  }

  function openCreate() {
    resetForm();
    setDrawerOpen(true);
  }

  function openEdit(proxy: ProxyProfile) {
    setEditingId(proxy.id);
    setForm(formFromProxy(proxy));
    setDrawerOpen(true);
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
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
      closeDrawer();
      await onChanged();
    } catch (err) {
      showToast("error", String(err));
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(proxy: ProxyProfile) {
    try {
      await api.deleteProxy(proxy.id);
      if (editingId === proxy.id) closeDrawer();
      setConfirmDelete(null);
      await onChanged();
    } catch (err) {
      showToast("error", String(err));
    }
  }

  const affectedHosts = confirmDelete ? hostsByProxy.get(confirmDelete.id) ?? [] : [];

  return (
    <div>
      <Card className="space-y-4 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex items-center gap-2 font-semibold">
            <Network size={16} className="text-muted-foreground" />
            {t("Proxy profiles")}
            <Badge variant="secondary" className="font-medium">
              {t("{count} proxies", { count: proxies.length })}
            </Badge>
          </div>
          <Button className="ml-auto" onClick={openCreate}>
            <Plus size={14} />
            {t("Add Proxy")}
          </Button>
        </div>

        <div className="grid gap-2">
          {proxies.map((proxy) => {
            const assigned = hostsByProxy.get(proxy.id) ?? [];
            return (
              <div key={proxy.id} className="rounded-lg border border-border bg-card px-3 py-2.5">
                <div className="flex items-start gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <strong className="truncate font-semibold">{proxy.name}</strong>
                      <Badge variant="secondary">{proxy.protocol.toUpperCase()}</Badge>
                    </div>
                    <div className="mt-1 break-all text-xs text-muted-foreground">
                      {proxy.host}:{proxy.port}
                      {proxy.username && ` · ${t("auth enabled")}`}
                    </div>
                    <HostChipList
                      className="mt-2"
                      names={assigned}
                      onSelect={onSelectHost}
                      collapseAfter={5}
                    />
                  </div>
                  <IconButton
                    title={t("Edit {name}", { name: proxy.name })}
                    onClick={() => openEdit(proxy)}
                  >
                    <Edit3 size={14} />
                  </IconButton>
                  <IconButton
                    title={t("Delete {name}", { name: proxy.name })}
                    onClick={() => setConfirmDelete(proxy)}
                  >
                    <Trash2 size={14} />
                  </IconButton>
                </div>
              </div>
            );
          })}
          {proxies.length === 0 && (
            <EmptyState icon={Network} title={t("No proxy profiles configured")} />
          )}
        </div>
      </Card>

      <SlideOverDrawer
        open={drawerOpen}
        onClose={closeDrawer}
        title={editingId ? t("Edit Proxy") : t("Add Proxy")}
      >
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
          <Button variant="secondary" type="submit" block disabled={saving}>
            <Save size={16} />
            {saving ? t("Saving...") : editingId ? t("Update proxy") : t("Save proxy")}
          </Button>
        </form>
      </SlideOverDrawer>

      {confirmDelete && (
        <ConfirmDialog
          title={t("Delete proxy {name}?", { name: confirmDelete.name })}
          note={
            <div className="grid gap-2">
              <InlineAlert>
                {t("Hosts using this proxy will switch back to direct connections.")}
              </InlineAlert>
              {affectedHosts.length > 0 && (
                <InlineAlert tone="destructive" bordered>
                  {t("Affected hosts: {names}", { names: affectedHosts.join(", ") })}
                </InlineAlert>
              )}
            </div>
          }
          confirmLabel={t("Delete")}
          danger
          onConfirm={() => handleDelete(confirmDelete)}
          onCancel={() => setConfirmDelete(null)}
        />
      )}
    </div>
  );
}

export default memo(ProxyPanel);

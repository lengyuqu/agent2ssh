import { FileKey, Plus, X } from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { HostGroup, HostProfile, ProxyProfile, SshKeyInfo } from "../types";
import { Button } from "./ui/button";
import { Card } from "./ui/card";
import { IconButton } from "./ui/icon-button";
import { Input } from "./ui/input";
import { Select } from "./ui/select";

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

const emptyForm = {
  name: "",
  host: "",
  user: "",
  port: 22,
  auth_mode: "password" as "password" | "managed_key" | "manual_key",
  key_path: "",
  password: "",
  jump_host: "",
  proxy_id: "",
  tags: "",
  group: "default",
  env: "",
  role: "",
  owner: "",
};

type Props = {
  hosts: HostProfile[];
  groups: HostGroup[];
  proxies: ProxyProfile[];
  initialGroup: string;
  editingHost?: HostProfile | null;
  onCancelEdit?: () => void;
  onSaved: () => void;
};

function formFromHost(host: HostProfile) {
  const auth_mode = host.key_path ? "manual_key" : "password";
  return {
    name: host.name,
    host: host.host,
    user: host.user ?? "",
    port: host.port ?? 22,
    auth_mode: auth_mode as "password" | "managed_key" | "manual_key",
    key_path: host.key_path ?? "",
    password: host.password ?? "",
    jump_host: host.jump_host ?? "",
    proxy_id: host.proxy_id ?? "",
    tags: (host.tags ?? []).join(", "),
    group: host.group || "default",
    env: host.env ?? "",
    role: host.role ?? "",
    owner: host.owner ?? "",
  };
}

export default function AddHostForm({
  hosts,
  groups,
  proxies,
  initialGroup,
  editingHost,
  onCancelEdit,
  onSaved,
}: Props) {
  const { t } = useI18n();
  const [form, setForm] = useState(emptyForm);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);
  const isEditing = Boolean(editingHost);

  useEffect(() => {
    api.listKeys().then(setKeys).catch(() => setKeys([]));
  }, []);

  useEffect(() => {
    setForm(editingHost ? formFromHost(editingHost) : { ...emptyForm, group: initialGroup || "default" });
  }, [editingHost, initialGroup]);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const tags = form.tags
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    const hostPayload: HostProfile = {
      name: form.name.trim(),
      host: form.host.trim(),
      user: form.user.trim() || null,
      port: form.port || null,
      key_path: form.auth_mode === "password" ? null : form.key_path.trim() || null,
      password: form.auth_mode === "password" ? form.password || null : null,
      jump_host: form.jump_host.trim() || null,
      proxy_id: form.proxy_id.trim() || null,
      tags,
      group: form.group || "default",
      env: form.env.trim() || null,
      role: form.role.trim() || null,
      owner: form.owner.trim() || null,
    };
    if (editingHost) {
      await api.updateHost(editingHost.name, hostPayload);
    } else {
      await api.addHost(hostPayload);
    }
    setForm(emptyForm);
    onCancelEdit?.();
    onSaved();
  }

  const otherHosts = hosts.filter((h) => h.name !== (editingHost?.name ?? form.name));

  return (
    <Card className="space-y-4 p-4">
      <div className="flex items-center gap-2 font-semibold">
        <Plus size={16} className="text-muted-foreground" />
        {isEditing ? t("Edit Host") : t("Add Host")}
        {isEditing && (
          <IconButton className="ml-auto" type="button" title={t("Cancel")} onClick={onCancelEdit}>
            <X size={14} />
          </IconButton>
        )}
      </div>
      <form className="grid gap-3.5" onSubmit={handleSubmit}>
        <label className={labelCls}>
          {t("Alias")}
          <Input
            required
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="prod"
          />
        </label>
        <label className={labelCls}>
          {t("Host")}
          <Input
            required
            value={form.host}
            onChange={(e) => setForm({ ...form, host: e.target.value })}
            placeholder="10.0.0.12"
          />
        </label>
        <div className="grid grid-cols-[1fr_110px] gap-2.5">
          <label className={labelCls}>
            {t("User")}
            <Input
              value={form.user}
              onChange={(e) => setForm({ ...form, user: e.target.value })}
              placeholder="ubuntu"
            />
          </label>
          <label className={labelCls}>
            {t("Port")}
            <Input
              type="number"
              min={1}
              max={65535}
              value={form.port}
              onChange={(e) => setForm({ ...form, port: Number(e.target.value) })}
            />
          </label>
        </div>
        <label className={labelCls}>
          {t("Authentication")}
          <Select
            value={form.auth_mode}
            onChange={(e) =>
              setForm({
                ...form,
                auth_mode: e.target.value as typeof form.auth_mode,
                key_path: "",
                password: "",
              })
            }
          >
            <option value="password">{t("Password")}</option>
            <option value="managed_key">{t("Managed key")}</option>
            <option value="manual_key">{t("Manual key path")}</option>
          </Select>
        </label>

        {form.auth_mode === "managed_key" && (
          <label className={labelCls}>
            {t("SSH Key")}
            <Select
              value={form.key_path}
              onChange={(e) => setForm({ ...form, key_path: e.target.value })}
            >
              <option value="">{t("Select a key")}</option>
              {keys.map((k) => (
                <option key={k.name} value={k.private_path}>
                  {k.name} ({k.key_type})
                </option>
              ))}
            </Select>
          </label>
        )}

        {form.auth_mode === "manual_key" && (
          <label className={labelCls}>
            {t("Manual key path")}
            <Input
              value={form.key_path}
              onChange={(e) => setForm({ ...form, key_path: e.target.value })}
              placeholder="~/.ssh/id_ed25519"
            />
          </label>
        )}

        {form.auth_mode === "password" && (
          <label className={labelCls}>
            {t("SSH Password")}
            <Input
              type="password"
              value={form.password}
              onChange={(e) => setForm({ ...form, password: e.target.value })}
              placeholder={t("Password")}
            />
            <span className="text-xs font-normal leading-snug text-muted-foreground">
              {t("Password is stored locally in the Agent2SSH config. Exec, ping, SFTP, jump hosts, sessions, terminals, and tunnels use the embedded SSH backend; system ssh/scp/sshpass are not required for SSH transport.")}
            </span>
          </label>
        )}
        <label className={labelCls}>
          {t("Group")}
          <Select
            value={form.group}
            onChange={(e) => setForm({ ...form, group: e.target.value })}
          >
            {groups.map((group) => (
              <option key={group.id} value={group.id}>
                {group.name}
              </option>
            ))}
          </Select>
        </label>
        <label className={labelCls}>
          {t("Tags (comma-separated)")}
          <Input
            value={form.tags}
            onChange={(e) => setForm({ ...form, tags: e.target.value })}
            placeholder="production, web, staging"
          />
        </label>
        <div className="grid grid-cols-3 gap-2.5">
          <label className={labelCls}>
            {t("Env")}
            <Input
              value={form.env}
              onChange={(e) => setForm({ ...form, env: e.target.value })}
              placeholder="prod"
            />
          </label>
          <label className={labelCls}>
            {t("Role")}
            <Input
              value={form.role}
              onChange={(e) => setForm({ ...form, role: e.target.value })}
              placeholder="web"
            />
          </label>
          <label className={labelCls}>
            {t("Owner")}
            <Input
              value={form.owner}
              onChange={(e) => setForm({ ...form, owner: e.target.value })}
              placeholder="platform"
            />
          </label>
        </div>
        <label className={labelCls}>
          {t("Jump host (bastion)")}
          <Select
            value={form.jump_host}
            onChange={(e) => setForm({ ...form, jump_host: e.target.value })}
          >
            <option value="">{t("None")}</option>
            {otherHosts.map((h) => (
              <option key={h.name} value={h.name}>
                {h.name} ({h.host})
              </option>
            ))}
          </Select>
        </label>
        <label className={labelCls}>
          {t("Proxy")}
          <Select
            value={form.proxy_id}
            onChange={(e) => setForm({ ...form, proxy_id: e.target.value })}
          >
            <option value="">{t("Direct connection")}</option>
            {proxies.map((proxy) => (
              <option key={proxy.id} value={proxy.id}>
                {proxy.name} ({proxy.protocol} {proxy.host}:{proxy.port})
              </option>
            ))}
          </Select>
        </label>
        <Button variant="secondary" type="submit" className="w-full">
          <FileKey size={16} />
          {isEditing ? t("Update host") : t("Save host")}
        </Button>
      </form>
    </Card>
  );
}

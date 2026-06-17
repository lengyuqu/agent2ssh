import { FileKey, Plus } from "lucide-react";
import { FormEvent, useEffect, useState } from "react";
import { api } from "../api";
import { useI18n } from "../i18n";
import type { HostProfile, SshKeyInfo } from "../types";

const emptyForm = {
  name: "",
  host: "",
  user: "",
  port: 22,
  auth_mode: "password" as "password" | "managed_key" | "manual_key",
  key_path: "",
  password: "",
  jump_host: "",
  tags: "",
  env: "",
  role: "",
  owner: "",
};

type Props = {
  hosts: HostProfile[];
  onSaved: () => void;
};

export default function AddHostForm({ hosts, onSaved }: Props) {
  const { t } = useI18n();
  const [form, setForm] = useState(emptyForm);
  const [keys, setKeys] = useState<SshKeyInfo[]>([]);

  useEffect(() => {
    api.listKeys().then(setKeys).catch(() => setKeys([]));
  }, []);

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const tags = form.tags
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    await api.addHost({
      name: form.name.trim(),
      host: form.host.trim(),
      user: form.user.trim() || null,
      port: form.port || null,
      key_path: form.auth_mode === "password" ? null : form.key_path.trim() || null,
      password: form.auth_mode === "password" ? form.password || null : null,
      jump_host: form.jump_host.trim() || null,
      tags,
      env: form.env.trim() || null,
      role: form.role.trim() || null,
      owner: form.owner.trim() || null,
    });
    setForm(emptyForm);
    onSaved();
  }

  const otherHosts = hosts.filter((h) => h.name !== form.name);

  return (
    <section className="panel">
      <div className="panel-title">
        <Plus size={16} />
        {t("Add Host")}
      </div>
      <form className="host-form" onSubmit={handleSubmit}>
        <label>
          {t("Alias")}
          <input
            required
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="prod"
          />
        </label>
        <label>
          {t("Host")}
          <input
            required
            value={form.host}
            onChange={(e) => setForm({ ...form, host: e.target.value })}
            placeholder="10.0.0.12"
          />
        </label>
        <div className="two-col">
          <label>
            {t("User")}
            <input
              value={form.user}
              onChange={(e) => setForm({ ...form, user: e.target.value })}
              placeholder="ubuntu"
            />
          </label>
          <label>
            {t("Port")}
            <input
              type="number"
              min={1}
              max={65535}
              value={form.port}
              onChange={(e) =>
                setForm({ ...form, port: Number(e.target.value) })
              }
            />
          </label>
        </div>
        <label>
          {t("Authentication")}
          <select
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
          </select>
        </label>

        {form.auth_mode === "managed_key" && (
          <label>
            {t("SSH Key")}
            <select
              value={form.key_path}
              onChange={(e) => setForm({ ...form, key_path: e.target.value })}
            >
              <option value="">{t("Select a key")}</option>
              {keys.map((k) => (
                <option key={k.name} value={k.private_path}>
                  {k.name} ({k.key_type})
                </option>
              ))}
            </select>
          </label>
        )}

        {form.auth_mode === "manual_key" && (
          <label>
            {t("Manual key path")}
          <input
            value={form.key_path}
            onChange={(e) => setForm({ ...form, key_path: e.target.value })}
            placeholder="~/.ssh/id_ed25519"
          />
          </label>
        )}

        {form.auth_mode === "password" && (
          <label>
            {t("SSH Password")}
            <input
              type="password"
              value={form.password}
              onChange={(e) => setForm({ ...form, password: e.target.value })}
              placeholder={t("Password")}
            />
            <span className="field-hint">
              {t("Password is stored locally in the Agent2SSH config. Direct exec, ping, and SFTP use the embedded SSH client; jump hosts, sessions, and tunnels still use the OpenSSH fallback.")}
            </span>
          </label>
        )}
        <label>
          {t("Tags (comma-separated)")}
          <input
            value={form.tags}
            onChange={(e) => setForm({ ...form, tags: e.target.value })}
            placeholder="production, web, staging"
          />
        </label>
        <div className="three-col">
          <label>
            {t("Env")}
            <input
              value={form.env}
              onChange={(e) => setForm({ ...form, env: e.target.value })}
              placeholder="prod"
            />
          </label>
          <label>
            {t("Role")}
            <input
              value={form.role}
              onChange={(e) => setForm({ ...form, role: e.target.value })}
              placeholder="web"
            />
          </label>
          <label>
            {t("Owner")}
            <input
              value={form.owner}
              onChange={(e) => setForm({ ...form, owner: e.target.value })}
              placeholder="platform"
            />
          </label>
        </div>
        <label>
          {t("Jump host (bastion)")}
          <select
            value={form.jump_host}
            onChange={(e) => setForm({ ...form, jump_host: e.target.value })}
          >
            <option value="">{t("None")}</option>
            {otherHosts.map((h) => (
              <option key={h.name} value={h.name}>
                {h.name} ({h.host})
              </option>
            ))}
          </select>
        </label>
        <button className="secondary" type="submit">
          <FileKey size={16} />
          {t("Save host")}
        </button>
      </form>
    </section>
  );
}

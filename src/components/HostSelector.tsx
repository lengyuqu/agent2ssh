import { Server } from "lucide-react";
import { useEffect } from "react";
import { useI18n } from "../i18n";
import type { HostProfile } from "../types";
import { Select } from "./ui/select";

type Props = {
  hosts: HostProfile[];
  value: string;
  onChange: (value: string) => void;
  label?: string;
  disabled?: boolean;
};

const labelCls = "grid gap-1.5 text-sm font-medium text-foreground/90";

function describeHost(host: HostProfile): string {
  const endpoint = `${host.user ? `${host.user}@` : ""}${host.host}:${host.port ?? 22}`;
  return `${host.name} - ${endpoint}`;
}

export default function HostSelector({
  hosts,
  value,
  onChange,
  label,
  disabled,
}: Props) {
  const { t } = useI18n();

  useEffect(() => {
    if (hosts.length === 0) {
      if (value) onChange("");
      return;
    }
    if (!value || !hosts.some((host) => host.name === value)) {
      onChange(hosts[0].name);
    }
  }, [hosts, onChange, value]);

  return (
    <label className={labelCls}>
      <span className="inline-flex items-center gap-1.5">
        <Server size={14} className="text-muted-foreground" />
        {label ?? t("Target server")}
      </span>
      <Select value={value} onChange={(event) => onChange(event.target.value)} disabled={disabled || hosts.length === 0}>
        {hosts.length === 0 && <option value="">{t("No hosts")}</option>}
        {hosts.map((host) => (
          <option key={host.name} value={host.name}>
            {describeHost(host)}
          </option>
        ))}
      </Select>
    </label>
  );
}

import { Server } from "lucide-react";
import { useEffect } from "react";
import { useI18n } from "../i18n";
import { labelCls } from "../lib/ui-classes";
import type { HostProfile } from "../types";
import { Select } from "./ui/select";

/** How the control is framed where it sits. */
type Shell =
  /** `<label>` plus a Server mark — the settings-panel shape. */
  | "label"
  /** `<label>` with plain text — for a field already under its own heading. */
  | "plain"
  /** Just the `<select>` — for toolbars that place the control themselves. */
  | "bare";

type Props = {
  hosts: HostProfile[];
  value: string;
  onChange: (value: string) => void;
  label?: string;
  disabled?: boolean;
  /**
   * Whether `""` is a legal value.
   *
   * Defaults to `false`: the control must always hold a real host, so it
   * defaults to the first one and disables itself while there are none. `true`
   * says the caller is offering "none" — an empty value is left alone, and the
   * control stays usable with an empty host list.
   */
  allowEmpty?: boolean;
  /**
   * A blank entry that is always offered — `None`, `Direct connection`. Omit it
   * and the only blank entry is the placeholder shown while there are no hosts
   * at all, which is not an answer the user can choose.
   */
  emptyOption?: string;
  /** One option's text. Defaults to `name - user@host:port`. */
  optionLabel?: (host: HostProfile) => string;
  shell?: Shell;
  /** Forwarded to the `<select>` itself, for callers that size it. */
  selectClassName?: string;
};

function describeHost(host: HostProfile): string {
  const endpoint = `${host.user ? `${host.user}@` : ""}${host.host}:${host.port ?? 22}`;
  return `${host.name} - ${endpoint}`;
}

/**
 * Pick one host — or, where the caller offers it, none.
 *
 * Four panels hand-rolled this select against the same `hosts` array and had
 * drifted: three option texts, two framing shapes, and a disagreement on the
 * part that matters. `ForwardPanel`'s jump host and `AddHostForm`'s bastion are
 * both optional, so an empty value has to survive as a real answer, while the
 * settings panels must never leave a stale host name in the field. `allowEmpty`
 * carries that distinction, and `emptyOption` separates a blank entry the user
 * may choose from the placeholder shown when there is nothing to choose.
 */
export default function HostSelector({
  hosts,
  value,
  onChange,
  label,
  disabled,
  allowEmpty = false,
  emptyOption,
  optionLabel = describeHost,
  shell = "label",
  selectClassName,
}: Props) {
  const { t } = useI18n();

  useEffect(() => {
    if (hosts.length === 0) {
      if (value) onChange("");
      return;
    }
    if (hosts.some((host) => host.name === value)) return;
    // The value is not selectable any more. When an empty value is legal it is
    // the right fallback — that is what "none" means. When it is not, the
    // control has to hold something real, so take the first host.
    const next = allowEmpty ? "" : hosts[0].name;
    if (next !== value) onChange(next);
  }, [hosts, onChange, value, allowEmpty]);

  const select = (
    <Select
      value={value}
      onChange={(event) => onChange(event.target.value)}
      disabled={disabled || (hosts.length === 0 && !allowEmpty)}
      className={selectClassName}
    >
      {emptyOption !== undefined && <option value="">{emptyOption}</option>}
      {emptyOption === undefined && hosts.length === 0 && (
        <option value="">{t("No hosts")}</option>
      )}
      {hosts.map((host) => (
        <option key={host.name} value={host.name}>
          {optionLabel(host)}
        </option>
      ))}
    </Select>
  );

  if (shell === "bare") return select;

  const text = label ?? t("Target server");

  return (
    <label className={labelCls}>
      {shell === "label" ? (
        <span className="inline-flex items-center gap-1.5">
          <Server size={14} className="text-muted-foreground" />
          {text}
        </span>
      ) : (
        text
      )}
      {select}
    </label>
  );
}

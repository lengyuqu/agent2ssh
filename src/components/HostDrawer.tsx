import type { ReactNode } from "react";
import { useI18n } from "../i18n";
import { SlideOverDrawer } from "./ui/slide-over-drawer";

type HostDrawerProps = {
  open: boolean;
  onClose: () => void;
  /** Set while editing an existing host. Omitted (or null) while creating one. */
  editingHostName?: string | null;
  children: ReactNode;
};

/** Right slide-over for add/edit host. The form stays owned by App via children. */
export default function HostDrawer({
  open,
  onClose,
  editingHostName,
  children,
}: HostDrawerProps) {
  const { t } = useI18n();
  const title = editingHostName
    ? t("Edit host: {name}", { name: editingHostName })
    : t("Add Host");

  return (
    <SlideOverDrawer
      open={open}
      onClose={onClose}
      title={title}
      overlayTestId="host-drawer-overlay"
      titleId="host-drawer-title"
    >
      {children}
    </SlideOverDrawer>
  );
}

import { Languages } from "lucide-react";
import { useI18n } from "../i18n";
import { cn } from "../lib/utils";

export default function LanguageSwitcher() {
  const { language, setLanguage, t } = useI18n();

  const btn = (active: boolean) =>
    cn(
      "rounded px-2 py-1 text-sm leading-none transition-colors",
      active ? "bg-primary text-primary-foreground" : "text-foreground hover:bg-muted"
    );

  return (
    <div
      className="inline-flex items-center gap-1 rounded-md border border-input bg-card px-1.5 py-1 text-foreground"
      aria-label={t("Language")}
    >
      <Languages size={15} className="text-muted-foreground" />
      <button type="button" className={btn(language === "en")} onClick={() => setLanguage("en")}>
        EN
      </button>
      <button type="button" className={btn(language === "zh")} onClick={() => setLanguage("zh")}>
        {t("中文")}
      </button>
    </div>
  );
}

import { Languages } from "lucide-react";
import { useI18n } from "../i18n";

export default function LanguageSwitcher() {
  const { language, setLanguage, t } = useI18n();

  return (
    <div className="language-switcher" aria-label={t("Language")}>
      <Languages size={15} />
      <button
        type="button"
        className={language === "en" ? "active" : ""}
        onClick={() => setLanguage("en")}
      >
        EN
      </button>
      <button
        type="button"
        className={language === "zh" ? "active" : ""}
        onClick={() => setLanguage("zh")}
      >
        中文
      </button>
    </div>
  );
}

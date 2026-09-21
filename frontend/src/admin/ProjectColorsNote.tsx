import { Lock } from "lucide-react";
import { useI18n } from "../i18n";
import { projectAppearanceText } from "./project-appearance-i18n";
import "./ProjectColorsNote.css";

/** Explains that a color preference follows the project's settings. */
export function ProjectColorsNote({ id, className }: { id?: string; className: string }) {
  const { locale } = useI18n();
  return <p className={`project-colors-note ${className}`} id={id}>
    <Lock size={12} aria-hidden="true" />{projectAppearanceText(locale)("setByProject")}
  </p>;
}

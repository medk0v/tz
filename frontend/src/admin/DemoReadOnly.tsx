import { createContext, useContext, type ComponentProps } from "react";
import { I18nContext } from "../i18n";
import "./DemoReadOnly.css";

export const DemoReadOnlyContext = createContext(false);

export function useDemoReadOnly() {
  return useContext(DemoReadOnlyContext);
}

export function DemoActionButton({ disabled, title, ...props }: ComponentProps<"button">) {
  const readOnly = useDemoReadOnly();
  const i18n = useContext(I18nContext);
  return <button {...props} disabled={readOnly || disabled} title={readOnly ? (i18n?.t("demo.readonlyAction") ?? "This action is unavailable in the demo account.") : title} />;
}

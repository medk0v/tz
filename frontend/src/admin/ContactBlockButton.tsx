import { DemoActionButton } from "./DemoReadOnly";
import { useState } from "react";
import { Ban, ShieldCheck } from "lucide-react";
import { setContactBlocked, type OperatorAuth } from "../api";
import { useI18n } from "../i18n";

export function ContactBlockButton({ auth, contactId, blocked, onUpdated, compact = false }: {
  auth: OperatorAuth;
  contactId: string;
  blocked: boolean;
  onUpdated: (blocked: boolean) => void;
  compact?: boolean;
}) {
  const { t } = useI18n();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(false);
  async function toggle() {
    if (saving) return;
    setSaving(true);
    setError(false);
    try {
      const result = await setContactBlocked(auth, contactId, !blocked);
      onUpdated(result.is_blocked);
    } catch {
      setError(true);
    } finally {
      setSaving(false);
    }
  }
  return <div className="contact-block-control">
    <DemoActionButton type="button" className={compact ? "table-link" : "secondary-button"} disabled={saving} onClick={() => void toggle()}>
      {blocked ? <ShieldCheck size={15} aria-hidden="true" /> : <Ban size={15} aria-hidden="true" />}
      {t(saving ? "contacts.blockSaving" : blocked ? "contacts.unblock" : "contacts.block")}
    </DemoActionButton>
    {error && <p className="field-error" role="alert">{t("contacts.blockError")}</p>}
  </div>;
}

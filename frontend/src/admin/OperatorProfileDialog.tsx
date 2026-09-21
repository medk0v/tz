import { DemoActionButton } from "./DemoReadOnly";
import { useEffect, useState } from "react";
import { Save, X } from "lucide-react";
import {
  updateOperatorChatProfile,
  uploadOperatorAvatar,
  type OperatorAuth,
  type OperatorChatProfile,
} from "../api";
import { useI18n } from "../i18n";
import { AvatarUploadField } from "./AvatarUploadField";

interface OperatorProfileDialogProps {
  auth: OperatorAuth;
  operatorId: string;
  displayName: string;
  avatarUrl: string | null;
  mode: "self" | "managed";
  onClose: () => void;
  onSaved: (profile: OperatorChatProfile) => void;
}

export function OperatorProfileDialog({
  auth,
  operatorId,
  displayName: initialDisplayName,
  avatarUrl,
  mode,
  onClose,
  onSaved,
}: OperatorProfileDialogProps) {
  const { t } = useI18n();
  const [displayName, setDisplayName] = useState(initialDisplayName);
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [avatarRemoved, setAvatarRemoved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !saving) onClose();
    };
    document.addEventListener("keydown", closeOnEscape);
    return () => document.removeEventListener("keydown", closeOnEscape);
  }, [onClose, saving]);

  async function save(event: React.FormEvent) {
    event.preventDefault();
    if (!displayName.trim() || saving) return;
    setSaving(true);
    setError(false);
    try {
      let saved = await updateOperatorChatProfile(auth, operatorId, {
        display_name: displayName.trim(),
        avatar_url: avatarRemoved ? null : avatarUrl,
      });
      if (avatarFile) {
        saved = await uploadOperatorAvatar(auth, operatorId, avatarFile);
      }
      onSaved(saved);
    } catch {
      setError(true);
    } finally {
      setSaving(false);
    }
  }

  return (
    <div
      className="profile-dialog-backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !saving) onClose();
      }}
    >
      <section className="profile-dialog" role="dialog" aria-modal="true" aria-labelledby="operator-profile-title">
        <header>
          <div>
            <h2 id="operator-profile-title">{t(mode === "self" ? "profile.selfTitle" : "profile.managedTitle")}</h2>
            <p>{t(mode === "self" ? "profile.selfDescription" : "profile.managedDescription")}</p>
          </div>
          <button className="icon-button" type="button" aria-label={t("profile.close")} disabled={saving} onClick={onClose}>
            <X size={17} />
          </button>
        </header>
        <form onSubmit={save}>
          {error && <div className="admin-notice admin-notice--error" role="alert">{t("profile.saveError")}</div>}
          <label>
            <span>{t("profile.displayName")}</span>
            <input
              autoFocus
              value={displayName}
              maxLength={200}
              disabled={saving}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </label>
          <AvatarUploadField
            displayName={displayName}
            avatarUrl={avatarUrl}
            file={avatarFile}
            removed={avatarRemoved}
            disabled={saving}
            onFileChange={(file) => {
              setAvatarFile(file);
              setAvatarRemoved(false);
            }}
            onRemove={() => {
              setAvatarFile(null);
              setAvatarRemoved(true);
            }}
          />
          <div className="profile-dialog-actions">
            <button className="secondary-button" type="button" disabled={saving} onClick={onClose}>
              {t("profile.cancel")}
            </button>
            <DemoActionButton className="primary-button" type="submit" disabled={saving || !displayName.trim()}>
              <Save size={16} />
              {saving ? t("profile.saving") : t("profile.save")}
            </DemoActionButton>
          </div>
        </form>
      </section>
    </div>
  );
}

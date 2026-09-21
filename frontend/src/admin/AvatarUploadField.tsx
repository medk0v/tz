import { DemoActionButton, useDemoReadOnly } from "./DemoReadOnly";
import { useEffect, useId, useMemo, useState } from "react";
import type { MessageKey } from "../i18n";
import { useI18n } from "../i18n";
import { OperatorAvatar } from "./OperatorAvatar";

export const MAX_AVATAR_UPLOAD_BYTES = 2 * 1_024 * 1_024;
const ACCEPTED_AVATAR_TYPES = new Set(["image/jpeg", "image/png", "image/webp"]);

interface AvatarUploadFieldProps {
  displayName: string;
  avatarUrl: string | null;
  label?: string;
  file: File | null;
  removed: boolean;
  disabled?: boolean;
  onFileChange: (file: File) => void;
  onRemove: () => void;
}

export function AvatarUploadField({
  displayName,
  avatarUrl,
  label,
  file,
  removed,
  disabled = false,
  onFileChange,
  onRemove,
}: AvatarUploadFieldProps) {
  const demoReadOnly = useDemoReadOnly();
  const { t } = useI18n();
  const inputId = useId();
  const [error, setError] = useState<MessageKey | null>(null);
  const previewUrl = useMemo(
    () => file && typeof URL.createObjectURL === "function" ? URL.createObjectURL(file) : null,
    [file],
  );

  useEffect(() => () => {
    if (previewUrl && typeof URL.revokeObjectURL === "function") URL.revokeObjectURL(previewUrl);
  }, [previewUrl]);

  const visibleAvatarUrl = file ? previewUrl : removed ? null : avatarUrl;
  const canRemove = file !== null || (!removed && avatarUrl !== null);
  const avatarLabel = label ?? t("profile.avatar");

  return (
    <div className="avatar-upload-field">
      <div className="operator-profile-preview">
        <OperatorAvatar
          displayName={displayName || "?"}
          avatarUrl={visibleAvatarUrl}
          size="large"
        />
        <div>
          <strong>{displayName || t("profile.namePlaceholder")}</strong>
          <span>{file?.name ?? t(visibleAvatarUrl ? "profile.avatarSelected" : "profile.avatarInitials")}</span>
        </div>
      </div>
      <label htmlFor={inputId}>
        <span>{avatarLabel}</span>
        <input
          id={inputId}
          type="file"
          aria-label={avatarLabel}
          accept="image/jpeg,image/png,image/webp,.jpg,.jpeg,.png,.webp"
          disabled={demoReadOnly || disabled}
          onChange={(event) => {
            const nextFile = event.currentTarget.files?.[0];
            if (!nextFile) return;
            if (nextFile.size > MAX_AVATAR_UPLOAD_BYTES) {
              setError("profile.avatarTooLarge");
              event.currentTarget.value = "";
              return;
            }
            if (nextFile.type && !ACCEPTED_AVATAR_TYPES.has(nextFile.type)) {
              setError("profile.avatarInvalidFile");
              event.currentTarget.value = "";
              return;
            }
            setError(null);
            onFileChange(nextFile);
          }}
        />
        <small>{t("profile.avatarHelp")}</small>
      </label>
      {canRemove && (
        <DemoActionButton className="secondary-button avatar-remove-button" type="button" disabled={disabled} onClick={onRemove}>
          {t("profile.removeAvatar")}
        </DemoActionButton>
      )}
      {error && <div className="field-error" role="alert">{t(error)}</div>}
    </div>
  );
}

import { useState } from "react";
import { resolveAvatarUrl } from "../api";

interface OperatorAvatarProps {
  displayName: string;
  avatarUrl: string | null;
  size?: "small" | "large";
}

function initials(displayName: string): string {
  const parts = displayName.trim().split(/\s+/).filter(Boolean);
  return parts
    .slice(0, 2)
    .map((part) => Array.from(part)[0] ?? "")
    .join("")
    .toLocaleUpperCase() || "?";
}

export function OperatorAvatar({
  displayName,
  avatarUrl,
  size = "small",
}: OperatorAvatarProps) {
  const [failedAvatarUrl, setFailedAvatarUrl] = useState<string | null>(null);
  const resolvedAvatarUrl = resolveAvatarUrl(avatarUrl);

  return (
    <span className={`operator-avatar operator-avatar--${size}`} aria-hidden="true">
      {resolvedAvatarUrl && failedAvatarUrl !== resolvedAvatarUrl ? (
        <img
          src={resolvedAvatarUrl}
          alt=""
          referrerPolicy="no-referrer"
          onError={() => setFailedAvatarUrl(resolvedAvatarUrl)}
        />
      ) : (
        initials(displayName)
      )}
    </span>
  );
}

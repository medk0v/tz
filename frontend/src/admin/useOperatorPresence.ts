import { useEffect } from "react";
import { refreshOperatorPresence, type OperatorAuth } from "../api";

const OPERATOR_HEARTBEAT_INTERVAL_MS = 20_000;

export function useOperatorPresence(auth: OperatorAuth | null) {
  useEffect(() => {
    if (!auth) return undefined;
    let cancelled = false;

    const heartbeat = () => {
      if (cancelled) return;
      void refreshOperatorPresence(auth).catch(() => {
        // Presence is best effort and retries without interrupting the workspace.
      });
    };
    const refreshWhenVisible = () => {
      if (document.visibilityState === "visible") heartbeat();
    };

    heartbeat();
    const timer = window.setInterval(heartbeat, OPERATOR_HEARTBEAT_INTERVAL_MS);
    window.addEventListener("online", heartbeat);
    document.addEventListener("visibilitychange", refreshWhenVisible);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
      window.removeEventListener("online", heartbeat);
      document.removeEventListener("visibilitychange", refreshWhenVisible);
    };
  }, [auth]);
}

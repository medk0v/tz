import { useEffect, useRef } from "react";
import {
  createOperatorRealtimeTicket,
  openRealtimeSocket,
  type OperatorAuth,
  type RealtimeEvent,
} from "../api";

const RECONNECT_DELAY_MS = 2_000;
const RETRY_DELAY_MS = 5_000;
const CONNECT_TIMEOUT_MS = 10_000;

export function useOperatorRealtime(
  auth: OperatorAuth | null,
  inboxId: string | null,
  onEvent: (event: RealtimeEvent) => void,
  onSync: () => void,
) {
  const onEventRef = useRef(onEvent);
  const onSyncRef = useRef(onSync);

  useEffect(() => {
    onEventRef.current = onEvent;
  }, [onEvent]);

  useEffect(() => {
    onSyncRef.current = onSync;
  }, [onSync]);

  useEffect(() => {
    if (!auth || !inboxId) {
      return undefined;
    }

    let cancelled = false;
    let socket: WebSocket | undefined;
    let reconnectTimer: number | undefined;
    let syncTimer: number | undefined;
    let handshakeTimer: number | undefined;
    let ticketController: AbortController | undefined;
    let connecting = false;
    let generation = 0;

    const hasUsableSocket = () => socket
      && socket.readyState !== WebSocket.CLOSING
      && socket.readyState !== WebSocket.CLOSED;

    const requestSync = () => {
      if (cancelled || syncTimer !== undefined) return;
      syncTimer = window.setTimeout(() => {
        syncTimer = undefined;
        if (!cancelled) onSyncRef.current();
      }, 0);
    };

    const scheduleReconnect = (delay: number) => {
      if (cancelled || reconnectTimer !== undefined) return;
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = undefined;
        void connect();
      }, delay);
    };

    const connect = async () => {
      if (cancelled || connecting || hasUsableSocket()) return;
      connecting = true;
      const currentGeneration = ++generation;
      const controller = new AbortController();
      ticketController = controller;
      const ticketTimeout = window.setTimeout(() => controller.abort(), CONNECT_TIMEOUT_MS);
      try {
        const ticket = await createOperatorRealtimeTicket(auth, inboxId, controller.signal);
        if (cancelled || currentGeneration !== generation) {
          return;
        }
        let nextHandshakeTimer: number | undefined;
        const clearHandshakeTimer = () => {
          if (nextHandshakeTimer === undefined) return;
          window.clearTimeout(nextHandshakeTimer);
          if (handshakeTimer === nextHandshakeTimer) handshakeTimer = undefined;
          nextHandshakeTimer = undefined;
        };
        const nextSocket = openRealtimeSocket(
          ticket,
          (event) => {
            if (
              cancelled
              || currentGeneration !== generation
              || socket !== nextSocket
            ) return;
            onEventRef.current(event);
          },
          () => {
            clearHandshakeTimer();
            if (cancelled || currentGeneration !== generation) return;
            if (socket === nextSocket) socket = undefined;
            scheduleReconnect(RECONNECT_DELAY_MS);
          },
          () => {
            clearHandshakeTimer();
            if (cancelled || currentGeneration !== generation) return;
            requestSync();
          },
        );
        socket = nextSocket;
        nextHandshakeTimer = window.setTimeout(() => {
          const expiredTimer = nextHandshakeTimer;
          nextHandshakeTimer = undefined;
          if (handshakeTimer === expiredTimer) handshakeTimer = undefined;
          if (
            cancelled
            || currentGeneration !== generation
            || nextSocket.readyState === WebSocket.OPEN
          ) return;
          generation += 1;
          if (socket === nextSocket) socket = undefined;
          nextSocket.close();
          scheduleReconnect(RECONNECT_DELAY_MS);
        }, CONNECT_TIMEOUT_MS);
        handshakeTimer = nextHandshakeTimer;
      } catch {
        if (!cancelled && currentGeneration === generation) {
          scheduleReconnect(RETRY_DELAY_MS);
        }
      } finally {
        window.clearTimeout(ticketTimeout);
        if (ticketController === controller) ticketController = undefined;
        if (currentGeneration === generation) connecting = false;
      }
    };

    const recover = () => {
      requestSync();
      if (hasUsableSocket() || connecting) return;
      if (reconnectTimer !== undefined) {
        window.clearTimeout(reconnectTimer);
        reconnectTimer = undefined;
      }
      void connect();
    };
    const recoverWhenVisible = () => {
      if (document.visibilityState === "visible") recover();
    };

    void connect();
    window.addEventListener("focus", recover);
    window.addEventListener("online", recover);
    document.addEventListener("visibilitychange", recoverWhenVisible);
    return () => {
      cancelled = true;
      generation += 1;
      ticketController?.abort();
      if (reconnectTimer !== undefined) {
        window.clearTimeout(reconnectTimer);
      }
      if (syncTimer !== undefined) {
        window.clearTimeout(syncTimer);
      }
      if (handshakeTimer !== undefined) {
        window.clearTimeout(handshakeTimer);
      }
      socket?.close();
      window.removeEventListener("focus", recover);
      window.removeEventListener("online", recover);
      document.removeEventListener("visibilitychange", recoverWhenVisible);
    };
  }, [auth, inboxId]);
}

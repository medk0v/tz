import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { useOperatorRealtime } from "./useOperatorRealtime";

vi.mock("../api", () => ({
  createOperatorRealtimeTicket: vi.fn(),
  openRealtimeSocket: vi.fn(),
}));

const auth: api.OperatorAuth = { kind: "session" };
const inboxId = "00000000-0000-4000-8000-000000000004";

interface SocketHarness {
  socket: WebSocket;
  setReadyState: (readyState: number) => void;
  onEvent: (event: api.RealtimeEvent) => void;
  onClose?: () => void;
  onOpen?: () => void;
}

describe("useOperatorRealtime", () => {
  const sockets: SocketHarness[] = [];

  beforeEach(() => {
    vi.mocked(api.createOperatorRealtimeTicket).mockResolvedValue("ticket");
    vi.mocked(api.openRealtimeSocket).mockImplementation(
      (_ticket, onEvent, onClose, onOpen) => {
        let readyState: number = WebSocket.CONNECTING;
        const socket = {
          get readyState() {
            return readyState;
          },
          close: vi.fn(() => {
            readyState = WebSocket.CLOSED;
          }),
        } as unknown as WebSocket;
        sockets.push({
          socket,
          setReadyState: (value) => { readyState = value; },
          onEvent,
          onClose,
          onOpen,
        });
        return socket;
      },
    );
  });

  afterEach(() => {
    cleanup();
    sockets.length = 0;
    vi.clearAllMocks();
    vi.useRealTimers();
  });

  it("resynchronizes on open and focus without reconnecting for callback changes", async () => {
    const firstEvent = vi.fn();
    const firstSync = vi.fn();
    const secondEvent = vi.fn();
    const secondSync = vi.fn();
    const rendered = renderHook(
      ({ onEvent, onSync }) => useOperatorRealtime(auth, inboxId, onEvent, onSync),
      { initialProps: { onEvent: firstEvent, onSync: firstSync } },
    );

    await waitFor(() => expect(sockets).toHaveLength(1));
    sockets[0].setReadyState(WebSocket.OPEN);
    act(() => sockets[0].onOpen?.());
    await waitFor(() => expect(firstSync).toHaveBeenCalledOnce());

    rendered.rerender({ onEvent: secondEvent, onSync: secondSync });
    expect(api.openRealtimeSocket).toHaveBeenCalledOnce();

    const event: api.RealtimeEvent = {
      event_id: "00000000-0000-4000-8000-000000000020",
      inbox_id: inboxId,
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000021",
      data: {},
    };
    act(() => sockets[0].onEvent(event));
    expect(secondEvent).toHaveBeenCalledWith(event);
    expect(firstEvent).not.toHaveBeenCalled();

    act(() => window.dispatchEvent(new Event("focus")));
    await waitFor(() => expect(secondSync).toHaveBeenCalledOnce());
    expect(api.openRealtimeSocket).toHaveBeenCalledOnce();
  });

  it("resynchronizes after reconnect and ignores a stale close callback", async () => {
    vi.useFakeTimers();
    const onEvent = vi.fn();
    const onSync = vi.fn();
    renderHook(() => useOperatorRealtime(auth, inboxId, onEvent, onSync));

    await act(async () => { await Promise.resolve(); });
    expect(sockets).toHaveLength(1);
    sockets[0].setReadyState(WebSocket.OPEN);
    act(() => {
      sockets[0].onOpen?.();
      vi.runOnlyPendingTimers();
    });
    expect(onSync).toHaveBeenCalledOnce();

    sockets[0].setReadyState(WebSocket.CLOSED);
    act(() => sockets[0].onClose?.());
    await act(async () => {
      vi.advanceTimersByTime(2_000);
      await Promise.resolve();
    });
    expect(sockets).toHaveLength(2);

    sockets[1].setReadyState(WebSocket.OPEN);
    act(() => {
      sockets[1].onOpen?.();
      vi.runOnlyPendingTimers();
    });
    expect(onSync).toHaveBeenCalledTimes(2);

    const event: api.RealtimeEvent = {
      event_id: "00000000-0000-4000-8000-000000000030",
      inbox_id: inboxId,
      type: "message.created",
      aggregate_id: "00000000-0000-4000-8000-000000000031",
      data: {},
    };
    act(() => sockets[0].onEvent(event));
    expect(onEvent).not.toHaveBeenCalled();
    act(() => sockets[1].onEvent(event));
    expect(onEvent).toHaveBeenCalledWith(event);

    act(() => sockets[0].onClose?.());
    await act(async () => {
      vi.advanceTimersByTime(2_000);
      await Promise.resolve();
    });
    expect(sockets).toHaveLength(2);
  });
});

// A private, bounded JSON-lines channel between the Gateway and offline browser.
export const MAX_FRAME_BYTES = 12 * 1024 * 1024;

export function browserChannel(socket, onMessage) {
  let pending = Buffer.alloc(0);
  socket.on("data", (chunk) => {
    pending = Buffer.concat([pending, chunk]);
    for (;;) {
      const end = pending.indexOf(10);
      if (end < 0) break;
      if (end > MAX_FRAME_BYTES) return socket.destroy();
      const line = pending.subarray(0, end);
      pending = pending.subarray(end + 1);
      try {
        const message = JSON.parse(line.toString("utf8"));
        Promise.resolve(onMessage(message)).catch(() => socket.destroy());
      } catch { socket.destroy(); return; }
    }
    if (pending.length > MAX_FRAME_BYTES) socket.destroy();
  });
  return (message) => {
    const line = JSON.stringify(message);
    if (Buffer.byteLength(line) > MAX_FRAME_BYTES) throw new Error("browser_response_too_large");
    if (!socket.destroyed) socket.write(`${line}\n`);
  };
}

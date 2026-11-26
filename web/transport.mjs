// Browser-side datagrams for the shared Dare session. The caller owns the
// Session's monotonic clock and passes inbound bytes to handle_datagram().
export async function connectZone(endpoint, onDatagram, onDisconnect) {
  if (typeof WebTransport === "undefined") {
    throw new Error("This browser does not support WebTransport.");
  }

  const transport = new WebTransport(endpoint, { requireUnreliable: true });
  void transport.closed.catch(() => {});
  let serverIp;
  let boundPort;

  try {
    await transport.ready;
    const streams = transport.incomingUnidirectionalStreams.getReader();
    try {
      const { value: stream, done } = await streams.read();
      if (done) throw new Error("The gateway did not send its UDP endpoint.");
      const reader = stream.getReader();
      try {
        const bytes = new Uint8Array(6);
        let offset = 0;
        while (true) {
          const { value, done: finished } = await reader.read();
          if (finished) break;
          if (offset + value.length > bytes.length) throw new Error("Invalid UDP endpoint.");
          bytes.set(value, offset);
          offset += value.length;
        }
        if (offset !== bytes.length) throw new Error("Incomplete UDP endpoint.");
        const fields = new DataView(bytes.buffer);
        serverIp = fields.getUint32(0);
        boundPort = fields.getUint16(4);
      } finally {
        reader.releaseLock();
      }
    } finally {
      streams.releaseLock();
    }
  } catch (error) {
    transport.close();
    throw error;
  }

  const reader = transport.datagrams.readable.getReader();
  const writer = transport.datagrams.writable.getWriter();
  let closed = false;
  let pending = 0;

  function disconnect(error) {
    if (closed) return;
    close();
    onDisconnect(error);
  }

  function close() {
    if (closed) return;
    closed = true;
    writer.releaseLock();
    transport.close();
  }

  async function receive() {
    try {
      while (!closed) {
        const { value, done } = await reader.read();
        if (done) break;
        onDatagram(value);
      }
    } catch (error) {
      disconnect(error);
      return;
    } finally {
      reader.releaseLock();
    }
    disconnect();
  }
  void receive();

  return {
    serverIp,
    boundPort,
    // Bound the write backlog. The shared session retransmits reliable messages.
    send(datagram) {
      if (closed || pending >= 16) return false;
      if (datagram.byteLength === 0 || datagram.byteLength > 520) return false;
      pending++;
      void writer.write(datagram.slice()).catch(disconnect).finally(() => pending--);
      return true;
    },
    close,
  };
}

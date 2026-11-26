"""HTTP/3 WebTransport bridge between a browser and one configured game zone."""

import argparse
import asyncio
import ipaddress
import struct
import time
from contextlib import suppress

from aioquic.asyncio import QuicConnectionProtocol, serve
from aioquic.h3.connection import H3_ALPN, H3Connection
from aioquic.h3.events import DatagramReceived, HeadersReceived
from aioquic.quic.configuration import QuicConfiguration
from aioquic.quic.events import ConnectionTerminated, ProtocolNegotiated, StreamReset

MAX_PACKET = 520
MAX_SESSIONS = 64
IDLE_SECONDS = 45
PACKETS_PER_SECOND = 250
BURST = 64


class ZoneSocket(asyncio.DatagramProtocol):
    def __init__(self, forward):
        self.forward = forward

    def datagram_received(self, data, addr):
        if 0 < len(data) <= MAX_PACKET:
            self.forward(data)


class Gateway(QuicConnectionProtocol):
    active_sessions = 0

    def __init__(self, *args, origin, zone, **kwargs):
        super().__init__(*args, **kwargs)
        self.origin = origin.encode()
        self.zone = zone
        self.http = None
        self.stream = None
        self.udp = None
        self.opening = None
        self.expiry = None
        self.tokens = BURST
        self.updated = time.monotonic()

    def quic_event_received(self, event):
        if isinstance(event, ProtocolNegotiated) and event.alpn_protocol in H3_ALPN:
            self.http = H3Connection(self._quic, enable_webtransport=True)
        if isinstance(event, ConnectionTerminated):
            self.close_session()
            return
        if isinstance(event, StreamReset) and event.stream_id == self.stream:
            self.close_session()
        if self.http is None:
            return
        for message in self.http.handle_event(event):
            if isinstance(message, HeadersReceived):
                self.on_headers(message)
            elif (
                isinstance(message, DatagramReceived)
                and message.stream_id == self.stream
                and self.udp is not None
            ):
                self.to_zone(message.data)

    def on_headers(self, message):
        stream = message.stream_id
        if stream == self.stream:
            if message.stream_ended:
                self.close_session()
            return
        headers = dict(message.headers)
        if (
            headers.get(b":method") != b"CONNECT"
            or headers.get(b":protocol") != b"webtransport"
            or headers.get(b":path") != b"/play/transport"
        ):
            self.http.send_headers(stream, [(b":status", b"404")], end_stream=True)
        elif headers.get(b"origin") != self.origin:
            self.http.send_headers(stream, [(b":status", b"403")], end_stream=True)
        elif message.stream_ended or self.stream is not None or Gateway.active_sessions >= MAX_SESSIONS:
            self.http.send_headers(stream, [(b":status", b"429")], end_stream=True)
        else:
            self.stream = stream
            Gateway.active_sessions += 1
            self.opening = asyncio.create_task(self.open_zone(stream))

    async def open_zone(self, stream):
        loop = asyncio.get_running_loop()
        try:
            udp, _ = await loop.create_datagram_endpoint(
                lambda: ZoneSocket(self.to_browser), remote_addr=self.zone
            )
        except OSError:
            if self.stream == stream:
                self.http.send_headers(stream, [(b":status", b"502")], end_stream=True)
                self.opening = None
                self.close_session()
                self.transmit()
            return
        if self.stream != stream:
            udp.close()
            return
        self.udp = udp
        self.opening = None
        self.refresh_expiry()
        self.http.send_headers(
            stream, [(b":status", b"200"), (b"sec-webtransport-http3-draft", b"draft02")]
        )
        # Report the game endpoint before the WASM session constructs its login.
        # This is a reliable, server-initiated WebTransport stream.
        endpoint = self.http.create_webtransport_stream(stream, is_unidirectional=True)
        local_port = udp.get_extra_info("sockname")[1]
        address = ipaddress.ip_address(self.zone[0])
        server_ip = int(address) if address.version == 4 else 0
        self._quic.send_stream_data(endpoint, struct.pack("!IH", server_ip, local_port), end_stream=True)
        self.transmit()

    def refresh_expiry(self):
        if self.expiry is not None:
            self.expiry.cancel()
        self.expiry = asyncio.get_running_loop().call_later(IDLE_SECONDS, self.expire)

    def expire(self):
        self.close_session()
        self.close()

    def close_session(self):
        if self.expiry is not None:
            self.expiry.cancel()
            self.expiry = None
        if self.opening is not None:
            self.opening.cancel()
            self.opening = None
        if self.udp is not None:
            self.udp.close()
            self.udp = None
        if self.stream is not None:
            self.stream = None
            Gateway.active_sessions -= 1

    def to_zone(self, data):
        if not 0 < len(data) <= MAX_PACKET:
            return
        now = time.monotonic()
        self.tokens = min(BURST, self.tokens + (now - self.updated) * PACKETS_PER_SECOND)
        self.updated = now
        if self.tokens >= 1:
            self.tokens -= 1
            self.udp.sendto(data)
            self.refresh_expiry()

    def to_browser(self, data):
        if self.stream is not None and self.udp is not None:
            self.http.send_datagram(self.stream, data)
            self.transmit()


async def main(args):
    config = QuicConfiguration(
        is_client=False, alpn_protocols=H3_ALPN, max_datagram_frame_size=65536,
        idle_timeout=IDLE_SECONDS,
    )
    config.load_cert_chain(args.cert, args.key)
    server = await serve(
        args.bind, args.port, configuration=config, retry=True,
        create_protocol=lambda *a, **kw: Gateway(
            *a, origin=args.origin, zone=(args.zone_ip, args.zone_port), **kw
        ),
    )
    try:
        await asyncio.Future()
    finally:
        server.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=4443)
    parser.add_argument("--origin", default="https://subspacefoundation.com")
    parser.add_argument("--zone-ip", type=lambda value: str(ipaddress.ip_address(value)), default="127.0.0.1")
    parser.add_argument("--zone-port", type=int, default=5000)
    parser.add_argument("--cert", required=True)
    parser.add_argument("--key", required=True)
    with suppress(KeyboardInterrupt):
        asyncio.run(main(parser.parse_args()))

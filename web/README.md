# Foundation browser transport

This is the transport slice of the Dare browser port. It is a fixed-destination
WebTransport-to-UDP gateway and a browser adapter. The WASM client and WebGPU
canvas still need to be integrated.

## Run against a test zone

Install `gateway/requirements.txt` in a Python virtual environment. Bind the
gateway to an address that browsers can reach over UDP, using a TLS certificate
for the gateway hostname. The default loopback bind is for local testing.

```sh
python web/gateway/gateway.py --cert /path/to/fullchain.pem --key /path/to/privkey.pem --zone-ip 127.0.0.1 --zone-port 5000
```

Serve the eventual browser build from Foundation's HTTPS origin. Point the browser
adapter at the gateway's HTTPS URL ending in `/play/transport`. The gateway needs
an HTTP/3 UDP listener. An ordinary HTTP reverse proxy to a TCP backend does not
forward these sessions.

The gateway sends a six-byte endpoint record on a reliable unidirectional stream
before gameplay. The browser adapter returns the configured zone IPv4 address
and the UDP source port. Pass those to `Session::set_local_endpoint` before
starting the game session. Inbound datagrams go to `Session::handle_datagram`.
Drive `tick` with the monotonic clock and drain `poll_transmit` into the adapter.
Use an explicitly VIE-capable private test listener for the first integration.
A future automatic cipher fallback must open a fresh gateway session so it gets
a new UDP source port, matching the native driver.

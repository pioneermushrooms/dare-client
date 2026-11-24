# Foundation browser transport

This prototype includes a fixed-destination WebTransport-to-UDP gateway,
a browser adapter, and a WASM spectator probe with a WebGPU position plot.
The desktop renderer and flight controls are outside this prototype.

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

## Build the spectator probe

The probe uses the shared Rust session in WASM and draws received player
positions on a WebGPU canvas. It enters as a spectator and has no flight controls.

The build was verified with upstream `oss-core` commit
`aad700a4cedb272c607050492b9db8b0f6c0149f` in the sibling checkout,
Rust 1.100.0, and wasm-bindgen CLI 0.2.130.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
CARGO_TARGET_DIR=web/session/target cargo build --locked --release \
  --target wasm32-unknown-unknown --manifest-path web/session/Cargo.toml
wasm-bindgen --target web --out-dir web/pkg \
  web/session/target/wasm32-unknown-unknown/release/dare_web_session.wasm
```

Serve `index.html`, `probe.mjs`, `transport.mjs`, and `pkg/` over HTTPS. The
probe expects `https://subspacefoundation.com:4443/play/transport`. The current
loopback-only gateway and its test certificate are suitable for on-host checks,
and a browser on another host needs a reachable endpoint and trusted certificate.

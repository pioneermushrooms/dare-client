# Foundation browser transport

This prototype includes a fixed-destination WebTransport-to-UDP gateway,
a browser adapter, and a WASM flight prototype with a WebGPU map and
keyboard flight and weapons. It uses simplified visual geometry.

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

## Build the flight prototype

The probe uses the shared Rust session in WASM and draws received player
positions, tiles, and projectiles on a WebGPU canvas. Arrow keys steer and
thrust, X fires, and Z drops bombs. Use a private test arena. A blank arena name joins the server's default public arena.

The build was verified with the Dare `oss-core` commit
`eae56a0016474ef9883eb74d47ba8864f019f5e8` in the sibling checkout,
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

## Build the shared Dare client

Run `web/build-client.sh <output-directory>` with the same `oss-core` revision,
Rust WASM target, and wasm-bindgen CLI listed above. The output uses Dare's
winit input, shared renderer, asset decoders, bundled skins, and audio mixer.
Serve only that output at `/play/`; `web/index.html` is the older test probe.
The browser client uses the Foundation gateway on UDP 4443. Leave the arena
field empty on the Dare login screen to join the public arena.

`web/install-foundation.sh <output-directory>` is an initial installer for the
Foundation host. It expects the staged Python gateway, the IP on a local
interface, and the current Foundation TLS certificate in Caddy's storage.
The public listener uses the host IP, leaving the separate loopback listener
available for testing. UDP 4443 must be permitted by the host and provider
firewalls. Restart the public gateway when Caddy renews its TLS certificate.

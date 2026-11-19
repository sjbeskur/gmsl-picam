# iris-client

Reusable, asynchronous Rust client for `irisd`. The `iris` CLI uses this library;
web backends and browser WebAssembly applications can use the same typed methods.
There is no dependency on libcamera, the daemon package, Axum, or Clap.

```rust,no_run
use iris_client::{IrisClient, SettingsPatch};

async fn adjust_camera() -> Result<(), iris_client::ClientError> {
    let camera = IrisClient::new("udp://127.0.0.1:8081")?;
    camera.start().await?;
    camera.set_settings(&SettingsPatch {
        exposure_us: Some(59000),
        gain: Some(1.5),
        ..Default::default()
    }).await?;
    let status = camera.status().await?;
    println!("{:?}", status.camera.latest_frame);
    Ok(())
}
```

`ping().await` sends a versioned UDP command and waits for its matching reply.
Native clients retry identical requests for up to 12 seconds. A timeout reports
an unknown outcome; query status before making a dependent decision. Commands
return requested settings separately from actual measured frame metadata.
Patches are validated before transmission, and daemon errors retain their
invalid/conflict/unavailable distinctions.

`photon-protocol` owns the command, reply, metadata, and settings types. `iris-api`
is a compatibility re-export. The camera daemon depends on the protocol, while
this library selects the transport and keeps CLI parsing out of the protocol.

Default features enable `udp` and `http`. Native backends that do not need HTTP
can avoid Reqwest entirely:

```toml
iris-client = { path = "../iris-client", default-features = false, features = ["udp", "config"] }
```

## Saved native profiles

Enable the optional `config` feature for native TOML persistence:

```toml
iris-client = { path = "../iris-client", features = ["config"] }
```

`iris_client::config::{ClientConfig, CameraProfile, default_config_path}` provides
load/save, validation, named profile lookup, and client construction with saved endpoints. The CLI uses this same implementation:

```bash
iris config front --endpoint udp://192.168.50.150:8081
iris camera front start
iris camera front stop
```

Profiles live in `~/.config/iris/config.toml` (or `$XDG_CONFIG_HOME/iris/config.toml`).
`iris show` displays them; `--config` selects another file. Saves are
atomic and private on Unix and preserve other profiles. Legacy token-file
references are ignored and removed on the next save. Browser callers use the transport directly without the
native filesystem module. A profile name identifies a connection to the one camera
currently owned by its daemon.

## Direct remote control

```rust,no_run
# async fn example() -> Result<(), iris_client::ClientError> {
let camera = iris_client::IrisClient::new("udp://192.168.50.150:8081")?;
camera.ping().await?;
let status = camera.status().await?;
# Ok(()) }
```

The home-development daemon accepts remote UDP commands without credentials.
See the [wire format and retry limits](../../docs/irisd-service.md#udp-commands-and-image-postcards).

## Web applications

- Rust backend: call async methods from its existing Tokio runtime. Configure a
  UDP endpoint to talk directly to `irisd`, or `IrisClient::with_http_client`
  when using the optional HTTP gateway.
- Browser WebAssembly: `cargo check -p iris-client --target wasm32-unknown-unknown`
  verifies the browser build. reqwest uses Fetch in this target. Pass an absolute
  URL for a same-origin backend proxy, e.g. `https://app.example/api/iris`.
- The base URL may include a proxy prefix. The client preserves that prefix for
  every API route. The proxy must forward `X-Iris-Control: 1` and enforce the web
  application's authentication/access policy.
- `iris-view` exposes the HTTP routes and forwards commands to the daemon over UDP.
  Saved HTTP profiles remain supported. The gateway has no general CORS routes.
  A browser UI and authenticated proxy are separate application work, not supplied
  by this crate.
- The default endpoint is `udp://127.0.0.1:8081`. Native HTTP endpoints use
  direct HTTP without TLS/proxy features and a 12-second timeout. For a native HTTPS backend, enable a TLS feature on the application's
  reqwest 0.12 dependency and pass its configured client. Browser HTTPS uses the
  browser's transport.

Tests cover typed response/patch serialization, reverse-proxy prefixes, mutation
headers, daemon errors, and invalid input without network access. Run:

```bash
cargo test -p iris-client
```

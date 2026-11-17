# iris-client

Reusable, asynchronous Rust client for `irisd`. The `iris` CLI uses this library;
web backends and browser WebAssembly applications can use the same typed methods.
There is no dependency on libcamera, the daemon package, Axum, or Clap.

```rust,no_run
use iris_client::{IrisClient, SettingsPatch};

async fn adjust_camera() -> Result<(), iris_client::ClientError> {
    let camera = IrisClient::new("http://127.0.0.1:8081")?;
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

`ping().await` checks the daemon with an empty HTTP 204 response. Control
methods return `DaemonStatus`, with requested settings separate from measured
frame metadata. Setting acceptance is not hardware confirmation: inspect actual
exposure/gain and camera state after subsequent frames. Errors distinguish invalid
input, transport/decoding failures, and daemon responses with HTTP status/message.
The library attaches the required mutation header and validates patches before
serialization (including rejection of non-finite gain values).

`iris-api` owns shared `CameraSettings`, `SettingsPatch`, and status/metadata types
plus validation. These types are re-exported here for callers. The daemon depends
on the shared types, not on the HTTP client. CLI-only argument parsing stays in
the `iris` executable.

## Saved native profiles

Enable the optional `config` feature for native TOML persistence:

```toml
iris-client = { path = "../iris-client", features = ["config"] }
```

`iris_client::config::{ClientConfig, CameraProfile, default_config_path}` provides
load/save, validation, named profile lookup, and client construction with saved
credential references. The CLI uses this same implementation:

```bash
iris config front --endpoint http://192.168.50.150:8081 --token-file ~/.config/iris/token
iris camera front start
iris camera front stop
```

Profiles live in `~/.config/iris/config.toml` (or `$XDG_CONFIG_HOME/iris/config.toml`).
`iris show` displays them; `--config` selects another file. Saves are
atomic and private on Unix, preserve other profiles, and store token file paths
rather than token values. Browser callers use the transport directly without the
native filesystem module. A profile name identifies a connection to the one camera
currently owned by its daemon.

## Direct remote control

```rust,no_run
# async fn example(token: &str) -> Result<(), iris_client::ClientError> {
let camera = iris_client::IrisClient::new("http://192.168.50.150:8081")?
    .with_token(token)?;
let status = camera.status().await?;
# Ok(()) }
```

Remote status and mutations require the daemon's bearer token. The library sends
it automatically and redacts it from Debug output. Local loopback clients may
omit it. Obtain credentials through the installation procedure in the
[service guide](../../docs/irisd-service.md).

## Web applications

- Rust backend: call async methods from its existing Tokio runtime. Configure a
  different transport with `IrisClient::with_http_client` when needed.
- Browser WebAssembly: `cargo check -p iris-client --target wasm32-unknown-unknown`
  verifies the browser build. reqwest uses Fetch in this target. Pass an absolute
  URL for a same-origin backend proxy, e.g. `https://app.example/api/iris`.
- The base URL may include a proxy prefix. The client preserves that prefix for
  every API route. The proxy must forward `X-Iris-Control: 1` and enforce the web
  application's authentication/access policy.
- The daemon requires a token for direct remote TCP control; it has no browser CORS routes.
  A browser UI and authenticated proxy are separate application work, not supplied
  by this crate.
- Native defaults use direct HTTP without TLS/proxy features and a 12-second
  timeout. For a native HTTPS backend, enable a TLS feature on the application's
  reqwest 0.12 dependency and pass its configured client. Browser HTTPS uses the
  browser's transport.

Tests cover typed response/patch serialization, reverse-proxy prefixes, mutation
headers, daemon errors, and invalid input without network access. Run:

```bash
cargo test -p iris-client
```

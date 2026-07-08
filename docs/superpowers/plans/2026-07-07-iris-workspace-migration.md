# Iris Workspace Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the `iris` Rust app out of `rust/` into a conventional workspace layout at the repo root, then trim shell scripts so `just` and Cargo are the primary developer entry points.

**Architecture:** Make the repository own a top-level Cargo workspace with a single Rust crate for `iris`. Keep the libcamera build and Pi deployment as external inputs, not vendored source. After the workspace move is stable, reduce bespoke shell wrappers to the minimum needed for cross-build orchestration and preserve the current working commands through `just`.

**Tech Stack:** Rust workspace, Cargo, `just`, Bash, Docker BuildKit.

---

### Task 1: Move The Rust App Into A Root Workspace

**Files:**
- Add: `Cargo.toml`
- Add: `Cargo.lock`
- Add: `crates/iris/Cargo.toml`
- Add: `crates/iris/README.md`
- Add: `crates/iris/src/lib.rs`
- Add: `crates/iris/src/bin/iris.rs`
- Add: `crates/iris/src/camera.rs`
- Add: `crates/iris/src/frame_store.rs`
- Add: `crates/iris/src/http.rs`
- Add: `crates/iris/src/jpeg.rs`
- Add: `crates/iris/src/timing.rs`
- Add: `crates/iris/examples/libcamera_capture.rs`
- Add: `crates/iris/examples/gstreamer_capture.rs`
- Delete: `rust/Cargo.toml`
- Delete: `rust/Cargo.lock`
- Delete: `rust/README.md`
- Delete: `rust/src/lib.rs`
- Delete: `rust/src/bin/iris.rs`
- Delete: `rust/src/camera.rs`
- Delete: `rust/src/frame_store.rs`
- Delete: `rust/src/http.rs`
- Delete: `rust/src/jpeg.rs`
- Delete: `rust/src/timing.rs`
- Delete: `rust/examples/libcamera_capture.rs`
- Delete: `rust/examples/gstreamer_capture.rs`

- [ ] **Step 1: Write the workspace manifest**

Create `Cargo.toml` at the repo root:

```toml
[workspace]
members = ["crates/iris"]
resolver = "2"
```

Create `crates/iris/Cargo.toml` by moving the existing `rust/Cargo.toml` contents under the new path unchanged except for relative paths:

```toml
[package]
name = "gmsl-picam-rs"
version = "0.1.0"
edition = "2021"

[features]
default = []
http-support = ["dep:axum", "dep:serde", "dep:serde_json"]
jpeg-support = ["dep:image"]
iris-service = ["http-support", "jpeg-support", "dep:clap", "dep:tokio", "dep:tracing-subscriber", "dep:libcamera"]
libcamera-example = ["dep:clap", "dep:libcamera", "jpeg-support"]
gstreamer-example = ["dep:clap", "dep:gstreamer", "dep:gstreamer-app", "dep:glib"]

[dependencies]
anyhow = "1"
axum = { version = "0.7", optional = true }
clap = { version = "4", features = ["derive"], optional = true }
gstreamer = { version = "0.23", optional = true }
gstreamer-app = { version = "0.23", optional = true }
glib = { version = "0.20", optional = true }
image = { version = "0.25", default-features = false, features = ["jpeg"], optional = true }
libcamera = { version = "0.7", optional = true }
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
tokio = { version = "1", features = ["full"], optional = true }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"], optional = true }

[[bin]]
name = "iris"
path = "src/bin/iris.rs"
required-features = ["iris-service"]

[[example]]
name = "libcamera_capture"
path = "examples/libcamera_capture.rs"
required-features = ["libcamera-example"]

[[example]]
name = "gstreamer_capture"
path = "examples/gstreamer_capture.rs"
required-features = ["gstreamer-example"]
```

- [ ] **Step 2: Move the Rust source tree**

Move the current Rust files into `crates/iris/src` and `crates/iris/examples` without changing their contents. Keep the module layout intact so the code compiles before any cleanup.

- [ ] **Step 3: Update the root build entry points**

Change `justfile` and Docker/package scripts to point Cargo at `crates/iris` instead of `rust/`.

Run:

```bash
cargo test --manifest-path crates/iris/Cargo.toml --no-default-features
just build-rust
```

Expected: the workspace resolves from the repo root and the same binaries still build.

- [ ] **Step 4: Commit the workspace move**

```bash
git add Cargo.toml crates/iris justfile docker/rust-arm64.Dockerfile scripts/build-rust-arm64.sh
git commit -m "refactor: move iris into cargo workspace"
```

### Task 2: Trim Scripts Around The New Layout

**Files:**
- Modify: `justfile`
- Modify: `scripts/build-rust-arm64.sh`
- Modify: `docker/rust-arm64.Dockerfile`
- Modify: `docs/superpowers/plans/2026-07-05-iris-camera-service.md`
- Modify: `docs/superpowers/specs/2026-07-05-iris-camera-service-design.md`

- [ ] **Step 1: Replace redundant shell glue with `just` recipes**

Keep only the shell logic needed for the cross-build. Move repo-root-relative path handling into `just` and keep deployment commands as thin wrappers.

- [ ] **Step 2: Update run/deploy help text**

Make the build script and `just` recipes describe the new workspace path and the `iris --camera-index` startup usage from the moved crate.

- [ ] **Step 3: Verify the repo still cross-builds**

Run:

```bash
just --list
just build-rust
```

Expected: the workspace recipes are listed from the repo root and the arm64 build still exports `iris`.

- [ ] **Step 4: Commit the cleanup**

```bash
git add justfile scripts/build-rust-arm64.sh docker/rust-arm64.Dockerfile docs/superpowers/plans/2026-07-05-iris-camera-service.md docs/superpowers/specs/2026-07-05-iris-camera-service-design.md
git commit -m "build: trim rust workspace scripts"
```

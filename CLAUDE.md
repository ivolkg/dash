# dash

Qt5/C++14 infotainment head unit for Raspberry Pi, being migrated to Rust
module by module. Plan and rationale: `docs/OXIDIZING.md`.

## Git

- Commit messages are one line. No body, no trailers, no Co-Authored-By,
  no AI attribution anywhere in commits or PRs.
- Branches: `develop` is upstream main. `oxidizing` holds the assessment
  docs. `dev-rs` is the Rust migration branch; all Rust work lands there.
- Never force-push `develop`.

## Layout

- `src/`, `include/`: C++ app. Pages in `app/pages`, widgets in
  `app/widgets`, core state in `session.cpp` and `arbiter.cpp`.
- `plugins/`: Qt plugins (brightness, launcher, radio, vehicle).
- `rust/`: Cargo workspace. One crate per migration step under
  `rust/crates/`. C++ consumes crates through cxx.
- `install.sh`: builds aasdk, openauto and dash from source. Slow.

## Rust checks

Run from `rust/` before pushing to `dev-rs`; CI runs the same:

```
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

CI also runs `cross test --target aarch64-unknown-linux-gnu` (Pi target).
Clippy warnings fail the build.

## Testing on a PC

Android Auto needs a real phone over USB, not a Pi. CAN uses `vcan0`,
ELM327 uses a `socat` pty pair, camera uses GStreamer `videotestsrc`.
Details in `docs/OXIDIZING.md`.

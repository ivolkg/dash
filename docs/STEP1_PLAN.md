# Step 1: Pi-fidelity test environment and baseline tests for dash as it is

## Context

Dash is being migrated to Rust module by module on `dev-rs` (plan in
`docs/OXIDIZING.md`). Before any module moves, we need a repeatable way to
build and run the existing C++ app in an environment that matches the
Raspberry Pi as closely as a PC can, and a baseline set of tests that pin
down current behaviour so every later step can prove it changed nothing.

Facts that shape the design:

- The only Pi is a manual "fire test" device running DietPi (Debian bookworm
  arm64). Not a CI runner. A second Pi may come later.
- Target OS: Raspberry Pi OS Bookworm 64-bit. DietPi has no `/etc/rpi-issue`,
  so `install.sh` treats it as a non-Pi: no `-DRPI_BUILD`, no pulseaudio or
  bluez rebuilds. Dash has zero `#if RPI` code; openauto's RPI path only adds
  OMX and legacy btservice code. The container builds the same generic path,
  so this is a fidelity match with the fire-test Pi, not a gap. Record it.
- QEMU full-system `raspi4b` has no PCIe: no USB, no Ethernet, no GPU. Out.
- Chosen environment: arm64 Debian bookworm userspace container with the
  Raspberry Pi apt archive pinned on top. Package-for-package what Pi OS and
  DietPi install, from official sources. No Pi kernel, no V3D, no USB, no
  Bluetooth hardware, no V4L2, no ALSA.
- The fork `ivolkg/dash` is public, so GitHub's free `ubuntu-24.04-arm`
  hosted runners apply: native arm64 in CI, no QEMU. Locally the user installs
  `podman qemu-user-static qemu-user-static-binfmt` on Arch and runs the same
  images under qemu-user.
- Existing `.github/workflows/build.yml` runs all of `install.sh` on x86 on
  every push: unpinned clones, `apt-get upgrade`, and a smoke check for a log
  line that no longer exists. Restrict it to manual dispatch in this step.
- Observability hooks in the current app: startup logs `loaded config:`
  (`src/dash.cpp:21`) and `[Bluetooth] Init complete!`
  (`src/app/services/bluetooth.cpp:42`, fires even without an adapter as long
  as a system D-Bus exists). Websocket server on 54545
  (`src/app/services/server.cpp`) only when `System/Server/enabled=true` in
  `~/.config/openDsh/dash.conf`; `/state` takes a JSON array of keys
  (`mode`, `color`, `page`, `brightness`, `volume`, `pages`) or an object to
  set; `/action` takes `cycle_page` and friends. CAN: `Gauge`
  (`src/app/pages/vehicle.cpp`) registers a handler on `0x7E8` and a timer
  sends mode-01 requests to `0x7DF`. ELM327 USB path opens the interface as
  a tty and sends `ATD ATZ ATE0 ATL0 ATS0 AT H0 ATSP0 0100`. Camera network
  path builds `rtspsrc location=<url> ! queue ! decodebin`. A `mocked`
  brightness plugin already exists.

## Design

### Container images (`ci/`)

`ci/Containerfile.deps`, one stage per dependency so a change rebuilds only
downstream:

1. `debian:bookworm` arm64, add Raspberry Pi archive key and
   `/etc/apt/preferences.d/raspi.pref` (priority 1001), install the package
   array from `install.sh` copied verbatim into `ci/apt-deps.txt`, plus
   container tooling: `dbus iproute2 can-utils socat procps python3-websockets
   python3-can python3-gi gir1.2-gst-rtsp-server-1.0 gstreamer1.0-tools ccache
   ninja-build git`.
2. aasdk at pinned SHA with `patches/aasdk_openssl-fips-fix.patch`.
3. h264bitstream at pinned SHA.
4. qt-gstreamer at pinned SHA with the three patches, bookworm cmake flags
   from `install.sh`.
5. openauto at pinned SHA, `-DGST_BUILD=true`.
6. `ldconfig`; `ENV QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software`;
   entrypoint starts `dbus-daemon --system --fork`.

Pins live in `ci/deps.lock` (aasdk `1bc0fe6`, openauto `e7caeb4`,
h264bitstream and qt-gstreamer recorded at first build). Image tag is a hash
of `Containerfile.deps + apt-deps.txt + deps.lock + patches/*`, pushed to
`ghcr.io/ivolkg/dash-deps:<tag>` and only rebuilt when the hash changes.

`ci/Containerfile.dash`: `FROM $DEPS_IMAGE`, copy repo, cmake Release with
`-DGST_BUILD=TRUE -G Ninja -DCMAKE_CXX_COMPILER_LAUNCHER=ccache`, build.
`install.sh` is not called; its package list and flags are duplicated into
`ci/` deliberately for this step.

### Baseline tests (`ci/tests/`)

Shared `lib.sh`: render `dash.conf.tpl` into `~/.config/openDsh/dash.conf`,
start `bin/dash` in background, wait up to 30 s for both readiness log lines,
on exit SIGTERM and assert the process ends (a hang is a failure). Each test
is one script with a 120 s timeout, logs to `/out/`.

| # | Test | Assertion |
|---|---|---|
| 01 build | `bin/dash`, `lib/plugins/brightness/libmocked.so`, `lib/plugins/vehicle/libtest.so` exist; `ldd bin/dash` has no "not found" |
| 02 smoke | readiness within 30 s with `test` vehicle plugin and `mocked` brightness; alive at 15 s; no `Segmentation`/`abort`/plugin-load errors in log; exits on SIGTERM |
| 03 websocket | `/state` array returns all six keys and `pages` has 6 entries; setting `mode=dark` echoes it; setting `volume=30` persists on re-query; `/action cycle_page` changes `page` |
| 04 vcan-obd | `can_bus=0 interface=vcan0`; log shows socketcan available and handler for id 2024; after switching to the Vehicle page via `/state`, `candump vcan0` sees a `7DF` request within 5 s; python-can responder answers on `7E8`; app alive 10 s later |
| 05 elm327-pty | `socat` pty pair, python fake ELM on one end; `can_bus=1 interface=/tmp/elm_app`; fake receives the init sequence in order, then `010C`-style queries after page switch; no `failed write`/`failed read` in log |
| 06 camera-rtsp | python GstRtspServer serving `videotestsrc` H.264 (fallback MJPEG) on `rtsp://127.0.0.1:8554/test`; `is_network=true`; after switching to Camera page, log shows the created pipeline string; that string plus `! fakesink` reaches PLAYING under `gst-launch-1.0` for 3 s. Frames are not asserted |

`run-all.sh` runs all, continues on failure, prints a summary, exits non-zero
if any failed. Tests 04 needs host `vcan` module and `--network host
--cap-add NET_ADMIN`; the others need nothing privileged.

Not automatable here, deferred to the fire test: gauge values, Bluetooth
media, Android Auto, local V4L2 camera, GPIO, backlight, any pixel output.

### CI (`.github/workflows/pi-sim.yml`)

Trigger: push and PR to `dev-rs`, weekly schedule, manual dispatch. Jobs on
`ubuntu-24.04-arm`:

- `deps-image`: compute tag, check `ghcr.io` manifest, build and push only if
  missing. About 2 min when cached, 35 to 50 min on a rebuild.
- `build-test`: needs `deps-image`; restore ccache via `actions/cache`;
  `modprobe vcan` and create `vcan0`; build `Containerfile.dash`; run
  `run-all.sh` with host network and NET_ADMIN; upload `/out` as artifact.
  About 10 to 15 min cold, 5 min warm.

`rust.yml` stays separate. `build.yml` changes to `workflow_dispatch` only.

### Local loop (`ci/dev.sh`)

Subcommands wrapping podman: `pull-deps` (from ghcr, saves an hour),
`build-deps`, `build`, `test [name]`, `shell`. All with `--arch arm64`,
`--network host --cap-add NET_ADMIN`, a named ccache volume, `-v $PWD/out:/out:Z`.
Prereq the user installs: `podman qemu-user-static qemu-user-static-binfmt`;
host `vcan0` once per boot. Expect roughly 60 to 90 min for a local deps
build under qemu-user on this 20-core machine, 5 to 8 min for a warm dash
rebuild, 5 min for the suite.

### Fire-test checklist (`docs/FIRE_TEST.md`)

Manual, on the DietPi Pi, one line each with expected result: boot to UI
within 20 s; websocket mode flip from a laptop changes theme live; AA over
USB connects, touch works, unplug returns to connect page within 10 s, replug
reconnects without restart; wireless AA pairs, connects, and survives a phone
Wi-Fi toggle (record retries, this is the known-weak path); camera shows
frames and recovers after stream loss; Bluetooth media shows track and
play/pause works; brightness slider moves the backlight; real CAN or ELM327
shows live RPM and speed; volume audible and a bound action fires; clean
shutdown from Settings.

### Docs

Append a "Step 1 environment" section to `docs/OXIDIZING.md` with the
fidelity limits stated verbatim: userspace only, same packages and ABI as Pi
OS and DietPi; no Pi kernel, V3D, backlight, GPIO, USB, Bluetooth, V4L2 or
ALSA; offscreen Qt exercises state, signals, plugins, websocket, CAN, ELM327
and pipeline construction but not pixels; timings prove nothing about Pi
performance.

## Files

| Path | Purpose |
|---|---|
| `ci/apt-deps.txt` | package list from `install.sh`, plus container-only section |
| `ci/deps.lock` | upstream repo URLs and pinned SHAs |
| `ci/Containerfile.deps` | base + Pi archive + apt + four upstream builds |
| `ci/Containerfile.dash` | dash build on top of deps image |
| `ci/entrypoint.sh` | system D-Bus then exec |
| `ci/dev.sh` | podman wrapper for the local loop |
| `ci/tests/lib.sh`, `dash.conf.tpl`, `run-all.sh`, `01..06-*.sh` | baseline tests |
| `ci/tests/fakes/elm327_fake.py`, `obd_responder.py`, `rtsp_testsrc.py` | fakes |
| `.github/workflows/pi-sim.yml` | new CI |
| `.github/workflows/build.yml` | `on: workflow_dispatch` only |
| `docs/FIRE_TEST.md` | manual checklist with results column |
| `docs/OXIDIZING.md` | step 1 section |
| `CLAUDE.md` | add the `ci/dev.sh` commands and the fire-test pointer |

No C++ changes in this step. Commits are one-liners, no trailers.

## Verification

1. `ci/dev.sh build-deps` or `pull-deps` succeeds locally once podman and
   binfmt are installed (user confirms `ls /proc/sys/fs/binfmt_misc | grep
   aarch64`).
2. `ci/dev.sh build` produces `bin/dash` for arm64 (`file bin/dash` says
   aarch64).
3. `ci/dev.sh test` runs all six tests green locally.
4. Push to `dev-rs`; `pi-sim.yml` runs green on the arm runner alongside
   `rust.yml`; the deps image appears in ghcr; the second push reuses it and
   `build-test` finishes in under 15 min.
5. `build.yml` no longer runs on push.
6. The user runs `docs/FIRE_TEST.md` on the DietPi Pi once to record the
   baseline results, including how wireless AA reconnect behaves today.

## Order of work

1. `ci/apt-deps.txt`, `ci/deps.lock`, `Containerfile.deps`, `entrypoint.sh`,
   `dev.sh`; build the deps image locally or in CI first since it gates all
   else.
2. `Containerfile.dash`, test 01.
3. `lib.sh`, template, tests 02 and 03.
4. Fakes and tests 04, 05, 06.
5. `pi-sim.yml`, restrict `build.yml`, run in CI.
6. `FIRE_TEST.md`, docs, CLAUDE.md.

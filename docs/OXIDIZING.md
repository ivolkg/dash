# Oxidizing dash: how much can move to Rust, and is it worth it

Assessment date: 2026-10-13. Branch: `oxidizing`.

## Short answer

About 5 to 8 percent of the code can be ported to Rust in place with a real
benefit. The rest is Qt Widgets UI and OpenAuto glue, which cannot be ported
without rewriting the UI in a different toolkit.

## Where the code is

| Layer | Lines | Share | Qt coupling | Rust fit |
|---|---:|---:|---|---|
| Pages, widgets, quick views, graphics, icon engine | 6,515 | 62% | QWidget everywhere | None. Needs a UI rewrite. |
| Core (session, arbiter, window, action, config, main) | 2,125 | 20% | QObject signals, QSettings, QPluginLoader | None. This *is* the Qt app. |
| Plugins (brightness, launcher, radio, vehicle) | 809 | 8% | Qt plugin ABI, return QWidgets | Poor. Would need a new C ABI plugin interface. |
| CAN bus + OBD (`src/canbus`, `src/obd`) | 538 | 5% | QCanBus, QBluetoothSocket, raw termios/fd | Good. Clear boundary, pure logic inside. |
| Services (bluetooth, clock, websocket server) | 483 | 5% | BluezQt, QWebSocket, reads Arbiter state | Poor. Exists to feed the UI. |
| OpenAuto bridge (`AAHandler`, openauto page) | ~540 | 5% | openauto::App is a QWidget | None. aasdk/openauto have no Rust port. |

Total: 10,563 lines of C++. Shell scripts (`install.sh` and friends) are
another 1,000 lines and are a separate question.

## Why the UI cannot be ported

- Rust has no maintained Qt Widgets binding. The `qt_widgets` crate (ritual) is
  unmaintained; `cxx-qt` targets QML/QObject, not QWidget layouts.
- 82% of dash is QWidget subclasses with layouts, stylesheets (`.qss`), and
  signal/slot wiring to the Arbiter. Porting means rewriting in QML + cxx-qt,
  Slint, or iced. That is a new application, not a port.
- OpenAuto renders Android Auto into a QWidget owned by dash. Any UI rewrite
  must still host that C++ widget, so the Qt dependency never goes away.

## What is actually worth oxidizing

### 1. CAN bus and OBD layer (recommended)

Files: `src/canbus/elm327.cpp`, `src/canbus/socketcanbus.cpp`, `src/obd/*`,
`include/canbus/*`, `include/obd/*`.

Why this layer:

- It holds the only unsafe-by-nature code in the repo: raw `open`/`termios`
  serial setup, `::send` on a Qt socket's file descriptor, `usleep` in the
  init sequence, function-local static singletons constructed from `Config`.
- It has zero tests, and the ELM327 response parser and OBD decoders are pure
  functions that are trivial to unit test in Rust.
- The boundary is one interface: `ICANBus` (`writeFrame`,
  `registerFrameHandler`). Vehicle plugins only see that.

Shape of the work:

- New crate `dash-can` (`socketcan`, `serialport`, `bluer` for RFCOMM ELM327).
- Expose via `cxx` behind a thin C++ `ICANBus` adapter so plugins and
  `VehiclePage` do not change.
- Build integration via `corrosion` (needs CMake 3.15+; project currently
  declares 3.7, so bump it).
- Add `rustup`/`cargo` to the `install.sh` dependency list and the Pi
  cross-compile notes.

Estimate: 2 to 3 days for one person, including tests and CI. Payoff: tested
parser, no raw fd handling in C++, no singletons.

### 2. GPIO notifier (small, optional)

`GPIONotifier` in `src/app/action.cpp` watches `/sys/class/gpio` with
`QFileSystemWatcher`. Could use the `gpiod` crate. About 60 lines. Only worth
it if (1) is already done and the cxx bridge exists.

## What is not worth it

- **Websocket server** (227 lines). `tokio-tungstenite` + `serde` would be
  nicer, but every handler reads or writes Arbiter state, so it would be a
  Rust shell around C++ calls.
- **Bluetooth service**. BluezQt media player signals feed widgets directly.
- **Plugins**. The Qt plugin ABI (`Q_INTERFACES`, `QPluginLoader`) is the
  contract with third-party vehicle plugins. Changing it breaks users.
- **Install scripts**. Rewriting 700 lines of bash in Rust does not fix the
  real problem, which is unpinned upstream clones and no `set -e`.

## Does Rust fix the maintainability problems?

No. The issues found in the maintainability review are:

| Problem | Fixed by Rust? |
|---|---|
| Unpinned upstream repos in `install.sh` | No |
| CI builds the world from source | No (cargo adds another toolchain to install) |
| Dead OMX/GST branches in CMake | No |
| Duplicated include/link lists | No |
| No tests | Partly, for the CAN layer only |
| `Config` singleton marked "temporary" | No |
| One `QProcess` leak | Trivially fixed in C++ |

Qt parent ownership already covers the 531 `new` calls. Memory safety is not
the pain point in this codebase; build hygiene and tests are.

## Options

1. **Oxidize the CAN/OBD layer only** (recommended). 2 to 3 days. Proves the
   cxx + corrosion toolchain on the Pi, adds the first tests, removes the
   riskiest C++.
2. **Full rewrite** in Rust + Slint or QML/cxx-qt, keeping aasdk/openauto as
   C++ behind cxx. 3 to 6 months. Only sensible if the UI is being redone
   anyway.
3. **Do not oxidize.** Spend the same 2 to 3 days on the maintainability
   list instead: pin deps, fix CI, clean CMake, add `set -e`.

If the goal is "learn Rust on a real target" or "the CAN layer keeps biting",
pick 1. If the goal is "make dash easier to maintain", pick 3.

## UI toolkit options (annotated 2026-10-13)

Slint is the only Rust toolkit built for an embedded touch target. It beats
Qt Widgets on rendering (GPU scene graph), display stack (direct DRM/KMS, no
X11), footprint, built-in scaling and theming, compiler-enforced thread
safety, and testability. It loses on Android Auto video embedding (frames
must be pulled through gstreamer-rs into a texture and touch forwarded back
over cxx) and on the Qt plugin ecosystem. The fair comparison is Slint versus
Qt Quick, not versus Widgets. cxx-qt with QML is the lower-risk alternative
that keeps openauto video and plugins. iced, egui, GTK4-rs, Tauri and Dioxus
are poor fits for the Pi.

## Replacing core in Rust (annotated 2026-10-13)

Replace Arbiter, Session, Config and Action with one headless `dash-core`
crate:

| Today | Rust |
|---|---|
| Arbiter setters plus Qt signals | `enum Msg` in, `enum Event` out, one `update()` |
| Session, Config singleton, raw QSettings, duplicated string keys | One serde `Settings` struct, TOML, atomic writes |
| Action keybindings, GPIONotifier on sysfs | `HashMap<ActionId, Trigger>`, `gpiod` crate (sysfs GPIO is deprecated) |
| `set_volume` via leaked QProcess | `alsa` or `libpulse-binding` |
| Brightness via QPluginLoader | Built-in backends behind a trait |
| Websocket Server with hand-written handlers | `tokio-tungstenite`, serde on the same `Msg`/`Event` |
| Bluetooth via BluezQt | `bluer` or `zbus` to `org.bluez.MediaPlayer1` (verify media player coverage) |
| Clock | `tokio::time::interval` |
| DASH_LOG on Boost.Log | `tracing` |
| Forge, MainWindow, main | Not core; belongs to the UI toolkit. `clap` for args. |

```rust
pub enum Msg { SetMode(Mode), SetColor(Rgb), SetScale(f32), SetVolume(u8),
               SetBrightness(u8), SetPage(PageId), ToggleFullscreen, Trigger(ActionId) }
pub enum Event { ModeChanged(Mode), VolumeChanged(u8), PageChanged(PageId) /* ... */ }
pub struct Core { settings: Settings, state: State, tx: broadcast::Sender<Event> }
```

CAN callbacks, websocket and GPIO run on tokio and send `Msg`. The UI thread
owns `Core`, drains the channel, redraws on `Event`. Core is testable with no
display, has one mutation path, and the UI toolkit becomes swappable. About
1,500 lines, one to two weeks. Does not require the UI rewrite: expose `Core`
through cxx and shrink `Arbiter` to a shim. `Layout`'s page registries stay
in C++ because they hold QWidget pointers.

## Migration plan: module by module, test each step

Each step lands behind a cxx bridge with the Qt UI untouched, and each has a
test gate that runs on a PC before it is tried on the Pi.

| Step | Module | Test gate on PC |
|---|---|---|
| 1 | Toolchain: corrosion in CMake, `cargo` in `install.sh`, CMake min 3.15, CI job | `cargo test` and the CMake build both pass in the Docker image |
| 2 | OBD decoders and ELM327 parser (pure functions) | Unit tests with recorded ELM responses |
| 3 | SocketCAN and ELM327 transport behind `ICANBus` | `vcan0` plus a fake ELM327 on a `socat` pty |
| 4 | `Settings` struct | Round-trip tests, migration from the existing `QSettings` ini |
| 5 | `Core` with `Msg`/`Event` | Headless state tests; Arbiter becomes a shim |
| 6 | Websocket server, clock, GPIO | Websocket client tests; `gpio-sim` or a mocked trait |
| 7 | Brightness and volume backends | Mocked backend on PC; sysfs backlight only on the Pi |
| 8 | Bluetooth | `python-dbusmock` BlueZ template on PC; real phone on the Pi |
| 9 | UI pages, one at a time (Slint or QML) | Offscreen render tests; visual check on the Pi |

Android Auto has no PC test. aasdk needs a real phone over USB. Google's
Desktop Head Unit emulates the head unit side, not the phone, so it does
not help. AA stays a hardware-only check at every step.

## Testing on a PC

Three tiers, fastest first:

1. **Host `cargo test`.** Pure logic: decoders, parser, settings, core state
   machine. Seconds. Covers steps 2, 4, 5.
2. **Docker on x86.** Debian bookworm image with the apt list from
   `install.sh`, aasdk and openauto prebuilt into the image, `sudo` present
   because the script calls it. Run the full app headless with
   `QT_QPA_PLATFORM=offscreen` or under Xvfb. Fakes:
   - CAN: `modprobe vcan; ip link add vcan0 type vcan` on the host, run the
     container with `--network host` and `--cap-add NET_ADMIN`.
   - ELM327: `socat -d -d pty,raw,echo=0 pty,raw,echo=0` plus the
     `ELM327-emulator` Python package on one end.
   - Camera: GStreamer `videotestsrc` or a V4L2 loopback device.
   - GPIO: `gpio-sim` kernel module (Linux 5.17+) or the mocked trait.
   - Bluetooth: `python-dbusmock` with its BlueZ template on a session bus.
   - Brightness: mocked backend (one already exists in `plugins/brightness/mocked`).
   This is also the GitHub Actions job, replacing the current build-the-world
   workflow.
3. **aarch64 under QEMU user mode.** `cross test --target aarch64-unknown-linux-gnu`
   runs the Rust tests for the Pi architecture in minutes without booting a
   Pi. Catches endianness, alignment and `usize` mistakes.

Full-system Pi emulation (QEMU `raspi3b`, or the `virt` machine with a Pi OS
kernel) boots, but has no V3D GPU, no DRM/KMS acceleration, no USB passthrough
worth using. It is useful for packaging checks only, not for rendering,
Android Auto, Bluetooth or backlight. Those four stay on real hardware.

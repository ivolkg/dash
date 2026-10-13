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

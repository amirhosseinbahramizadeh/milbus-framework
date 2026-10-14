# milbus-analyzer

A Rust workspace for simulating, analyzing, and stress-testing military and
avionics data buses: **MIL-STD-1553B**, **ARINC 429**, and **Link 16**.
milbus-analyzer provides a virtual bus with no physical hardware, an interactive
TUI, headless bus-attack injection, and JSON frame capture/replay.

> **Disclaimer**
> This tool is provided for **educational use and authorized testing only**.
> Do not use it against any system you do not own or do not have explicit
> permission to test. The authors accept no liability for misuse.

## Installation

Requires a stable Rust toolchain (edition 2021).

```bash
git clone <https://github.com/amirhosseinbahramizadeh/milbus-framework>
cd milbus-analyzer
cargo build --release
```

The binary is produced at `target/release/milbus-analyzer`.

## Usage

The CLI exposes three subcommands: `simulate`, `inject`, and `replay`.

### `simulate` — interactive TUI on a virtual bus

Starts one bus controller and N remote terminals, then opens the Ratatui TUI.

```bash
# 3 RTs, 10 ms polling, capture all frames to a JSON file on exit
milbus-analyzer simulate --rts 3 --interval 10 --capture run.json
```

TUI keybindings:

| Key     | Action                                   |
|---------|------------------------------------------|
| `Tab`   | Switch protocol tab (MIL-1553B/429/16)   |
| `i`     | Open the hex injection box               |
| `Enter` | Send the injected frame                  |
| `s`     | Start / stop the simulator               |
| `q`     | Quit                                     |

### `inject` — headless bus attack

Runs a single attack and reports what the bus monitor observed.

```bash
milbus-analyzer inject --kind fake-command --target 5
milbus-analyzer inject --kind flood --target 1 --duration 1000
milbus-analyzer inject --kind corrupt-status --target 3
```

### `replay` — replay a captured log

Loads a JSON capture (written by `simulate --capture`) and re-injects its frames
through a fresh bus.

```bash
milbus-analyzer replay run.json
```

## Workspace layout

| Crate           | Purpose                                                   |
|-----------------|-----------------------------------------------------------|
| `protocol-core` | MIL-STD-1553B, ARINC 429, Link 16 encoding/decoding        |
| `simulator`     | Virtual bus, device roles, and bus-attack injection        |
| `tui`           | Ratatui-based terminal interface                           |
| `cli`           | Binary entry point (`milbus-analyzer`) and frame capture   |

## Running tests

```bash
cargo test --workspace
```

## License

MIT. See [LICENSE](LICENSE).

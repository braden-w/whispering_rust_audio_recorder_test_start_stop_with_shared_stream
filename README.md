# Rust Audio Recorder with Start/Stop Control

A test project demonstrating audio recording with start/stop functionality using shared streams in Rust. Built with CPAL (Cross-Platform Audio Library).

## Features

- List available audio input devices
- Record audio from the default input device to WAV files
- Support for different sample formats (16-bit integer and 32-bit float)

## Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- A working audio input device

## Building

```bash
cargo build --release
```

## Usage

### List Available Devices

```bash
cargo run -- list-devices
```

### Record Audio

Record 10 seconds of audio to output.wav:
```bash
cargo run -- record -d 10 -o output.wav
```

Options:
- `-d, --duration <SECONDS>`: Recording duration in seconds (default: 5)
- `-o, --output <FILE>`: Output WAV file path

## License

MIT 
# Rust Audio Recorder CLI

A command-line audio recording application built with Rust using CPAL (Cross-Platform Audio Library). This application allows you to control audio recording through simple interactive commands.

## Features

- Interactive command-line interface
- Initialize and manage audio input streams
- Record audio from the default input device to WAV files
- Clean stream management with start/stop functionality
- 32-bit float WAV recording format

## Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- A working audio input device

## Building

```bash
cargo build --release
```

## Usage

Run the application:
```bash
cargo run
```

### Available Commands

The application provides an interactive prompt with the following commands:

- `init` - Initialize the audio stream
- `start` - Start recording (saves to output.wav)
- `stop` - Stop the current recording
- `drop` - Drop the audio stream
- `exit` - Exit the program

### Example Session

```bash
$ cargo run
Audio Recorder CLI
Available commands:
  init    - Initialize the audio stream
  drop    - Drop the audio stream
  start   - Start recording (saves to output.wav)
  stop    - Stop recording
  exit    - Exit the program

> init
Stream initialized successfully
> start
Recording started
> stop
Recording stopped
> exit
Exiting...
```

## Output

Recordings are saved as `output.wav` in the current directory using 32-bit float format.

## License

MIT 
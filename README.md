# Rust Audio Recorder CLI

A command-line audio recording application built with Rust using CPAL (Cross-Platform Audio Library). This application allows you to control audio recording through simple interactive commands.

I created this project as an experimental prototype for implementing native audio recording in [Whispering](https://github.com/braden-w/whispering), an open-source transcription application. The goal was to explore using Rust's CPAL library to provide more reliable and efficient audio recording capabilities compared to web-based recording solutions.

This was my first time working with threads (ever!) and I learned a lot. I'm sure there are many improvements that could be made, but I'm happy with the results. Message passing is hard!

## Features

- List available recording devices
- Initialize recording sessions with configurable settings
- Start/stop/cancel recording operations
- WAV file output with configurable bit depth
- Command-line interface with interactive commands
- Thread-safe audio recording with proper resource management
- Error handling and logging

## Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))

## Building

```bash
cargo build --release
```

## Usage

Run the application using:

```bash
cargo run
```

### Available Commands

- `devices` - List all available recording devices
- `init [device_name] [bits_per_sample]` - Initialize recording session
  - `device_name` - Name of the recording device (default: "default")
  - `bits_per_sample` - Bit depth (16, 24, or 32)
- `destroy` - Close the current recording session
- `start [id]` - Start recording (optional ID for filename)
- `stop` - Stop recording and save the WAV file
- `cancel` - Cancel the current recording
- `exit` - Exit the application

### Example Usage

```bash
# List available devices
> devices

# Initialize recording with specific device and 32-bit depth
> init "My Audio Device" 32

# Start recording with custom ID
> start my_recording

# Stop recording
> stop

# Exit application
> exit
```

## Architecture

The application is structured into three main components:

1. `main.rs` - Command-line interface and application flow
2. `recorder.rs` - High-level recording operations and state management
3. `thread.rs` - Low-level audio thread handling and WAV file operations

### Key Components

- Uses `cpal` for audio device interaction
- `hound` for WAV file handling
- Thread-safe communication using channels
- Global state management with thread-safe mutexes
- Comprehensive error handling with custom error types
- Tracing-based logging system

## Error Handling

The application includes robust error handling for:
- Audio device initialization
- Recording session management
- File operations
- Thread communication
- Invalid user input

## Logging

The application uses the `tracing` crate for logging with configurable levels:
- Set `RUST_LOG=debug` for detailed debug output
- Default level is INFO
- Logs include timestamps and log levels

## License

[Add your license here]

## Contributing

[Add contribution guidelines here] 
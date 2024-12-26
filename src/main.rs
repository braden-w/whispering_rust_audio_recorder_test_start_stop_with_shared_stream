mod recorder;
mod thread;
use recorder::{
    cancel_recording, close_recording_session, close_thread, enumerate_recording_devices,
    init_recording_session, start_recording, stop_recording,
};
use thread::UserRecordingSessionConfig;
use tracing::{debug, error, info, warn, Level};

fn parse_command(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current_arg = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for c in input.chars() {
        match (c, in_quotes, escaped) {
            ('\\', _, false) => escaped = true,
            ('"', _, true) => {
                current_arg.push('"');
                escaped = false;
            }
            ('"', false, false) => in_quotes = true,
            ('"', true, false) => {
                if !current_arg.is_empty() {
                    args.push(current_arg.clone());
                    current_arg.clear();
                }
                in_quotes = false;
            }
            (' ', false, false) => {
                if !current_arg.is_empty() {
                    args.push(current_arg.clone());
                    current_arg.clear();
                }
            }
            (c, _, true) => {
                current_arg.push('\\');
                current_arg.push(c);
                escaped = false;
            }
            (c, _, false) => current_arg.push(c),
        }
    }

    if !current_arg.is_empty() {
        args.push(current_arg);
    }

    args
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging with environment variable control
    // Set RUST_LOG=debug for debug output, info by default
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(Level::INFO.into()),
        )
        .init();

    info!("Starting Audio Recorder CLI");
    debug!("Initializing command interface");

    println!("Audio Recorder CLI");
    println!("Available commands:");
    println!("  devices                              - List available recording devices");
    println!("  init [device_name] [bits_per_sample] - Initialize the audio stream");
    println!("  destroy                              - Destroy the audio stream");
    println!("  start [id]                           - Start recording. Optional id for filename [id].wav (default: output)");
    println!("  stop                                 - Stop recording and save the file");
    println!("  cancel                               - Cancel recording without saving");
    println!("  exit                                 - Exit the program");
    println!("\nNote: Use quotes for arguments containing spaces, e.g., init \"My Device\" 32");

    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let parts = parse_command(input.trim());
        debug!("Parsed command: {:?}", parts);

        match parts.get(0).map(|s| s.as_str()) {
            Some("devices") => match enumerate_recording_devices() {
                Ok(devices) => {
                    info!("Successfully enumerated {} devices", devices.len());
                    println!("\nAvailable recording devices:");
                    for device in devices {
                        println!("  - {} (ID: {})", device.label, device.device_id);
                    }
                }
                Err(e) => {
                    error!("Failed to enumerate devices: {}", e);
                    println!("Error: Failed to enumerate devices: {}", e);
                }
            },
            Some("init") => {
                let device_name = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "default".to_string());

                let bits_per_sample = parts
                    .get(2)
                    .and_then(|s| s.parse::<u16>().ok())
                    .unwrap_or(32);

                if bits_per_sample != 16 && bits_per_sample != 24 && bits_per_sample != 32 {
                    error!("Invalid bits_per_sample value: {}", bits_per_sample);
                    println!("Error: bits_per_sample must be 16, 24, or 32");
                    continue;
                }

                debug!(
                    "Initializing recording session with device: {}, bits: {}",
                    device_name, bits_per_sample
                );
                let config = UserRecordingSessionConfig {
                    device_name,
                    bits_per_sample,
                };

                match init_recording_session(config) {
                    Ok(_) => {
                        info!("Recording session initialized successfully");
                        println!("Recording session initialized");
                    }
                    Err(e) => {
                        error!("Failed to initialize recording session: {}", e);
                        println!("Error initializing recording session: {}", e);
                    }
                }
            }
            Some("destroy") => {
                debug!("Attempting to destroy recording session");
                match close_recording_session() {
                    Ok(_) => {
                        info!("Recording session destroyed successfully");
                        println!("Recording session destroyed");
                    }
                    Err(e) => {
                        error!("Failed to destroy recording session: {}", e);
                        println!("Error destroying recording session: {}", e);
                    }
                }
            }
            Some("start") => {
                let id = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "output".to_string());

                debug!("Starting recording with id: {}", id);
                match start_recording(id) {
                    Ok(_) => {
                        info!("Recording started successfully");
                        println!("Recording started");
                    }
                    Err(e) => {
                        error!("Failed to start recording: {}", e);
                        println!("Error starting recording: {}", e);
                    }
                }
            }
            Some("stop") => {
                debug!("Attempting to stop recording");
                match stop_recording() {
                    Ok(wav_data) => {
                        info!("Recording stopped successfully ({} bytes)", wav_data.len());
                        println!("Recording stopped and saved ({} bytes)", wav_data.len());
                    }
                    Err(e) => {
                        error!("Failed to stop recording: {}", e);
                        println!("Error stopping recording: {}", e);
                    }
                }
            }
            Some("cancel") => {
                debug!("Attempting to cancel recording");
                match cancel_recording() {
                    Ok(_) => {
                        info!("Recording cancelled successfully");
                        println!("Recording cancelled");
                    }
                    Err(e) => {
                        error!("Failed to cancel recording: {}", e);
                        println!("Error cancelling recording: {}", e);
                    }
                }
            }
            Some("exit") => {
                info!("Received exit command");
                // Try to clean up any active recording session before exiting
                if let Err(e) = close_recording_session() {
                    warn!("Failed to clean up recording session: {}", e);
                    println!("Warning: Failed to clean up recording session: {}", e);
                }

                // Close the audio thread
                if let Err(e) = close_thread() {
                    warn!("Failed to close audio thread: {}", e);
                    println!("Warning: Failed to close audio thread: {}", e);
                }

                info!("Exiting application");
                println!("Exiting...");
                break;
            }
            _ => {
                error!("Unknown command received: {:?}", parts);
                println!("Unknown command. Available commands: devices, init [device_name] [bits_per_sample], destroy, start [id], stop, cancel, exit");
            }
        }
    }

    Ok(())
}

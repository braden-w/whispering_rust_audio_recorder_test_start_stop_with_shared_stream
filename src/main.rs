mod recorder;
mod thread;
use recorder::{
    cancel_recording, close_recording_session, enumerate_recording_devices, init_recording_session,
    start_recording, stop_recording,
};
use thread::UserRecordingSessionConfig;

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

fn main() -> std::result::Result<(), String> {
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
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?;
        let parts = parse_command(input.trim());
        println!("parts: {:?}", parts);

        match parts.get(0).map(|s| s.as_str()) {
            Some("devices") => {
                let devices = enumerate_recording_devices()
                    .map_err(|e| format!("Failed to enumerate devices: {}", e))?;
                println!("\nAvailable recording devices:");
                for device in devices {
                    println!("  - {} (ID: {})", device.label, device.device_id);
                }
            }
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
                    println!("Error: bits_per_sample must be 16, 24, or 32");
                    continue;
                }

                let config = UserRecordingSessionConfig {
                    device_name,
                    bits_per_sample,
                };

                match init_recording_session(config) {
                    Ok(_) => println!("Recording session initialized"),
                    Err(e) => println!("Error initializing recording session: {}", e),
                }
            }
            Some("destroy") => match close_recording_session() {
                Ok(_) => println!("Recording session destroyed"),
                Err(e) => println!("Error destroying recording session: {}", e),
            },
            Some("start") => {
                let id = parts
                    .get(1)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "output".to_string());

                match start_recording(id) {
                    Ok(_) => println!("Recording started"),
                    Err(e) => println!("Error starting recording: {}", e),
                }
            }
            Some("stop") => match stop_recording() {
                Ok(wav_data) => {
                    println!("Recording stopped and saved ({} bytes)", wav_data.len());
                }
                Err(e) => println!("Error stopping recording: {}", e),
            },
            Some("cancel") => match cancel_recording() {
                Ok(_) => println!("Recording cancelled"),
                Err(e) => println!("Error cancelling recording: {}", e),
            },
            Some("exit") => {
                // Try to clean up any active recording session before exiting
                if let Err(e) = close_recording_session() {
                    println!("Warning: Failed to clean up recording session: {}", e);
                }
                println!("Exiting...");
                break;
            }
            _ => {
                println!("Unknown command. Available commands: devices, init [device_name] [bits_per_sample], destroy, start [id], stop, cancel, exit");
            }
        }
    }

    Ok(())
}

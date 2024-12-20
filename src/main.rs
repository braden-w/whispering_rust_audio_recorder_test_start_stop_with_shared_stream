use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    fs::File,
    io::BufWriter,
    sync::{mpsc, Arc, Mutex},
};

const CHANNEL_BUFFER_SIZE: usize = 1;
const DEFAULT_BITS_PER_SAMPLE: u16 = 32;

// Commands that can be sent to the audio thread
#[derive(Debug)]
enum AudioCommand {
    InitStream,
    DropStream,
    StartRecording(String, u16),
    StopRecording,
    CancelRecording(String),
    Exit,
}

fn spawn_audio_thread() -> std::result::Result<mpsc::Sender<AudioCommand>, String> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || -> std::result::Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No input device available".to_string())?;

        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let stream_config = config.clone().into();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut stream_option: Option<cpal::Stream> = None;

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::InitStream => {
                    if stream_option.is_none() {
                        let writer_for_closure = Arc::clone(&writer_clone);
                        match device.build_input_stream(
                            &stream_config,
                            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                                if let Some(writer) = &mut *writer_for_closure.lock().unwrap() {
                                    for &sample in data {
                                        let _ = writer.write_sample(sample);
                                    }
                                }
                            },
                            |err| eprintln!("Stream error: {}", err),
                            None,
                        ) {
                            Ok(stream) => {
                                let _ = stream.play();
                                stream_option = Some(stream);
                                println!("Stream initialized successfully");
                            }
                            Err(e) => eprintln!("Failed to build stream: {}", e),
                        }
                    } else {
                        println!("Stream is already initialized");
                    }
                }
                AudioCommand::DropStream => {
                    if let Some(stream) = stream_option.take() {
                        drop(stream);
                        println!("Stream dropped successfully");
                    } else {
                        println!("No active stream to drop");
                    }
                }
                AudioCommand::StartRecording(filename, bits_per_sample) => {
                    if let Some(_) = &stream_option {
                        let spec = {
                            let config: &cpal::SupportedStreamConfig = &config;
                            hound::WavSpec {
                                channels: config.channels(),
                                sample_rate: config.sample_rate().0,
                                bits_per_sample,
                                sample_format: match bits_per_sample {
                                    16 => hound::SampleFormat::Int,
                                    24 => hound::SampleFormat::Int,
                                    32 => hound::SampleFormat::Float,
                                    _ => {
                                        eprintln!(
                                            "Unsupported bits per sample: {}",
                                            bits_per_sample
                                        );
                                        continue;
                                    }
                                },
                            }
                        };
                        match hound::WavWriter::create(&filename, spec) {
                            Ok(new_writer) => {
                                *writer.lock().unwrap() = Some(new_writer);
                                println!(
                                    "Recording started with {} bits per sample",
                                    bits_per_sample
                                );
                            }
                            Err(e) => eprintln!("Failed to create WAV writer: {}", e),
                        }
                    } else {
                        println!("Stream not initialized. Please initialize stream first.");
                    }
                }
                AudioCommand::StopRecording => {
                    if let Some(writer) = writer.lock().unwrap().take() {
                        let _ = writer.finalize();
                        println!("Recording stopped");
                    } else {
                        println!("No active recording to stop");
                    }
                }
                AudioCommand::CancelRecording(filename) => {
                    if let Some(writer) = writer.lock().unwrap().take() {
                        drop(writer);
                        if let Err(e) = std::fs::remove_file(filename) {
                            eprintln!("Failed to delete partial recording: {}", e);
                        }
                        println!("Recording cancelled and file deleted");
                    } else {
                        println!("No active recording to cancel");
                    }
                }
                AudioCommand::Exit => {
                    println!("Audio thread exiting...");
                    break;
                }
            }
        }

        Ok(())
    });

    Ok(tx)
}

fn main() -> std::result::Result<(), String> {
    let audio_tx: Arc<Mutex<Option<mpsc::Sender<AudioCommand>>>> = Arc::new(Mutex::new(None));
    let current_recording_filename = Arc::new(Mutex::new(None::<String>));

    println!("Audio Recorder CLI");
    println!("Available commands:");
    println!("  init                                - Initialize the audio stream");
    println!("  drop                                - Drop the audio stream");
    println!("  start [bits_per_sample] [id]        - Start recording. Optional bits_per_sample: 16, 24, or 32 (default: 32).");
    println!("                                        Optional id for filename [id].wav (default: output)");
    println!("  stop                                - Stop recording and save the file");
    println!("  cancel                              - Cancel recording without saving");
    println!("  exit                                - Exit the program");

    loop {
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| e.to_string())?;
        let command = input.trim();

        // Split command into parts for handling arguments
        let parts: Vec<&str> = command.split_whitespace().collect();
        match parts.get(0).map(|s| *s) {
            Some("init") => {
                if audio_tx.lock().unwrap().is_some() {
                    println!("Stream already initialized");
                    continue;
                }

                match spawn_audio_thread() {
                    Ok(tx) => {
                        *audio_tx.lock().unwrap() = Some(tx);
                        if let Some(tx) = &*audio_tx.lock().unwrap() {
                            tx.send(AudioCommand::InitStream)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    Err(e) => println!("Failed to initialize stream: {}", e),
                }
            }
            Some("drop") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::DropStream)
                        .map_err(|e| e.to_string())?;
                    *audio_tx.lock().unwrap() = None;
                } else {
                    println!("No active stream to drop");
                }
            }
            Some("start") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    let bits_per_sample = if let Some(bits_str) = parts.get(1) {
                        match bits_str.parse::<u16>() {
                            Ok(bits) if [16, 24, 32].contains(&bits) => bits,
                            _ => {
                                println!("Invalid bits per sample. Using default (32). Valid values are: 16, 24, 32");
                                DEFAULT_BITS_PER_SAMPLE
                            }
                        }
                    } else {
                        DEFAULT_BITS_PER_SAMPLE
                    };

                    let filename = if let Some(id) = parts.get(2) {
                        format!("{}.wav", id)
                    } else {
                        "output.wav".to_string()
                    };
                    *current_recording_filename.lock().unwrap() = Some(filename);

                    tx.send(AudioCommand::StartRecording(
                        if let Some(id) = parts.get(2) {
                            format!("{}.wav", id)
                        } else {
                            "output.wav".to_string()
                        },
                        bits_per_sample,
                    ))
                    .map_err(|e| e.to_string())?;
                } else {
                    println!("Stream not initialized");
                }
            }
            Some("stop") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    // First stop the recording
                    tx.send(AudioCommand::StopRecording)
                        .map_err(|e| e.to_string())?;
                    // Then drop the stream
                    tx.send(AudioCommand::DropStream)
                        .map_err(|e| e.to_string())?;
                    // Finally exit the thread
                    tx.send(AudioCommand::Exit).map_err(|e| e.to_string())?;
                    // Clear the sender from the mutex
                    *audio_tx.lock().unwrap() = None;
                } else {
                    println!("Stream not initialized");
                }
            }
            Some("cancel") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::CancelRecording(
                        current_recording_filename.lock().unwrap().take().unwrap(),
                    ))
                    .map_err(|e| e.to_string())?;
                } else {
                    println!("Stream not initialized");
                }
            }
            Some("exit") => {
                if let Some(tx) = audio_tx.lock().unwrap().take() {
                    // First stop any ongoing recording
                    tx.send(AudioCommand::StopRecording)
                        .map_err(|e| e.to_string())?;
                    // Then drop the stream
                    tx.send(AudioCommand::DropStream)
                        .map_err(|e| e.to_string())?;
                    // Finally exit the thread
                    tx.send(AudioCommand::Exit).map_err(|e| e.to_string())?;
                }
                println!("Exiting...");
                break;
            }
            _ => {
                println!("Unknown command. Available commands: init, drop, start [bits_per_sample], stop, cancel, exit");
            }
        }
    }

    Ok(())
}

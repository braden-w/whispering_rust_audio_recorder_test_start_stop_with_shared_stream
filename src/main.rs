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
            }
        }

        Ok(())
    });

    Ok(tx)
}

fn main() -> std::result::Result<(), String> {
    let audio_tx: Arc<Mutex<Option<mpsc::Sender<AudioCommand>>>> = Arc::new(Mutex::new(None));
    let mut current_bits_per_sample = DEFAULT_BITS_PER_SAMPLE;

    println!("Audio Recorder CLI");
    println!("Available commands:");
    println!("  init                    - Initialize the audio stream");
    println!("  drop                    - Drop the audio stream");
    println!("  start                   - Start recording (saves to output.wav)");
    println!("  stop                    - Stop recording");
    println!("  bits [16|24|32]         - Set bits per sample (default: 32)");
    println!("  exit                    - Exit the program");

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
            Some("bits") => {
                if let Some(bits_str) = parts.get(1) {
                    match bits_str.parse::<u16>() {
                        Ok(bits) => {
                            if [16, 24, 32].contains(&bits) {
                                current_bits_per_sample = bits;
                                println!("Bits per sample set to {}", bits);
                            } else {
                                println!("Invalid bits per sample. Valid values are: 16, 24, 32");
                            }
                        }
                        Err(_) => println!("Invalid number format. Please use: bits [16|24|32]"),
                    }
                } else {
                    println!("Current bits per sample: {}", current_bits_per_sample);
                }
            }
            Some("start") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::StartRecording(
                        "output.wav".to_string(),
                        current_bits_per_sample,
                    ))
                    .map_err(|e| e.to_string())?;
                } else {
                    println!("Stream not initialized");
                }
            }
            Some("stop") => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::StopRecording)
                        .map_err(|e| e.to_string())?;
                } else {
                    println!("Stream not initialized");
                }
            }
            Some("exit") => {
                if let Some(tx) = audio_tx.lock().unwrap().take() {
                    tx.send(AudioCommand::DropStream)
                        .map_err(|e| e.to_string())?;
                }
                println!("Exiting...");
                break;
            }
            _ => {
                println!("Unknown command. Available commands: init, drop, start, stop, bits [16|24|32], exit");
            }
        }
    }

    Ok(())
}

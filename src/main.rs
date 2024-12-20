use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    fmt,
    fs::File,
    io::BufWriter,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

const CHANNEL_BUFFER_SIZE: usize = 1;
const WAV_BITS_PER_SAMPLE: u16 = 32;

// Custom error type for audio operations
#[derive(Debug)]
enum AudioError {
    DeviceError(String),
    StreamError(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceError(msg) => write!(f, "Device error: {}", msg),
            AudioError::StreamError(msg) => write!(f, "Stream error: {}", msg),
        }
    }
}

// Commands that can be sent to the audio thread
#[derive(Debug)]
enum AudioCommand {
    InitStream,
    DropStream,
    StartRecording(String),
    StopRecording,
}

fn spawn_audio_thread() -> Result<mpsc::Sender<AudioCommand>, AudioError> {
    let (tx, mut rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    std::thread::spawn(move || {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| AudioError::DeviceError("No input device available".to_string()))?;

        let config = device
            .default_input_config()
            .map_err(|e| AudioError::DeviceError(e.to_string()))?;
        let stream_config = config.clone().into();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut stream_option: Option<cpal::Stream> = None;

        while let Some(cmd) = rx.blocking_recv() {
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
                AudioCommand::StartRecording(filename) => {
                    if let Some(_) = &stream_option {
                        let spec = {
                            let config: &cpal::SupportedStreamConfig = &config;
                            hound::WavSpec {
                                channels: config.channels(),
                                sample_rate: config.sample_rate().0,
                                bits_per_sample: WAV_BITS_PER_SAMPLE,
                                sample_format: hound::SampleFormat::Float,
                            }
                        };
                        match hound::WavWriter::create(&filename, spec) {
                            Ok(new_writer) => {
                                *writer.lock().unwrap() = Some(new_writer);
                                println!("Recording started");
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

        Ok::<(), AudioError>(())
    });

    Ok(tx)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let audio_tx: Arc<Mutex<Option<mpsc::Sender<AudioCommand>>>> = Arc::new(Mutex::new(None));

    println!("Audio Recorder CLI");
    println!("Available commands:");
    println!("  init    - Initialize the audio stream");
    println!("  drop    - Drop the audio stream");
    println!("  start   - Start recording (saves to output.wav)");
    println!("  stop    - Stop recording");
    println!("  exit    - Exit the program");

    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let command = input.trim();

        match command {
            "init" => {
                if audio_tx.lock().unwrap().is_some() {
                    println!("Stream already initialized");
                    continue;
                }

                match spawn_audio_thread() {
                    Ok(tx) => {
                        *audio_tx.lock().unwrap() = Some(tx);
                        if let Some(tx) = &*audio_tx.lock().unwrap() {
                            tx.send(AudioCommand::InitStream).await?;
                        }
                    }
                    Err(e) => println!("Failed to initialize stream: {}", e),
                }
            }
            "drop" => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::DropStream).await?;
                    *audio_tx.lock().unwrap() = None;
                } else {
                    println!("No active stream to drop");
                }
            }
            "start" => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::StartRecording("output.wav".to_string()))
                        .await?;
                } else {
                    println!("Stream not initialized");
                }
            }
            "stop" => {
                if let Some(tx) = &*audio_tx.lock().unwrap() {
                    tx.send(AudioCommand::StopRecording).await?;
                } else {
                    println!("Stream not initialized");
                }
            }
            "exit" => {
                if let Some(tx) = audio_tx.lock().unwrap().take() {
                    tx.send(AudioCommand::DropStream).await?;
                }
                println!("Exiting...");
                break;
            }
            _ => {
                println!("Unknown command. Available commands: init, drop, start, stop, exit");
            }
        }
    }

    Ok(())
}

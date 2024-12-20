use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Stream, SupportedStreamConfig,
};
use std::sync::mpsc::{self, Sender};
use std::{
    fs::File,
    io::BufWriter,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub enum AudioCommand {
    InitStream,
    DestroyStream,
    CloseThread,

    StartRecording(String, u16),
    StopRecording,
    CancelRecording(String),
}

pub struct AudioThread {
    tx: Option<mpsc::Sender<AudioCommand>>,
}

impl AudioThread {
    pub fn new() -> Self {
        Self { tx: None }
    }

    fn open_thread(&mut self, device_name: String) -> Result<(), String> {
        if self.tx.is_some() {
            return Ok(());
        }
        self.tx = Some(spawn_audio_thread(device_name)?);
        Ok(())
    }

    fn close_thread(&mut self) -> Result<(), String> {
        if let Some(tx) = self.tx.take() {
            tx.send(AudioCommand::CloseThread)
                .map_err(|e| e.to_string())?;
        }
        self.tx = None;
        Ok(())
    }

    pub fn send_command(&self, command: AudioCommand) -> Result<(), String> {
        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| "Thread not running".to_string())?;
        tx.send(command).map_err(|e| e.to_string())
    }

    pub fn is_running(&self) -> bool {
        self.tx.is_some()
    }
}

fn spawn_audio_thread(device_name: String) -> Result<mpsc::Sender<AudioCommand>, String> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || -> Result<(), String> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .map_err(|e| e.to_string())?
            .find(|d| matches!(d.name(), Ok(name) if name == device_name))
            .ok_or_else(|| "Device not found".to_string())?;

        let config = device.default_input_config().map_err(|e| e.to_string())?;
        let stream_config = config.clone().into();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut maybe_stream: Option<Stream> = None;

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::InitStream => {
                    if maybe_stream.is_none() {
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
                                maybe_stream = Some(stream);
                                println!("Stream initialized successfully");
                            }
                            Err(e) => eprintln!("Failed to build stream: {}", e),
                        }
                    } else {
                        println!("Stream is already initialized");
                    }
                }
                AudioCommand::DestroyStream => {
                    if let Some(stream) = maybe_stream.take() {
                        drop(stream);
                        println!("Stream destroyed successfully");
                    } else {
                        println!("No active stream to destroy");
                    }
                }
                AudioCommand::CloseThread => {
                    println!("Audio thread exiting...");
                    break;
                }
                AudioCommand::StartRecording(filename, bits_per_sample) => {
                    if let Some(_) = &maybe_stream {
                        let spec = {
                            let config: &SupportedStreamConfig = &config;
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
            }
        }

        Ok(())
    });

    Ok(tx)
}

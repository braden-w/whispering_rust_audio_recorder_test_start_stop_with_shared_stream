use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Stream, StreamConfig, SupportedStreamConfig,
};
use std::sync::mpsc::{self};
use std::{
    fs::File,
    io::BufWriter,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub struct UserRecordingSessionConfig {
    device_name: String,
    bits_per_sample: u16,
}

#[derive(Debug)]
pub enum AudioCommand {
    CloseThread,

    InitRecordingSession(UserRecordingSessionConfig),
    CloseRecordingSession,

    StartRecording(String),
    StopRecording,
    CancelRecording(String),
}

pub struct AudioThreadManager {
    tx: Option<mpsc::Sender<AudioCommand>>,
}

impl AudioThreadManager {
    pub fn new() -> Self {
        Self { tx: None }
    }

    pub fn open(&mut self) -> Result<(), String> {
        if self.tx.is_some() {
            return Ok(());
        }
        self.tx = Some(spawn_audio_thread()?);
        Ok(())
    }

    pub fn close(&mut self) -> Result<(), String> {
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

fn spawn_audio_thread() -> Result<mpsc::Sender<AudioCommand>, String> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || -> Result<(), String> {
        let host = cpal::default_host();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut maybe_stream: Option<Stream> = None;
        let mut current_recording_session_wav_writer_config: Option<hound::WavSpec> = None;

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::InitRecordingSession(recording_session_config) => {
                    if maybe_stream.is_some() {
                        println!("Stream is already initialized");
                    } else {
                        let device = host
                            .input_devices()
                            .map_err(|e| e.to_string())?
                            .find(|d| matches!(d.name(), Ok(name) if name == recording_session_config.device_name))
                            .ok_or_else(|| "Device not found".to_string())?;
                        let default_device_config =
                            device.default_input_config().map_err(|e| e.to_string())?;
                        current_recording_session_wav_writer_config = Some(hound::WavSpec {
                            channels: default_device_config.channels(),
                            sample_rate: default_device_config.sample_rate().0,
                            bits_per_sample: recording_session_config.bits_per_sample,
                            sample_format: match recording_session_config.bits_per_sample {
                                16 => hound::SampleFormat::Int,
                                24 => hound::SampleFormat::Int,
                                32 => hound::SampleFormat::Float,
                                _ => {
                                    eprintln!(
                                        "Unsupported bits per sample: {}",
                                        recording_session_config.bits_per_sample
                                    );
                                    continue;
                                }
                            },
                        });

                        let stream_config = default_device_config.into();
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
                    }
                }
                AudioCommand::CloseRecordingSession => {
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
                AudioCommand::StartRecording(filename) => {
                    match hound::WavWriter::create(
                        &filename,
                        current_recording_session_wav_writer_config.unwrap(),
                    ) {
                        Ok(new_writer) => {
                            *writer.lock().unwrap() = Some(new_writer);
                            println!(
                                "Recording started with {} bits per sample",
                                current_recording_session_wav_writer_config
                                    .unwrap()
                                    .bits_per_sample
                            );
                        }
                        Err(e) => eprintln!("Failed to create WAV writer: {}", e),
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

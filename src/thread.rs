use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Stream,
};
use std::sync::mpsc::{self, SendError};
use std::{
    fs::File,
    io::BufWriter,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
pub struct UserRecordingSessionConfig {
    pub device_name: String,
    pub bits_per_sample: u16,
}

#[derive(Debug, Clone)]
pub enum RecordingState {
    Idle,
    Initialized,
    Recording,
    Paused,
    Error(String),
}

#[derive(Debug)]
pub enum AudioCommand {
    CloseThread,
    EnumerateRecordingDevices,
    InitRecordingSession(UserRecordingSessionConfig),
    CloseRecordingSession,
    StartRecording(String),
    StopRecording,
    CancelRecording(String),
}

#[derive(Debug)]
pub enum AudioResponse {
    RecordingDeviceList(Vec<String>),
    Error(String),
    Success(String),
}

struct RecordingSessionSettings {
    device_name: String,
    bits_per_sample: u16,
}

struct RecordingSession {
    settings: RecordingSessionSettings,
    stream: Stream,
    spec: hound::WavSpec,
}

pub fn spawn_audio_thread(
    response_tx: mpsc::Sender<AudioResponse>,
) -> Result<mpsc::Sender<AudioCommand>, SendError<AudioCommand>> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || -> Result<(), SendError<AudioResponse>> {
        let host = cpal::default_host();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut current_recording_session: Option<RecordingSession> = None;

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::EnumerateRecordingDevices => {
                    let devices = host
                        .input_devices()
                        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
                        .unwrap_or_else(|e| {
                            let _ = response_tx.send(AudioResponse::Error(e.to_string()));
                            vec![]
                        });
                    response_tx.send(AudioResponse::RecordingDeviceList(devices))?;
                }
                AudioCommand::InitRecordingSession(recording_session_config) => {
                    if current_recording_session.is_some() {
                        response_tx.send(AudioResponse::Error(
                            "Stream already initialized".to_string(),
                        ))?;
                        continue;
                    }

                    let device = match host.input_devices() {
                        Ok(devices) => {
                            let device_result = devices
                                .into_iter()
                                .find(|d| matches!(d.name(), Ok(name) if name == recording_session_config.device_name));

                            match device_result {
                                Some(device) => device,
                                None => {
                                    let _ = response_tx
                                        .send(AudioResponse::Error("Device not found".to_string()));
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = response_tx.send(AudioResponse::Error(e.to_string()));
                            continue;
                        }
                    };

                    let default_device_config = match device.default_input_config() {
                        Ok(config) => config,
                        Err(e) => {
                            let _ = response_tx.send(AudioResponse::Error(e.to_string()));
                            continue;
                        }
                    };

                    let config: cpal::SupportedStreamConfig = default_device_config.into();
                    println!("Stream config: {:?}", config);
                    let writer_for_closure = Arc::clone(&writer_clone);
                    let response_tx_clone = response_tx.clone();

                    let bytes_per_sample = config.sample_format().sample_size();
                    let spec = hound::WavSpec {
                        channels: config.channels(),
                        sample_rate: config.sample_rate().0,
                        bits_per_sample: bytes_per_sample * 8 as _,
                        sample_format: if config.sample_format().is_float() {
                            hound::SampleFormat::Float
                        } else {
                            hound::SampleFormat::Int
                        },
                    };

                    let stream = match device.build_input_stream(
                        &config.into(),
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            if let Some(writer) = &mut *writer_for_closure.lock().unwrap() {
                                for &sample in data {
                                    println!("Processed sample: {}", sample);
                                    writer.write_sample(sample).unwrap();
                                }
                            }
                        },
                        move |err| {
                            let _ = response_tx_clone
                                .send(AudioResponse::Error(format!("Error in stream: {}", err)));
                        },
                        None,
                    ) {
                        Ok(stream) => stream,
                        Err(e) => {
                            let _ = response_tx.send(AudioResponse::Error(format!(
                                "Failed to build stream: {}",
                                e
                            )));
                            continue;
                        }
                    };

                    if let Err(e) = stream.play() {
                        let _ = response_tx.send(AudioResponse::Error(format!(
                            "Failed to start stream: {}",
                            e
                        )));
                        continue;
                    }

                    current_recording_session = Some(RecordingSession {
                        settings: RecordingSessionSettings {
                            device_name: recording_session_config.device_name,
                            bits_per_sample: recording_session_config.bits_per_sample,
                        },
                        stream: stream,
                        spec: spec,
                    });

                    response_tx.send(AudioResponse::Success(
                        "Recording session initialized".to_string(),
                    ))?;
                }
                AudioCommand::StartRecording(filename) => {
                    let recording_session = match &current_recording_session {
                        None => {
                            response_tx.send(AudioResponse::Error(
                                "Recording session not initialized".to_string(),
                            ))?;
                            continue;
                        }
                        Some(session) => session,
                    };

                    let new_writer =
                        match hound::WavWriter::create(&filename, recording_session.spec) {
                            Ok(writer) => writer,
                            Err(e) => {
                                response_tx.send(AudioResponse::Error(format!(
                                    "Failed to create WAV writer: {}",
                                    e
                                )))?;
                                continue;
                            }
                        };

                    *writer.lock().unwrap() = Some(new_writer);
                    response_tx.send(AudioResponse::Success("Recording started".to_string()))?;
                }
                AudioCommand::StopRecording => {
                    let wav_writer_result = writer
                        .lock()
                        .map_err(|e| format!("Failed to acquire lock: {}", e))
                        .and_then(|mut guard| {
                            guard
                                .take()
                                .ok_or_else(|| "No active recording to stop".to_string())
                        });

                    match wav_writer_result {
                        Ok(writer) => {
                            drop(writer);
                            response_tx
                                .send(AudioResponse::Success("Recording stopped".to_string()))?;
                        }
                        Err(err) => {
                            response_tx.send(AudioResponse::Error(err))?;
                        }
                    }
                }
                AudioCommand::CancelRecording(filename) => {
                    let wav_writer_result = writer
                        .lock()
                        .map_err(|e| format!("Failed to acquire lock: {}", e))
                        .and_then(|mut guard| {
                            guard
                                .take()
                                .ok_or_else(|| "No active recording to cancel".to_string())
                        });

                    match wav_writer_result {
                        Ok(writer) => {
                            drop(writer);
                            match std::fs::remove_file(&filename) {
                                Ok(_) => response_tx.send(AudioResponse::Success(
                                    "Recording cancelled and file deleted".to_string(),
                                ))?,
                                Err(e) => response_tx.send(AudioResponse::Error(format!(
                                    "Failed to delete partial recording: {}",
                                    e
                                )))?,
                            }
                        }
                        Err(err) => {
                            response_tx.send(AudioResponse::Error(err))?;
                        }
                    }
                }
                AudioCommand::CloseRecordingSession => {
                    if let Some(session) = current_recording_session.take() {
                        drop(session.stream);
                        response_tx.send(AudioResponse::Success(
                            "Recording session closed successfully".to_string(),
                        ))?;
                    } else {
                        response_tx.send(AudioResponse::Error(
                            "No active recording session to close".to_string(),
                        ))?;
                    }
                }
                AudioCommand::CloseThread => {
                    let _ = response_tx.send(AudioResponse::Success(
                        "Audio thread exiting...".to_string(),
                    ));
                    break;
                }
            }
        }

        Ok(())
    });

    Ok(tx)
}

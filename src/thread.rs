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
    writer: Option<hound::WavWriter<BufWriter<File>>>,
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

                    let sample_format = match recording_session_config.bits_per_sample {
                        16 | 24 => hound::SampleFormat::Int,
                        32 => hound::SampleFormat::Float,
                        _ => {
                            let _ = response_tx.send(AudioResponse::Error(format!(
                                "Unsupported bits per sample: {}",
                                recording_session_config.bits_per_sample
                            )));
                            continue;
                        }
                    };

                    let stream_config: cpal::StreamConfig = default_device_config.into();
                    let writer_for_closure = Arc::clone(&writer_clone);
                    let response_tx_clone = response_tx.clone();
                    // Create a spec that matches our input format
                    let spec = hound::WavSpec {
                        channels: stream_config.channels,
                        sample_rate: stream_config.sample_rate.0,
                        bits_per_sample: recording_session_config.bits_per_sample,
                        sample_format,
                    };

                    let stream = match device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut max_level = 0.0f32;
                            if let Some(writer) = &mut *writer_for_closure.lock().unwrap() {
                                for &sample in data {
                                    max_level = max_level.max(sample.abs());
                                    match spec.sample_format {
                                        hound::SampleFormat::Float => {
                                            let _ = writer.write_sample(sample);
                                        }
                                        hound::SampleFormat::Int => {
                                            // Convert float to integer based on bits_per_sample
                                            match spec.bits_per_sample {
                                                16 => {
                                                    let int_sample = (sample * 32767.0) as i16;
                                                    let _ = writer.write_sample(int_sample);
                                                }
                                                24 => {
                                                    let int_sample = (sample * 8388607.0) as i32;
                                                    let _ = writer.write_sample(int_sample);
                                                }
                                                _ => unreachable!(),
                                            };
                                        }
                                    }
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
                        writer: None,
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

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
    EnumerateAudioDevices,
    InitRecordingSession(UserRecordingSessionConfig),
    CloseRecordingSession,
    StartRecording(String),
    StopRecording,
    CancelRecording(String),
}

#[derive(Debug)]
pub enum AudioResponse {
    DeviceList(Vec<String>),
    StateUpdate(RecordingState),
    RecordingProgress { duration_ms: u64, peak_level: f32 },
    Error(String),
    Success(String),
}

pub fn spawn_audio_thread(
    response_tx: mpsc::Sender<AudioResponse>,
) -> Result<mpsc::Sender<AudioCommand>, SendError<AudioCommand>> {
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || -> Result<(), SendError<AudioResponse>> {
        let host = cpal::default_host();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = Arc::clone(&writer);

        let mut maybe_stream: Option<Stream> = None;
        let mut current_recording_session_wav_writer_config: Option<hound::WavSpec> = None;

        while let Ok(cmd) = rx.recv() {
            match cmd {
                AudioCommand::EnumerateAudioDevices => {
                    let devices = host
                        .input_devices()
                        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
                        .unwrap_or_else(|e| {
                            let _ = response_tx.send(AudioResponse::Error(e.to_string()));
                            vec![]
                        });
                    response_tx.send(AudioResponse::DeviceList(devices))?;
                }
                AudioCommand::InitRecordingSession(recording_session_config) => {
                    if maybe_stream.is_some() {
                        let _ = response_tx.send(AudioResponse::Error(
                            "Stream already initialized".to_string(),
                        ));
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

                    current_recording_session_wav_writer_config = Some(hound::WavSpec {
                        channels: default_device_config.channels(),
                        sample_rate: default_device_config.sample_rate().0,
                        bits_per_sample: recording_session_config.bits_per_sample,
                        sample_format,
                    });

                    let stream_config = default_device_config.into();
                    let writer_for_closure = Arc::clone(&writer_clone);
                    let response_tx_clone = response_tx.clone();

                    let stream = match device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            let mut max_level = 0.0f32;
                            if let Some(writer) = &mut *writer_for_closure.lock().unwrap() {
                                for &sample in data {
                                    max_level = max_level.max(sample.abs());
                                    let _ = writer.write_sample(sample);
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

                    maybe_stream = Some(stream);
                }
                AudioCommand::StartRecording(filename) => {
                    let wav_config = match current_recording_session_wav_writer_config {
                        None => {
                            response_tx.send(AudioResponse::Error(
                                "Recording session not initialized".to_string(),
                            ))?;
                            continue;
                        }
                        Some(config) => config,
                    };
                    let new_writer = match hound::WavWriter::create(&filename, wav_config) {
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
                    let mut writer_guard = writer.lock().unwrap();
                    let Some(writer) = writer_guard.take() else {
                        response_tx.send(AudioResponse::Error(
                            "No active recording to stop".to_string(),
                        ))?;
                        continue;
                    };

                    drop(writer);
                    let _ =
                        response_tx.send(AudioResponse::Success("Recording stopped".to_string()));
                }
                AudioCommand::CancelRecording(filename) => {
                    let mut writer_guard = writer.lock().unwrap();
                    let Some(writer) = writer_guard.take() else {
                        response_tx.send(AudioResponse::Error(
                            "No active recording to cancel".to_string(),
                        ))?;
                        continue;
                    };

                    drop(writer);
                    std::fs::remove_file(&filename).map_or_else(
                        |e| {
                            response_tx.send(AudioResponse::Error(format!(
                                "Failed to delete partial recording: {}",
                                e
                            )))
                        },
                        |_| {
                            response_tx.send(AudioResponse::Success(
                                "Recording cancelled and file deleted".to_string(),
                            ))
                        },
                    )?;
                }
                AudioCommand::CloseRecordingSession => {
                    if let Some(stream) = maybe_stream.take() {
                        drop(stream);
                        let _ = response_tx.send(AudioResponse::Success(
                            "Stream destroyed successfully".to_string(),
                        ));
                    } else {
                        let _ = response_tx.send(AudioResponse::Error(
                            "No active stream to destroy".to_string(),
                        ));
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

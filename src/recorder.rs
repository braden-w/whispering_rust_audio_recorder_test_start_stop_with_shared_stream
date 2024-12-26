use crate::thread::{spawn_audio_thread, AudioCommand, AudioResponse, UserRecordingSessionConfig};
use once_cell::sync::Lazy;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use thiserror::Error;
use tracing::{debug, error, info, warn};

// Global static mutex to hold the audio thread sender and state
static AUDIO_THREAD: Lazy<Mutex<Option<(Sender<AudioCommand>, Receiver<AudioResponse>)>>> =
    Lazy::new(|| Mutex::new(None));

// Track current recording state
static CURRENT_RECORDING: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("Audio thread not initialized")]
    ThreadNotInitialized,
    #[error("Failed to send command: {0}")]
    SendError(String),
    #[error("Failed to receive response: {0}")]
    ReceiveError(String),
    #[error("Audio error: {0}")]
    AudioError(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("No active recording session")]
    NoActiveRecording,
    #[error("Failed to acquire lock: {0}")]
    LockError(String),
}

type Result<T> = std::result::Result<T, RecorderError>;

#[derive(Debug)]
pub struct DeviceInfo {
    pub device_id: String,
    pub label: String,
}

fn ensure_thread_initialized() -> Result<()> {
    debug!("Ensuring thread is initialized...");
    let mut thread = AUDIO_THREAD
        .lock()
        .map_err(|e| RecorderError::LockError(e.to_string()))?;

    if thread.is_none() {
        debug!("Thread not initialized, creating new audio thread...");
        let (response_tx, response_rx) = mpsc::channel();
        let command_tx =
            spawn_audio_thread(response_tx).map_err(|e| RecorderError::SendError(e.to_string()))?;
        *thread = Some((command_tx, response_rx));
        info!("Audio thread created successfully");
    } else {
        debug!("Thread already initialized");
    }
    Ok(())
}

fn with_thread<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&Sender<AudioCommand>, &Receiver<AudioResponse>) -> Result<T>,
{
    ensure_thread_initialized()?;
    let thread = AUDIO_THREAD
        .lock()
        .map_err(|e| RecorderError::LockError(e.to_string()))?;
    let (tx, rx) = thread.as_ref().ok_or(RecorderError::ThreadNotInitialized)?;
    f(tx, rx)
}

pub fn enumerate_recording_devices() -> Result<Vec<DeviceInfo>> {
    debug!("Enumerating recording devices");
    with_thread(|tx, rx| {
        tx.send(AudioCommand::EnumerateRecordingDevices)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::RecordingDeviceList(devices)) => {
                info!("Found {} recording devices", devices.len());
                Ok(devices
                    .into_iter()
                    .map(|label| DeviceInfo {
                        device_id: label.clone(),
                        label,
                    })
                    .collect())
            }
            Ok(AudioResponse::Error(e)) => {
                error!("Failed to enumerate devices: {}", e);
                Err(RecorderError::AudioError(e))
            }
            Ok(_) => {
                error!("Unexpected response while enumerating devices");
                Err(RecorderError::AudioError("Unexpected response".to_string()))
            }
            Err(e) => {
                error!("Failed to receive device enumeration response: {}", e);
                Err(RecorderError::ReceiveError(e.to_string()))
            }
        }
    })
}

pub fn init_recording_session(settings: UserRecordingSessionConfig) -> Result<()> {
    info!(
        "Starting init_recording_session with settings: {:?}",
        settings
    );
    with_thread(|tx, rx| {
        debug!("Sending InitRecordingSession command...");
        tx.send(AudioCommand::InitRecordingSession(settings))
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        debug!("Waiting for response...");
        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                info!("Recording session initialized successfully");
                Ok(())
            }
            Ok(AudioResponse::Error(e)) => {
                error!("Failed to initialize recording session: {}", e);
                Err(RecorderError::AudioError(e))
            }
            Ok(_) => {
                error!("Unexpected response during initialization");
                Err(RecorderError::AudioError("Unexpected response".to_string()))
            }
            Err(e) => {
                error!("Failed to receive initialization response: {}", e);
                Err(RecorderError::ReceiveError(e.to_string()))
            }
        }
    })
}

pub fn close_recording_session() -> Result<()> {
    with_thread(|tx, rx| {
        tx.send(AudioCommand::CloseRecordingSession)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                *CURRENT_RECORDING.lock().unwrap() = None;
                Ok(())
            }
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

pub fn close_thread() -> Result<()> {
    let mut thread = AUDIO_THREAD
        .lock()
        .map_err(|e| RecorderError::LockError(e.to_string()))?;

    if let Some((tx, rx)) = thread.take() {
        debug!("Sending CloseThread command...");
        tx.send(AudioCommand::CloseThread)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                info!("Audio thread closed successfully");
                Ok(())
            }
            Ok(AudioResponse::Error(e)) => {
                error!("Error closing audio thread: {}", e);
                Err(RecorderError::AudioError(e))
            }
            Ok(_) => {
                error!("Unexpected response while closing thread");
                Err(RecorderError::AudioError("Unexpected response".to_string()))
            }
            Err(e) => {
                error!("Failed to receive thread close response: {}", e);
                Err(RecorderError::ReceiveError(e.to_string()))
            }
        }
    } else {
        debug!("No audio thread to close");
        Ok(())
    }
}

pub fn start_recording(recording_id: String) -> Result<()> {
    let filename = format!("{}.wav", recording_id);

    with_thread(|tx, rx| {
        tx.send(AudioCommand::StartRecording(filename.clone()))
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                *CURRENT_RECORDING.lock().unwrap() = Some(filename);
                Ok(())
            }
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

pub fn stop_recording() -> Result<Vec<u8>> {
    debug!("Stopping recording");
    with_thread(|tx, rx| {
        let current_recording = CURRENT_RECORDING
            .lock()
            .map_err(|e| RecorderError::LockError(e.to_string()))?
            .clone();
        let filename = current_recording.ok_or(RecorderError::NoActiveRecording)?;

        tx.send(AudioCommand::StopRecording)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                debug!("Reading WAV file contents");
                let contents = std::fs::read(&filename)?;

                debug!("Cleaning up temporary file");
                if let Err(e) = std::fs::remove_file(&filename) {
                    warn!("Failed to clean up temporary file: {}", e);
                }

                *CURRENT_RECORDING
                    .lock()
                    .map_err(|e| RecorderError::LockError(e.to_string()))? = None;

                info!("Recording stopped successfully ({} bytes)", contents.len());
                Ok(contents)
            }
            Ok(AudioResponse::Error(e)) => {
                error!("Failed to stop recording: {}", e);
                Err(RecorderError::AudioError(e))
            }
            Ok(_) => {
                error!("Unexpected response while stopping recording");
                Err(RecorderError::AudioError("Unexpected response".to_string()))
            }
            Err(e) => {
                error!("Failed to receive stop recording response: {}", e);
                Err(RecorderError::ReceiveError(e.to_string()))
            }
        }
    })
}

pub fn cancel_recording() -> Result<()> {
    with_thread(|tx, rx| {
        let current_recording = CURRENT_RECORDING.lock().unwrap().clone();
        let filename = current_recording.ok_or(RecorderError::NoActiveRecording)?;

        tx.send(AudioCommand::CancelRecording(filename))
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                *CURRENT_RECORDING.lock().unwrap() = None;
                Ok(())
            }
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

use crate::thread::{spawn_audio_thread, AudioCommand, AudioResponse, UserRecordingSessionConfig};
use once_cell::sync::Lazy;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;

// Global static mutex to hold the audio thread sender and state
static AUDIO_THREAD: Lazy<Mutex<Option<(Sender<AudioCommand>, Receiver<AudioResponse>)>>> =
    Lazy::new(|| Mutex::new(None));

// Track current recording state
static CURRENT_RECORDING: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug)]
pub enum RecorderError {
    ThreadNotInitialized,
    SendError(String),
    ReceiveError(String),
    AudioError(String),
    IoError(std::io::Error),
    NoActiveRecording,
}

impl std::fmt::Display for RecorderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecorderError::ThreadNotInitialized => write!(f, "Audio thread not initialized"),
            RecorderError::SendError(e) => write!(f, "Failed to send command: {}", e),
            RecorderError::ReceiveError(e) => write!(f, "Failed to receive response: {}", e),
            RecorderError::AudioError(e) => write!(f, "Audio error: {}", e),
            RecorderError::IoError(e) => write!(f, "IO error: {}", e),
            RecorderError::NoActiveRecording => write!(f, "No active recording session"),
        }
    }
}

impl std::error::Error for RecorderError {}

type Result<T> = std::result::Result<T, RecorderError>;

#[derive(Debug)]
pub struct DeviceInfo {
    pub device_id: String,
    pub label: String,
}

fn ensure_thread_initialized() -> Result<()> {
    let mut thread = AUDIO_THREAD.lock().unwrap();
    if thread.is_none() {
        let (response_tx, response_rx) = mpsc::channel();
        let command_tx =
            spawn_audio_thread(response_tx).map_err(|e| RecorderError::SendError(e.to_string()))?;
        *thread = Some((command_tx, response_rx));
    }
    Ok(())
}

fn with_thread<F, T>(f: F) -> Result<T>
where
    F: FnOnce(&Sender<AudioCommand>, &Receiver<AudioResponse>) -> Result<T>,
{
    ensure_thread_initialized()?;
    let thread = AUDIO_THREAD.lock().unwrap();
    let (tx, rx) = thread.as_ref().ok_or(RecorderError::ThreadNotInitialized)?;
    f(tx, rx)
}

pub async fn enumerate_recording_devices() -> Result<Vec<DeviceInfo>> {
    with_thread(|tx, rx| {
        tx.send(AudioCommand::EnumerateRecordingDevices)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::RecordingDeviceList(devices)) => Ok(devices
                .into_iter()
                .map(|label| DeviceInfo {
                    device_id: label.clone(),
                    label,
                })
                .collect()),
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

pub async fn init_recording_session(settings: UserRecordingSessionConfig) -> Result<()> {
    with_thread(|tx, rx| {
        tx.send(AudioCommand::InitRecordingSession(settings))
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => Ok(()),
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

pub async fn close_recording_session() -> Result<()> {
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

pub async fn start_recording(recording_id: String) -> Result<()> {
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

pub async fn stop_recording() -> Result<Vec<u8>> {
    with_thread(|tx, rx| {
        let current_recording = CURRENT_RECORDING.lock().unwrap().clone();
        let filename = current_recording.ok_or(RecorderError::NoActiveRecording)?;

        tx.send(AudioCommand::StopRecording)
            .map_err(|e| RecorderError::SendError(e.to_string()))?;

        match rx.recv() {
            Ok(AudioResponse::Success(_)) => {
                // Read the WAV file into memory
                let contents = std::fs::read(&filename).map_err(|e| RecorderError::IoError(e))?;

                // Clean up the file
                let _ = std::fs::remove_file(&filename);
                *CURRENT_RECORDING.lock().unwrap() = None;

                Ok(contents)
            }
            Ok(AudioResponse::Error(e)) => Err(RecorderError::AudioError(e)),
            Ok(_) => Err(RecorderError::AudioError("Unexpected response".to_string())),
            Err(e) => Err(RecorderError::ReceiveError(e.to_string())),
        }
    })
}

pub async fn cancel_recording() -> Result<()> {
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

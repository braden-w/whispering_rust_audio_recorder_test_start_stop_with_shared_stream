use crate::thread::{spawn_audio_thread, AudioCommand, AudioResponse, UserRecordingSessionConfig};
use once_cell::sync::Lazy;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Mutex;

// Global static mutex to hold the audio thread sender
static AUDIO_THREAD: Lazy<Mutex<Option<(Sender<AudioCommand>, Receiver<AudioResponse>)>>> =
    Lazy::new(|| Mutex::new(None));

const DEFAULT_BITS_PER_SAMPLE: u16 = 32;

fn enumerate_audio_devices() -> Result<Vec<String>, String> {
    let thread = AUDIO_THREAD.lock().map_err(|e| e.to_string())?;
    if let Some((sender, receiver)) = &*thread {
        sender
            .send(AudioCommand::EnumerateAudioDevices)
            .map_err(|e| e.to_string())?;
        match receiver.recv().map_err(|e| e.to_string())? {
            AudioResponse::DeviceList(devices) => Ok(devices),
            _ => Err("Unexpected response from audio thread".to_string()),
        }
    } else {
        Err("No recording session found".to_string())
    }
}

#[tauri::command]
fn start_from_new_recording_session(
    device_name: String,
    bits_per_sample: u16,
    file_path: String,
) -> Result<(), String> {
    let thread = AUDIO_THREAD.lock().map_err(|e| e.to_string())?.take();
    if thread.is_some() {
        return Err(
            "Attempted to start a new recording session while one already exists".to_string(),
        );
    }

    let sender = spawn_audio_thread()?;
    sender
        .send(AudioCommand::InitRecordingSession(
            UserRecordingSessionConfig {
                device_name,
                bits_per_sample,
            },
        ))
        .map_err(|e| e.to_string())?;
    sender
        .send(AudioCommand::StartRecording(file_path))
        .map_err(|e| e.to_string())?;
    *AUDIO_THREAD.lock().map_err(|e| e.to_string())? = Some((sender, receiver));

    Ok(())
}

#[tauri::command]
fn start_from_existing_recording_session(file_path: String) -> Result<(), String> {
    let maybe_thread_sender = AUDIO_THREAD.lock().map_err(|e| e.to_string())?.take();

    if maybe_thread_sender.is_none() {
        return Err("No recording session found".to_string());
    }
    let thread_sender = maybe_thread_sender.unwrap();
    thread_sender
        .send(AudioCommand::StartRecording(file_path))
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn end_recording_session() -> Result<(), String> {
    let maybe_thread_sender = AUDIO_THREAD.lock().map_err(|e| e.to_string())?.take();

    if maybe_thread_sender.is_none() {
        return Err("No recording session found".to_string());
    }

    let thread_sender = maybe_thread_sender.unwrap();
    thread_sender
        .send(AudioCommand::CloseThread)
        .map_err(|e| e.to_string())?;

    *AUDIO_THREAD.lock().map_err(|e| e.to_string())? = None;

    Ok(())
}

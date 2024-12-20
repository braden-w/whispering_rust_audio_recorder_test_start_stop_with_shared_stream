use axum::{extract::State, response::IntoResponse, routing::get, Router};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

// Commands that can be sent to the audio thread
#[derive(Debug)]
enum AudioCommand {
    Start,
    Stop,
}

// Shared state between handlers
struct AppState {
    audio_tx: mpsc::Sender<AudioCommand>,
}

fn spawn_audio_thread() -> mpsc::Sender<AudioCommand> {
    let (tx, mut rx) = mpsc::channel(1);

    std::thread::spawn(move || {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .expect("no input device available");
        let config = device
            .default_input_config()
            .expect("no default input config");

        let spec = hound::WavSpec {
            channels: config.channels(),
            sample_rate: config.sample_rate().0,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let writer = Arc::new(Mutex::new(
            hound::WavWriter::create("output.wav", spec).unwrap(),
        ));
        let writer_clone = writer.clone();
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut writer = writer_clone.lock().unwrap();
                    for &sample in data {
                        writer.write_sample(sample).unwrap();
                    }
                },
                |err| eprintln!("Error in stream: {}", err),
                None,
            )
            .expect("Failed to build input stream");

        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                AudioCommand::Start => {
                    println!("Starting recording");
                    if let Err(e) = stream.play() {
                        eprintln!("Failed to start stream: {}", e);
                    }
                }
                AudioCommand::Stop => {
                    println!("Stopping recording");
                    if let Err(e) = stream.pause() {
                        eprintln!("Failed to stop stream: {}", e);
                    }
                }
            }
        }

        println!("Audio thread shutting down");
    });

    tx
}

async fn start_recording(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Err(e) = state.audio_tx.send(AudioCommand::Start).await {
        return format!("Failed to start recording: {}", e);
    }
    "Recording started".to_string()
}

async fn stop_recording(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Err(e) = state.audio_tx.send(AudioCommand::Stop).await {
        return format!("Failed to stop recording: {}", e);
    }
    "Recording stopped".to_string()
}

#[tokio::main]
async fn main() {
    // Initialize the audio thread and get the sender
    let audio_tx = spawn_audio_thread();

    // Create shared state
    let state = Arc::new(AppState { audio_tx });

    // Build the router
    let app = Router::new()
        .route("/start", get(start_recording))
        .route("/stop", get(stop_recording))
        .with_state(state);

    // Start the server
    println!("Server starting on http://localhost:3000");
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

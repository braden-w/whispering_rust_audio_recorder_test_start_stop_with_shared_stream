use axum::{extract::State, response::IntoResponse, routing::get, Router};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    fs::File,
    io::BufWriter,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

// Commands that can be sent to the audio thread
#[derive(Debug)]
enum AudioCommand {
    Start(String), // Add filename parameter
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
        let config_clone = config.clone();

        // Keep writer in an Option to swap it when starting new recordings
        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = writer.clone();

        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Some(writer) = &mut *writer_clone.lock().unwrap() {
                        for &sample in data {
                            writer.write_sample(sample).unwrap();
                        }
                    }
                },
                |err| eprintln!("Error in stream: {}", err),
                None,
            )
            .expect("Failed to build input stream");

        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                AudioCommand::Start(filename) => {
                    let spec = hound::WavSpec {
                        channels: config_clone.channels(),
                        sample_rate: config_clone.sample_rate().0,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    };

                    // Create new writer for the new file
                    *writer.lock().unwrap() =
                        Some(hound::WavWriter::create(&filename, spec).unwrap());

                    stream.play().unwrap();
                }
                AudioCommand::Stop => {
                    stream.pause().unwrap();
                    // Finalize the current writer
                    if let Some(writer) = writer.lock().unwrap().take() {
                        writer.finalize().unwrap();
                    }
                }
            }
        }

        println!("Audio thread shutting down");
    });

    tx
}

async fn start_recording(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Err(e) = state.audio_tx.send(AudioCommand::Start("output.wav".to_string())).await
    {
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

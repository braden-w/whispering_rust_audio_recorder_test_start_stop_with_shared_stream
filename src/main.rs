use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    fmt,
    fs::File,
    io::BufWriter,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc;

// Configuration constants
const SERVER_HOST: [u8; 4] = [127, 0, 0, 1];
const SERVER_PORT: u16 = 3000;
const CHANNEL_BUFFER_SIZE: usize = 1;
const WAV_BITS_PER_SAMPLE: u16 = 32;

// Custom error type for audio operations
#[derive(Debug)]
enum AudioError {
    DeviceError(String),
    StreamError(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceError(msg) => write!(f, "Device error: {}", msg),
            AudioError::StreamError(msg) => write!(f, "Stream error: {}", msg),
        }
    }
}

// Commands that can be sent to the audio thread
#[derive(Debug)]
enum AudioCommand {
    Start(String),
    Stop,
}

// Shared state between handlers
#[derive(Clone)]
struct AppState {
    audio_tx: mpsc::Sender<AudioCommand>,
}

// Response type for API endpoints
struct ApiResponse {
    status: StatusCode,
    message: String,
}

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

fn create_wav_spec(config: &cpal::SupportedStreamConfig) -> hound::WavSpec {
    hound::WavSpec {
        channels: config.channels(),
        sample_rate: config.sample_rate().0,
        bits_per_sample: WAV_BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Float,
    }
}

fn spawn_audio_thread() -> Result<mpsc::Sender<AudioCommand>, AudioError> {
    let (tx, mut rx) = mpsc::channel(CHANNEL_BUFFER_SIZE);

    std::thread::spawn(move || {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| AudioError::DeviceError("No input device available".to_string()))?;

        let config = device
            .default_input_config()
            .map_err(|e| AudioError::DeviceError(e.to_string()))?;
        let config_clone = config.clone();

        let writer = Arc::new(Mutex::new(None::<hound::WavWriter<BufWriter<File>>>));
        let writer_clone = writer.clone();

        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if let Some(writer) = &mut *writer_clone.lock().unwrap() {
                        for &sample in data {
                            let _ = writer.write_sample(sample);
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| AudioError::StreamError(e.to_string()))?;

        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                AudioCommand::Start(filename) => {
                    let spec = create_wav_spec(&config_clone);
                    match hound::WavWriter::create(&filename, spec) {
                        Ok(new_writer) => {
                            *writer.lock().unwrap() = Some(new_writer);
                            let _ = stream.play();
                        }
                        Err(e) => eprintln!("Failed to create WAV writer: {}", e),
                    }
                }
                AudioCommand::Stop => {
                    let _ = stream.pause();
                    if let Some(writer) = writer.lock().unwrap().take() {
                        let _ = writer.finalize();
                    }
                }
            }
        }

        Ok::<(), AudioError>(())
    });

    Ok(tx)
}

async fn start_recording(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state
        .audio_tx
        .send(AudioCommand::Start("output.wav".to_string()))
        .await
    {
        Ok(_) => ApiResponse {
            status: StatusCode::OK,
            message: "Recording started".to_string(),
        },
        Err(e) => ApiResponse {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Failed to start recording: {}", e),
        },
    }
}

async fn stop_recording(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.audio_tx.send(AudioCommand::Stop).await {
        Ok(_) => ApiResponse {
            status: StatusCode::OK,
            message: "Recording stopped".to_string(),
        },
        Err(e) => ApiResponse {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: format!("Failed to stop recording: {}", e),
        },
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let audio_tx =
        spawn_audio_thread().map_err(|e| format!("Failed to spawn audio thread: {}", e))?;

    let state = Arc::new(AppState { audio_tx });
    let app = Router::new()
        .route("/start", get(start_recording))
        .route("/stop", get(stop_recording))
        .with_state(state);

    println!("Server starting on http://localhost:{}", SERVER_PORT);
    let addr = SocketAddr::from((SERVER_HOST, SERVER_PORT));

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .map_err(|e| e.into())
}

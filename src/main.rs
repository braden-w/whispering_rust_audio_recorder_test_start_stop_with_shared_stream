use anyhow::Result;
use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

mod cli;

use cli::{AudioCommand, Cli};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        AudioCommand::ListDevices => {
            let host = cpal::default_host();
            let devices = host.input_devices()?;
            println!("\nInput Devices:");
            for device in devices {
                println!("  {}", device.name()?);
            }
        }
        AudioCommand::Record { duration, output } => {
            let host = cpal::default_host();

            let device = host
                .default_input_device()
                .expect("no input device available");

            println!("Recording using default input device: {}", device.name()?);
            println!(
                "Recording for {} seconds to file: {}",
                duration,
                output.display()
            );

            let config = device.default_input_config()?;
            println!("Using input config: {:?}", config);
            let spec = hound::WavSpec {
                channels: config.channels(),
                sample_rate: config.sample_rate().0,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };

            let writer = Arc::new(Mutex::new(hound::WavWriter::create("output.wav", spec)?));
            let writer_clone = writer.clone();
            let stream = device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut writer = writer_clone.lock().unwrap();
                    for &sample in data {
                        writer.write_sample(sample).unwrap();
                    }
                },
                |err| eprintln!("Error in stream: {}", err),
                None,
            )?;
            stream.play()?;
            std::thread::sleep(std::time::Duration::from_secs(5)); // Record for 5 seconds
            drop(stream);

            return Ok(());
        }
    }

    Ok(())
}

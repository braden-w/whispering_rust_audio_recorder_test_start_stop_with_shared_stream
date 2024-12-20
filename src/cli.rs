use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: AudioCommand,
}

#[derive(Subcommand)]
pub enum AudioCommand {
    /// List all available audio input devices
    ListDevices,

    /// Record audio from the default input device
    Record {
        /// Duration to record in seconds
        #[arg(short, long, default_value_t = 5)]
        duration: u64,

        /// Output file path (WAV format)
        #[arg(short, long)]
        output: PathBuf,
    },
}

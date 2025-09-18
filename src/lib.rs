mod cli;
mod config;
mod crypto;
mod envscan;
mod examples;
mod gitignore;
mod hooks;
mod init;
mod precommit;
mod types;
mod util;

pub use crate::cli::Cli;
pub use crate::types::*;

use clap::Parser;
use std::io;

pub fn run() -> io::Result<()> {
    let cli = Cli::parse();
    crate::cli::dispatch(cli)
}

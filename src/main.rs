use clap::Parser;
use mec::cli::{Cli, Command};
use mec::diagnostics::doctor;
use mec::hardware::{LinuxSysfsReader, SystemPaths};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        None => println!("MEC — MSI EC Control Center"),
        Some(Command::Doctor) => {
            let report = doctor(SystemPaths::new(cli.sys_root), LinuxSysfsReader);
            println!("{report}");
        }
        Some(Command::Status) => {
            match mec::cli::status::status(SystemPaths::new(cli.sys_root), LinuxSysfsReader) {
                Ok(report) => println!("{report}"),
                Err(error) => {
                    eprintln!("MEC status unavailable: {error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

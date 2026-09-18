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
        Some(Command::Monitor { interval }) => {
            match mec::cli::monitor::run_monitor(
                SystemPaths::new(cli.sys_root),
                interval,
                &mut std::io::stdout(),
                &mut std::io::stderr(),
            ) {
                Ok(()) => {}
                Err(error) => {
                    eprintln!("MEC monitor unavailable: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some(Command::Status { json }) => {
            match mec::cli::status::status(SystemPaths::new(cli.sys_root), LinuxSysfsReader) {
                Ok(report) => {
                    if let Some(error) = report.snapshot_error() {
                        eprintln!("MEC status degraded: {error}");
                    }
                    if json {
                        match report.to_json() {
                            Ok(document) => println!("{document}"),
                            Err(error) => {
                                eprintln!("MEC status JSON unavailable: {error}");
                                std::process::exit(1);
                            }
                        }
                    } else {
                        println!("{report}");
                    }
                }
                Err(error) => {
                    eprintln!("MEC status unavailable: {error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

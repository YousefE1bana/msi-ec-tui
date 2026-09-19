use std::io::IsTerminal;

use clap::Parser;
use mec::cli::{BatteryCommand, Cli, Command, FanCommand, ProfileCommand};
use mec::diagnostics::doctor;
use mec::hardware::{HardwareCommand, LinuxSysfsReader, SystemPaths};

/// Runs one parsed control through the composed safe pipeline. Prints the
/// success line only after verified execution; failures surface the typed
/// error on stderr with a non-zero exit. No privilege elevation, no retry.
fn run_hardware_command(paths: SystemPaths, command: HardwareCommand, success: &str) {
    match mec::safety::execute_hardware_command(paths, &command) {
        Ok(()) => println!("{success}"),
        Err(error) => {
            eprintln!("MEC control failed: {error}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        None => {
            if mec::tui::should_launch_tui(
                std::io::stdin().is_terminal(),
                std::io::stdout().is_terminal(),
            ) {
                if let Err(error) = mec::tui::run_tui(SystemPaths::new(cli.sys_root)) {
                    eprintln!("MEC TUI unavailable: {error}");
                    std::process::exit(1);
                }
            } else {
                println!("MEC — MSI EC Control Center");
            }
        }
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
        Some(Command::Fan { command }) => match command {
            FanCommand::Mode { mode } => {
                let message = format!("MEC control applied: fan mode = {mode}");
                run_hardware_command(
                    SystemPaths::new(cli.sys_root),
                    HardwareCommand::SetFanMode(mode),
                    &message,
                );
            }
        },
        Some(Command::Shift { mode }) => {
            let message = format!("MEC control applied: shift mode = {mode}");
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetShiftMode(mode),
                &message,
            );
        }
        Some(Command::CoolerBoost { state }) => {
            let message = format!("MEC control applied: cooler boost = {}", state.as_str());
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetCoolerBoost(state.as_bool()),
                &message,
            );
        }
        Some(Command::SuperBattery { state }) => {
            let message = format!("MEC control applied: super battery = {}", state.as_str());
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetSuperBattery(state.as_bool()),
                &message,
            );
        }
        Some(Command::Webcam { state }) => {
            let message = format!("MEC control applied: webcam = {}", state.as_str());
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetWebcam(state.as_bool()),
                &message,
            );
        }
        Some(Command::WebcamBlock { state }) => {
            let message = format!("MEC control applied: webcam block = {}", state.as_str());
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetWebcamBlock(state.as_bool()),
                &message,
            );
        }
        Some(Command::KeyboardBacklight { level }) => {
            let message = format!("MEC control applied: keyboard backlight = {level}");
            run_hardware_command(
                SystemPaths::new(cli.sys_root),
                HardwareCommand::SetKeyboardBacklight(level),
                &message,
            );
        }
        Some(Command::Battery { command }) => match command {
            BatteryCommand::Limit { threshold } => {
                let message = format!(
                    "MEC control applied: battery limit = {}%",
                    threshold.end_percent()
                );
                run_hardware_command(
                    SystemPaths::new(cli.sys_root),
                    HardwareCommand::SetBatteryThreshold(threshold),
                    &message,
                );
            }
        },
        Some(Command::Profile { command }) => match command {
            ProfileCommand::List => match mec::profiles::ProfileStore::user_default() {
                Ok(store) => match store.list() {
                    Ok(slugs) => {
                        print!("{}", mec::cli::profile::render_list_text(&slugs));
                    }
                    Err(error) => {
                        eprintln!("MEC profile failed: {error}");
                        std::process::exit(1);
                    }
                },
                Err(error) => {
                    eprintln!("MEC profile failed: {error}");
                    std::process::exit(1);
                }
            },
            ProfileCommand::Show { profile } => {
                if let Ok(preset) = mec::profiles::BuiltinPreset::from_slug(&profile) {
                    match mec::cli::profile::discover_capabilities(SystemPaths::new(cli.sys_root)) {
                        Ok(capabilities) => {
                            match mec::cli::profile::resolve_builtin(&capabilities, preset) {
                                Ok(resolved) => print!(
                                    "{}",
                                    mec::cli::profile::render_show_text(
                                        preset.slug(),
                                        mec::cli::ProfileSource::Builtin,
                                        &resolved
                                    )
                                ),
                                Err(error) => {
                                    eprintln!("MEC profile failed: {error}");
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("MEC profile failed: {error}");
                            std::process::exit(1);
                        }
                    }
                } else {
                    match mec::profiles::ProfileStore::user_default() {
                        Ok(store) => match mec::cli::profile::resolve_custom(&store, &profile) {
                            Ok((slug, resolved)) => print!(
                                "{}",
                                mec::cli::profile::render_show_text(
                                    slug.as_str(),
                                    mec::cli::ProfileSource::Custom,
                                    &resolved
                                )
                            ),
                            Err(error) => {
                                eprintln!("MEC profile failed: {error}");
                                std::process::exit(1);
                            }
                        },
                        Err(error) => {
                            eprintln!("MEC profile failed: {error}");
                            std::process::exit(1);
                        }
                    }
                }
            }
            ProfileCommand::Apply { profile } => {
                let resolved = if let Ok(preset) = mec::profiles::BuiltinPreset::from_slug(&profile)
                {
                    match mec::cli::profile::discover_capabilities(SystemPaths::new(
                        cli.sys_root.clone(),
                    )) {
                        Ok(capabilities) => {
                            match mec::cli::profile::resolve_builtin(&capabilities, preset) {
                                Ok(resolved) => resolved,
                                Err(error) => {
                                    eprintln!("MEC profile failed: {error}");
                                    std::process::exit(1);
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("MEC profile failed: {error}");
                            std::process::exit(1);
                        }
                    }
                } else {
                    match mec::profiles::ProfileStore::user_default() {
                        Ok(store) => match mec::cli::profile::resolve_custom(&store, &profile) {
                            Ok((_, resolved)) => resolved,
                            Err(error) => {
                                eprintln!("MEC profile failed: {error}");
                                std::process::exit(1);
                            }
                        },
                        Err(error) => {
                            eprintln!("MEC profile failed: {error}");
                            std::process::exit(1);
                        }
                    }
                };
                match mec::safety::apply_profile(SystemPaths::new(cli.sys_root), &resolved) {
                    Ok(report) => {
                        println!(
                            "{}",
                            mec::cli::profile::render_apply_success(&resolved, &report)
                        );
                    }
                    Err(error) => {
                        eprintln!("MEC profile failed: {error}");
                        std::process::exit(1);
                    }
                }
            }
        },
    }
}

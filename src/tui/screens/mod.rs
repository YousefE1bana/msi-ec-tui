//! Read-only screen collection with AppState dispatcher.
//!
//! Each screen renders already-sampled [`LiveHardware`] state plus injected
//! startup [`Capabilities`]. Renderers never sample, discover, or probe.

mod battery;
mod dashboard;
mod devices;
mod diagnostics;
mod fans;
mod performance;
#[cfg(test)]
pub(crate) mod support;

pub use battery::render_battery;
pub use dashboard::render_dashboard;
pub use devices::render_devices;
pub use diagnostics::render_diagnostics;
pub use fans::render_fans;
pub use performance::render_performance;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::{AppState, LiveHardware, Screen};
use crate::hardware::{Capabilities, EcBackend};

/// Renders the screen selected by `app`, ignoring help visibility until
/// Task 6 owns the overlay.
pub fn render_screen<B: EcBackend>(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    live: &LiveHardware<B>,
    capabilities: &Capabilities,
) {
    match app.current_screen() {
        Screen::Dashboard => render_dashboard(frame, area, live),
        Screen::Performance => render_performance(frame, area, live, capabilities),
        Screen::Fans => render_fans(frame, area, live, capabilities),
        Screen::Battery => render_battery(frame, area, live, capabilities),
        Screen::Devices => render_devices(frame, area, live, capabilities),
        Screen::Diagnostics => render_diagnostics(frame, area, live, capabilities),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use crate::app::{AppAction, AppState, Screen};
    use crate::hardware::SupportMode;

    use super::support::{full_capabilities, healthy_snapshot, live_for, screen_text};
    use super::{
        render_battery, render_devices, render_diagnostics, render_fans, render_performance,
        render_screen,
    };

    fn app_on(screen: Screen) -> AppState {
        let mut app = AppState::default();
        app.apply(AppAction::GoTo(screen));
        app
    }

    fn dispatched(screen: Screen) -> String {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let app = app_on(screen);
        screen_text(100, 30, |frame| {
            render_screen(frame, frame.area(), &app, &live, &capabilities);
        })
    }

    #[test]
    fn dashboard_dispatch_renders_dashboard() {
        assert!(dispatched(Screen::Dashboard).contains("THERMALS"));
    }

    #[test]
    fn performance_dispatch_renders_performance() {
        assert!(dispatched(Screen::Performance).contains("Available Shift Modes"));
    }

    #[test]
    fn fans_dispatch_renders_fans() {
        assert!(dispatched(Screen::Fans).contains("CPU Fan Telemetry"));
    }

    #[test]
    fn battery_dispatch_renders_battery() {
        assert!(dispatched(Screen::Battery).contains("Threshold Control"));
    }

    #[test]
    fn devices_dispatch_renders_devices() {
        assert!(dispatched(Screen::Devices).contains("Fn Key"));
    }

    #[test]
    fn diagnostics_dispatch_renders_diagnostics() {
        assert!(dispatched(Screen::Diagnostics).contains("Manufacturer"));
    }

    #[test]
    fn rendering_all_screens_performs_zero_backend_calls() {
        let (live, calls) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        assert_eq!(calls.get(), 1);
        for screen in Screen::ALL {
            let app = app_on(screen);
            let _ = screen_text(100, 30, |frame| {
                render_screen(frame, frame.area(), &app, &live, &capabilities);
            });
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn every_screen_survives_tiny_area() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        for screen in Screen::ALL {
            let app = app_on(screen);
            let text = screen_text(20, 8, |frame| {
                render_screen(frame, frame.area(), &app, &live, &capabilities);
            });
            assert!(text.contains("Terminal too small"), "{screen:?}");
        }
    }

    #[test]
    fn every_screen_survives_zero_area() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        for screen in Screen::ALL {
            let app = app_on(screen);
            let backend = TestBackend::new(10, 5);
            let mut terminal = Terminal::new(backend).expect("test terminal constructs");
            terminal
                .draw(|frame| {
                    render_screen(frame, Rect::new(0, 0, 0, 0), &app, &live, &capabilities);
                })
                .expect("zero-area screen draws");
        }
    }

    #[test]
    fn individual_renderers_stay_reachable() {
        let (live, _) = live_for(vec![Ok(healthy_snapshot())], SupportMode::Ready, 1);
        let capabilities = full_capabilities();
        let _ = screen_text(100, 30, |frame| {
            render_performance(frame, frame.area(), &live, &capabilities);
        });
        let _ = screen_text(100, 30, |frame| {
            render_fans(frame, frame.area(), &live, &capabilities);
        });
        let _ = screen_text(100, 30, |frame| {
            render_battery(frame, frame.area(), &live, &capabilities);
        });
        let _ = screen_text(100, 30, |frame| {
            render_devices(frame, frame.area(), &live, &capabilities);
        });
        let _ = screen_text(100, 30, |frame| {
            render_diagnostics(frame, frame.area(), &live, &capabilities);
        });
    }
}

//! Application navigation state: screen model and transitions.
//!
//! Interaction state only. Hardware snapshots, device identity, and
//! terminal types are deliberately absent; later tasks introduce the live
//! data model and event mapping separately.

use super::action::AppAction;

/// Read-only TUI screen in canonical navigation order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    /// Overview of thermals, performance, battery, and devices.
    #[default]
    Dashboard,
    /// Shift mode, fan mode, cooler boost, super battery.
    Performance,
    /// Fan telemetry and mode controls.
    Fans,
    /// Charge, thresholds, AC state.
    Battery,
    /// Webcam, backlight, function keys.
    Devices,
    /// Capability-aware built-in profile catalog (read-only).
    Profiles,
    /// Compatibility and diagnostics report.
    Diagnostics,
}

impl Screen {
    /// Canonical PLAN-006 navigation order.
    pub const ALL: [Screen; 7] = [
        Screen::Dashboard,
        Screen::Performance,
        Screen::Fans,
        Screen::Battery,
        Screen::Devices,
        Screen::Profiles,
        Screen::Diagnostics,
    ];

    /// Stable user-facing title.
    pub fn title(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Performance => "Performance",
            Screen::Fans => "Fans",
            Screen::Battery => "Battery",
            Screen::Devices => "Devices",
            Screen::Profiles => "Profiles",
            Screen::Diagnostics => "Diagnostics",
        }
    }

    fn position(self) -> usize {
        Self::ALL
            .iter()
            .position(|screen| *screen == self)
            .expect("screen is a member of ALL")
    }

    /// Advance with wraparound.
    pub fn next(self) -> Self {
        Self::ALL[(self.position() + 1) % Self::ALL.len()]
    }

    /// Move back with wraparound.
    pub fn previous(self) -> Self {
        Self::ALL[(self.position() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Interaction and navigation state for the read-only TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppState {
    current_screen: Screen,
    help_visible: bool,
    should_quit: bool,
}

impl AppState {
    /// Screen currently displayed.
    pub fn current_screen(&self) -> Screen {
        self.current_screen
    }

    /// Whether the help overlay is visible.
    pub fn help_visible(&self) -> bool {
        self.help_visible
    }

    /// Whether the application should exit.
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Applies one terminal-independent action. `MoveUp`/`MoveDown` keep
    /// legacy screen-navigation fallback semantics here; row-driven
    /// screens are dispatched contextually above this layer.
    pub fn apply(&mut self, action: AppAction) {
        match action {
            AppAction::Quit => self.should_quit = true,
            AppAction::NextScreen => self.current_screen = self.current_screen.next(),
            AppAction::PreviousScreen => {
                self.current_screen = self.current_screen.previous();
            }
            AppAction::MoveUp => self.current_screen = self.current_screen.previous(),
            AppAction::MoveDown => self.current_screen = self.current_screen.next(),
            AppAction::GoTo(screen) => self.current_screen = screen,
            AppAction::ToggleHelp => self.help_visible = !self.help_visible,
            AppAction::ShowHelp => self.help_visible = true,
            AppAction::HideHelp => self.help_visible = false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screen_is_dashboard() {
        assert_eq!(Screen::default(), Screen::Dashboard);
    }

    #[test]
    fn canonical_screen_order() {
        assert_eq!(
            Screen::ALL,
            [
                Screen::Dashboard,
                Screen::Performance,
                Screen::Fans,
                Screen::Battery,
                Screen::Devices,
                Screen::Profiles,
                Screen::Diagnostics,
            ]
        );
    }

    #[test]
    fn stable_titles_match_exactly() {
        assert_eq!(Screen::Dashboard.title(), "Dashboard");
        assert_eq!(Screen::Performance.title(), "Performance");
        assert_eq!(Screen::Fans.title(), "Fans");
        assert_eq!(Screen::Battery.title(), "Battery");
        assert_eq!(Screen::Devices.title(), "Devices");
        assert_eq!(Screen::Profiles.title(), "Profiles");
        assert_eq!(Screen::Diagnostics.title(), "Diagnostics");
    }

    #[test]
    fn next_walks_canonical_order_with_wrap() {
        assert_eq!(Screen::Dashboard.next(), Screen::Performance);
        assert_eq!(Screen::Performance.next(), Screen::Fans);
        assert_eq!(Screen::Fans.next(), Screen::Battery);
        assert_eq!(Screen::Battery.next(), Screen::Devices);
        assert_eq!(Screen::Devices.next(), Screen::Profiles);
        assert_eq!(Screen::Profiles.next(), Screen::Diagnostics);
        assert_eq!(Screen::Diagnostics.next(), Screen::Dashboard);
    }

    #[test]
    fn previous_walks_reverse_order_with_wrap() {
        assert_eq!(Screen::Dashboard.previous(), Screen::Diagnostics);
        assert_eq!(Screen::Diagnostics.previous(), Screen::Profiles);
        assert_eq!(Screen::Profiles.previous(), Screen::Devices);
        assert_eq!(Screen::Devices.previous(), Screen::Battery);
        assert_eq!(Screen::Battery.previous(), Screen::Fans);
        assert_eq!(Screen::Fans.previous(), Screen::Performance);
        assert_eq!(Screen::Performance.previous(), Screen::Dashboard);
    }

    #[test]
    fn default_state_starts_dashboard_hidden_not_quit() {
        let state = AppState::default();
        assert_eq!(state.current_screen(), Screen::Dashboard);
        assert!(!state.help_visible());
        assert!(!state.should_quit());
    }

    #[test]
    fn next_screen_updates_current_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::NextScreen);
        assert_eq!(state.current_screen(), Screen::Performance);
    }

    #[test]
    fn previous_screen_updates_current_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::PreviousScreen);
        assert_eq!(state.current_screen(), Screen::Diagnostics);
    }

    #[test]
    fn goto_selects_exact_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::GoTo(Screen::Fans));
        assert_eq!(state.current_screen(), Screen::Fans);
    }

    #[test]
    fn goto_selects_profiles_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::GoTo(Screen::Profiles));
        assert_eq!(state.current_screen(), Screen::Profiles);
    }

    #[test]
    fn quit_sets_should_quit() {
        let mut state = AppState::default();
        state.apply(AppAction::Quit);
        assert!(state.should_quit());
    }

    #[test]
    fn toggle_help_flips_visibility() {
        let mut state = AppState::default();
        state.apply(AppAction::ToggleHelp);
        assert!(state.help_visible());
        state.apply(AppAction::ToggleHelp);
        assert!(!state.help_visible());
    }

    #[test]
    fn show_hide_help_are_idempotent() {
        let mut state = AppState::default();
        state.apply(AppAction::ShowHelp);
        state.apply(AppAction::ShowHelp);
        assert!(state.help_visible());
        state.apply(AppAction::HideHelp);
        state.apply(AppAction::HideHelp);
        assert!(!state.help_visible());
    }

    #[test]
    fn move_up_falls_back_to_previous_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::MoveUp);
        assert_eq!(state.current_screen(), Screen::Diagnostics);
    }

    #[test]
    fn move_down_falls_back_to_next_screen() {
        let mut state = AppState::default();
        state.apply(AppAction::MoveDown);
        assert_eq!(state.current_screen(), Screen::Performance);
    }

    #[test]
    fn navigation_does_not_mutate_help_visibility() {
        let mut state = AppState::default();
        state.apply(AppAction::ShowHelp);
        state.apply(AppAction::NextScreen);
        assert!(state.help_visible());
        state.apply(AppAction::HideHelp);
        state.apply(AppAction::GoTo(Screen::Battery));
        assert!(!state.help_visible());
    }

    #[test]
    fn non_quit_actions_do_not_set_should_quit() {
        for action in [
            AppAction::NextScreen,
            AppAction::PreviousScreen,
            AppAction::MoveUp,
            AppAction::MoveDown,
            AppAction::GoTo(Screen::Fans),
            AppAction::ToggleHelp,
            AppAction::ShowHelp,
            AppAction::HideHelp,
        ] {
            let mut state = AppState::default();
            state.apply(action);
            assert!(!state.should_quit());
        }
    }
}

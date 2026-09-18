//! Key-to-action translation for the read-only TUI.
//!
//! Pure mapping from Crossterm key events to terminal-independent
//! [`AppAction`] intents. Release events are ignored; shortcuts reject
//! incidental modifiers conservatively.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::app::{AppAction, Screen};

/// Translates one key event into an application action.
///
/// Returns `None` for release events and unmapped keys. Modifier handling
/// is conservative: shortcuts fire only with their natural modifiers, so
/// `Alt+q` or `Ctrl+q` never quit and `Alt+1` never jumps screens.
pub fn action_for_key(key: KeyEvent) -> Option<AppAction> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Char('q') if key.modifiers.is_empty() => Some(AppAction::Quit),
        KeyCode::Char('Q') if allows_only_shift(key.modifiers) => Some(AppAction::Quit),
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => Some(AppAction::Quit),
        KeyCode::Tab if key.modifiers.is_empty() => Some(AppAction::NextScreen),
        KeyCode::BackTab if allows_only_shift(key.modifiers) => Some(AppAction::PreviousScreen),
        KeyCode::Right | KeyCode::Down if key.modifiers.is_empty() => Some(AppAction::NextScreen),
        KeyCode::Left | KeyCode::Up if key.modifiers.is_empty() => Some(AppAction::PreviousScreen),
        KeyCode::Char('l' | 'j') if key.modifiers.is_empty() => Some(AppAction::NextScreen),
        KeyCode::Char('h' | 'k') if key.modifiers.is_empty() => Some(AppAction::PreviousScreen),
        KeyCode::Char('?') if allows_only_shift(key.modifiers) => Some(AppAction::ToggleHelp),
        KeyCode::Esc if key.modifiers.is_empty() => Some(AppAction::HideHelp),
        KeyCode::Char(digit @ '1'..='6') if key.modifiers.is_empty() => {
            Some(AppAction::GoTo(screen_for_digit(digit)))
        }
        _ => None,
    }
}

/// Accepts bare keys and keys whose only modifier is Shift, which layouts
/// require to produce characters such as `Q` or `?`.
fn allows_only_shift(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

fn screen_for_digit(digit: char) -> Screen {
    match digit {
        '1' => Screen::Dashboard,
        '2' => Screen::Performance,
        '3' => Screen::Fans,
        '4' => Screen::Battery,
        '5' => Screen::Devices,
        _ => Screen::Diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::app::{AppAction, Screen};

    use super::action_for_key;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    fn release(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Release,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn q_quits() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('q'), KeyModifiers::empty())),
            Some(AppAction::Quit)
        );
    }

    #[test]
    fn uppercase_q_quits() {
        assert_eq!(
            action_for_key(press(
                KeyCode::Char('Q'),
                KeyModifiers::empty().union(KeyModifiers::SHIFT)
            )),
            Some(AppAction::Quit)
        );
    }

    #[test]
    fn ctrl_c_quits() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(AppAction::Quit)
        );
    }

    #[test]
    fn tab_advances_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::Tab, KeyModifiers::empty())),
            Some(AppAction::NextScreen)
        );
    }

    #[test]
    fn backtab_goes_to_previous_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::BackTab, KeyModifiers::empty())),
            Some(AppAction::PreviousScreen)
        );
    }

    #[test]
    fn question_mark_toggles_help() {
        assert_eq!(
            action_for_key(press(
                KeyCode::Char('?'),
                KeyModifiers::empty().union(KeyModifiers::SHIFT)
            )),
            Some(AppAction::ToggleHelp)
        );
    }

    #[test]
    fn escape_hides_help() {
        assert_eq!(
            action_for_key(press(KeyCode::Esc, KeyModifiers::empty())),
            Some(AppAction::HideHelp)
        );
    }

    #[test]
    fn digits_jump_to_screens() {
        let cases = [
            ('1', Screen::Dashboard),
            ('2', Screen::Performance),
            ('3', Screen::Fans),
            ('4', Screen::Battery),
            ('5', Screen::Devices),
            ('6', Screen::Diagnostics),
        ];
        for (digit, screen) in cases {
            assert_eq!(
                action_for_key(press(KeyCode::Char(digit), KeyModifiers::empty())),
                Some(AppAction::GoTo(screen)),
                "digit {digit} must jump to {}",
                screen.title(),
            );
        }
    }

    #[test]
    fn unrelated_key_maps_to_none() {
        assert_eq!(
            action_for_key(press(KeyCode::Enter, KeyModifiers::empty())),
            None
        );
        assert_eq!(
            action_for_key(press(KeyCode::Char('x'), KeyModifiers::empty())),
            None
        );
        assert_eq!(
            action_for_key(press(KeyCode::Char('c'), KeyModifiers::empty())),
            None
        );
    }

    #[test]
    fn release_events_are_ignored() {
        assert_eq!(
            action_for_key(release(KeyCode::Char('q'), KeyModifiers::empty())),
            None
        );
        assert_eq!(
            action_for_key(release(KeyCode::Tab, KeyModifiers::empty())),
            None
        );
    }

    #[test]
    fn alt_q_does_not_quit() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('q'), KeyModifiers::ALT)),
            None
        );
    }

    #[test]
    fn ctrl_q_does_not_quit() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn alt_digit_does_not_jump() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('1'), KeyModifiers::ALT)),
            None
        );
    }

    #[test]
    fn right_advances_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::Right, KeyModifiers::empty())),
            Some(AppAction::NextScreen)
        );
    }

    #[test]
    fn down_advances_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::Down, KeyModifiers::empty())),
            Some(AppAction::NextScreen)
        );
    }

    #[test]
    fn left_goes_to_previous_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::Left, KeyModifiers::empty())),
            Some(AppAction::PreviousScreen)
        );
    }

    #[test]
    fn up_goes_to_previous_screen() {
        assert_eq!(
            action_for_key(press(KeyCode::Up, KeyModifiers::empty())),
            Some(AppAction::PreviousScreen)
        );
    }

    #[test]
    fn vim_next_keys_advance_screen() {
        for key in ['l', 'j'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(key), KeyModifiers::empty())),
                Some(AppAction::NextScreen),
                "vim key {key} must advance",
            );
        }
    }

    #[test]
    fn vim_previous_keys_go_back() {
        for key in ['h', 'k'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(key), KeyModifiers::empty())),
                Some(AppAction::PreviousScreen),
                "vim key {key} must go back",
            );
        }
    }

    #[test]
    fn alt_vim_keys_do_nothing() {
        for key in ['h', 'j', 'k', 'l'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(key), KeyModifiers::ALT)),
                None,
                "Alt+{key} must not navigate",
            );
        }
    }

    #[test]
    fn ctrl_vim_keys_do_nothing() {
        for key in ['h', 'j', 'k', 'l'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(key), KeyModifiers::CONTROL)),
                None,
                "Ctrl+{key} must not navigate",
            );
        }
    }

    #[test]
    fn shifted_vim_keys_do_nothing() {
        for key in ['H', 'J', 'K', 'L'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(key), KeyModifiers::empty())),
                None,
                "shifted {key} must not navigate",
            );
        }
    }

    #[test]
    fn arrow_release_events_do_nothing() {
        for code in [KeyCode::Right, KeyCode::Down, KeyCode::Left, KeyCode::Up] {
            assert_eq!(
                action_for_key(release(code, KeyModifiers::empty())),
                None,
                "{code:?} release must be ignored",
            );
        }
    }

    #[test]
    fn vim_release_events_do_nothing() {
        for key in ['h', 'j', 'k', 'l'] {
            assert_eq!(
                action_for_key(release(KeyCode::Char(key), KeyModifiers::empty())),
                None,
                "{key} release must be ignored",
            );
        }
    }

    #[test]
    fn enter_remains_unmapped() {
        assert_eq!(
            action_for_key(press(KeyCode::Enter, KeyModifiers::empty())),
            None
        );
    }
}

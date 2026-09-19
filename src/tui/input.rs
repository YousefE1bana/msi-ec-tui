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
/// Vim navigation (`h`/`j`/`k`/`l`) stays enabled; use
/// [`action_for_key_with_options`] to honor the configured `vim_keys`
/// setting.
pub fn action_for_key(key: KeyEvent) -> Option<AppAction> {
    action_for_key_with_options(key, true)
}

/// Key translation honoring the configured vim-keys setting.
///
/// With `vim_keys` false, `h`/`j`/`k`/`l` become unmapped while arrow keys,
/// Tab, digits, palette, help, and quit shortcuts keep working. The input
/// layer never reads files: callers pass the prepared boolean down from
/// the loaded [`crate::config::AppConfig`].
pub fn action_for_key_with_options(key: KeyEvent, vim_keys: bool) -> Option<AppAction> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Char('q') if key.modifiers.is_empty() => Some(AppAction::Quit),
        KeyCode::Char('Q') if allows_only_shift(key.modifiers) => Some(AppAction::Quit),
        KeyCode::Char('c') if key.modifiers == KeyModifiers::CONTROL => Some(AppAction::Quit),
        KeyCode::Tab if key.modifiers.is_empty() => Some(AppAction::NextScreen),
        KeyCode::BackTab if allows_only_shift(key.modifiers) => Some(AppAction::PreviousScreen),
        KeyCode::Right if key.modifiers.is_empty() => Some(AppAction::MoveRight),
        KeyCode::Down if key.modifiers.is_empty() => Some(AppAction::MoveDown),
        KeyCode::Left if key.modifiers.is_empty() => Some(AppAction::MoveLeft),
        KeyCode::Up if key.modifiers.is_empty() => Some(AppAction::MoveUp),
        KeyCode::Char('l') if key.modifiers.is_empty() && vim_keys => Some(AppAction::MoveRight),
        KeyCode::Char('j') if key.modifiers.is_empty() && vim_keys => Some(AppAction::MoveDown),
        KeyCode::Char('h') if key.modifiers.is_empty() && vim_keys => Some(AppAction::MoveLeft),
        KeyCode::Char('k') if key.modifiers.is_empty() && vim_keys => Some(AppAction::MoveUp),
        KeyCode::Enter if key.modifiers.is_empty() => Some(AppAction::Activate),
        KeyCode::Esc if key.modifiers.is_empty() => Some(AppAction::Cancel),
        KeyCode::Char('?') if allows_only_shift(key.modifiers) => Some(AppAction::ToggleHelp),
        KeyCode::Char('p') if key.modifiers.is_empty() => Some(AppAction::TogglePalette),
        KeyCode::Char('P') if allows_only_shift(key.modifiers) => Some(AppAction::TogglePalette),
        KeyCode::Char(digit @ '1'..='7') if key.modifiers.is_empty() => {
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
        '6' => Screen::Profiles,
        _ => Screen::Diagnostics,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    use crate::app::{AppAction, Screen};

    use super::{action_for_key, action_for_key_with_options};

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
    fn escape_cancels() {
        assert_eq!(
            action_for_key(press(KeyCode::Esc, KeyModifiers::empty())),
            Some(AppAction::Cancel)
        );
    }

    #[test]
    fn enter_activates() {
        assert_eq!(
            action_for_key(press(KeyCode::Enter, KeyModifiers::empty())),
            Some(AppAction::Activate)
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
            ('6', Screen::Profiles),
            ('7', Screen::Diagnostics),
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
    fn digit_eight_is_unmapped() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('8'), KeyModifiers::empty())),
            None
        );
    }

    #[test]
    fn palette_keys_toggle_palette() {
        // Bare p and bare/shifted P open the palette; never Profiles.
        for (code, modifiers) in [
            (KeyCode::Char('p'), KeyModifiers::empty()),
            (KeyCode::Char('P'), KeyModifiers::empty()),
            (
                KeyCode::Char('P'),
                KeyModifiers::empty().union(KeyModifiers::SHIFT),
            ),
        ] {
            assert_eq!(
                action_for_key(press(code, modifiers)),
                Some(AppAction::TogglePalette),
                "{code:?} must toggle the palette",
            );
        }
    }

    #[test]
    fn alt_ctrl_palette_keys_do_nothing() {
        for modifiers in [KeyModifiers::ALT, KeyModifiers::CONTROL] {
            for code in [KeyCode::Char('p'), KeyCode::Char('P')] {
                assert_eq!(
                    action_for_key(press(code, modifiers)),
                    None,
                    "{code:?} with {modifiers:?} must not open the palette",
                );
            }
        }
    }

    #[test]
    fn palette_release_events_do_nothing() {
        for (code, modifiers) in [
            (KeyCode::Char('p'), KeyModifiers::empty()),
            (
                KeyCode::Char('P'),
                KeyModifiers::empty().union(KeyModifiers::SHIFT),
            ),
        ] {
            assert_eq!(
                action_for_key(release(code, modifiers)),
                None,
                "{code:?} release must be ignored",
            );
        }
    }

    #[test]
    fn digit_release_events_do_nothing() {
        for digit in ['1', '6', '7'] {
            assert_eq!(
                action_for_key(release(KeyCode::Char(digit), KeyModifiers::empty())),
                None,
                "{digit} release must be ignored",
            );
        }
    }

    #[test]
    fn unrelated_key_maps_to_none() {
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
        for digit in ['1', '6', '7'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(digit), KeyModifiers::ALT)),
                None,
                "Alt+{digit} must not navigate",
            );
        }
    }

    #[test]
    fn ctrl_digit_does_not_jump() {
        for digit in ['1', '6', '7'] {
            assert_eq!(
                action_for_key(press(KeyCode::Char(digit), KeyModifiers::CONTROL)),
                None,
                "Ctrl+{digit} must not navigate",
            );
        }
    }

    #[test]
    fn right_moves_right() {
        assert_eq!(
            action_for_key(press(KeyCode::Right, KeyModifiers::empty())),
            Some(AppAction::MoveRight)
        );
    }

    #[test]
    fn down_moves_down() {
        assert_eq!(
            action_for_key(press(KeyCode::Down, KeyModifiers::empty())),
            Some(AppAction::MoveDown)
        );
    }

    #[test]
    fn left_moves_left() {
        assert_eq!(
            action_for_key(press(KeyCode::Left, KeyModifiers::empty())),
            Some(AppAction::MoveLeft)
        );
    }

    #[test]
    fn up_moves_up() {
        assert_eq!(
            action_for_key(press(KeyCode::Up, KeyModifiers::empty())),
            Some(AppAction::MoveUp)
        );
    }

    #[test]
    fn vim_horizontal_keys_move_sideways() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('l'), KeyModifiers::empty())),
            Some(AppAction::MoveRight)
        );
        assert_eq!(
            action_for_key(press(KeyCode::Char('h'), KeyModifiers::empty())),
            Some(AppAction::MoveLeft)
        );
    }

    #[test]
    fn vim_vertical_keys_move_rows() {
        assert_eq!(
            action_for_key(press(KeyCode::Char('j'), KeyModifiers::empty())),
            Some(AppAction::MoveDown)
        );
        assert_eq!(
            action_for_key(press(KeyCode::Char('k'), KeyModifiers::empty())),
            Some(AppAction::MoveUp)
        );
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
    fn enter_activates_editing() {
        assert_eq!(
            action_for_key(press(KeyCode::Enter, KeyModifiers::empty())),
            Some(AppAction::Activate)
        );
    }

    #[test]
    fn vim_keys_disabled_unmaps_hjkl() {
        for key in ['h', 'j', 'k', 'l'] {
            assert_eq!(
                action_for_key_with_options(
                    press(KeyCode::Char(key), KeyModifiers::empty()),
                    false
                ),
                None,
                "{key} must stay unmapped when vim keys are off",
            );
        }
    }

    #[test]
    fn vim_keys_disabled_keeps_arrows_tab_digits_palette() {
        use crate::app::Screen;
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Right, KeyModifiers::empty()), false),
            Some(AppAction::MoveRight)
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Up, KeyModifiers::empty()), false),
            Some(AppAction::MoveUp)
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Tab, KeyModifiers::empty()), false),
            Some(AppAction::NextScreen)
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Char('3'), KeyModifiers::empty()), false),
            Some(AppAction::GoTo(Screen::Fans))
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Char('p'), KeyModifiers::empty()), false),
            Some(AppAction::TogglePalette)
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Enter, KeyModifiers::empty()), false),
            Some(AppAction::Activate)
        );
        assert_eq!(
            action_for_key_with_options(press(KeyCode::Esc, KeyModifiers::empty()), false),
            Some(AppAction::Cancel)
        );
    }

    #[test]
    fn vim_keys_enabled_keeps_hjkl() {
        for (key, action) in [
            ('h', AppAction::MoveLeft),
            ('j', AppAction::MoveDown),
            ('k', AppAction::MoveUp),
            ('l', AppAction::MoveRight),
        ] {
            assert_eq!(
                action_for_key_with_options(press(KeyCode::Char(key), KeyModifiers::empty()), true),
                Some(action),
                "{key} must navigate when vim keys are on",
            );
        }
    }
}

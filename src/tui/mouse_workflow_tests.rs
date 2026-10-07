// Included inside app::tests to reuse the existing backend and executor mocks.
mod p3_mouse {
    use super::*;
    use crate::hardware::{BatteryThreshold, FanMode, HardwareCommand, ShiftMode};
    use crate::tui::app::step_tui_loop;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};

    type TestApp = TuiApp<LoopBackend, crate::tui::executor::FakeTuiExecutor>;
    const SIZES: [(u16, u16); 4] = [(160, 50), (120, 35), (100, 30), (80, 24)];

    fn draw(app: &mut TestApp, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width.max(1), height.max(1))).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, width, height);
                app.set_viewport(area);
                crate::tui::screens::render_screen(
                    frame,
                    area,
                    app.state(),
                    app.live(),
                    app.capabilities(),
                    app.profile_catalog(),
                    app.profile_selection(),
                    app.controls(),
                    app.palette(),
                    app.notifications(),
                    app.notifications_open(),
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    // Search rendered cells, not hit-test geometry: detects mismatched captions,
    // positions, clipping and modal layering independently of the input mapper.
    fn caption(buffer: &Buffer, text: &str) -> Rect {
        caption_in(buffer, buffer.area, text)
    }

    fn caption_in(buffer: &Buffer, area: Rect, text: &str) -> Rect {
        let letters: Vec<_> = text.chars().map(|c| c.to_string()).collect();
        for y in area.y..area.bottom() {
            for x in area.x..area.right().saturating_sub(letters.len() as u16 - 1) {
                if letters
                    .iter()
                    .enumerate()
                    .all(|(i, letter)| buffer[(x + i as u16, y)].symbol() == letter)
                {
                    return Rect::new(x, y, letters.len() as u16, 1);
                }
            }
        }
        panic!(
            "{text:?} not visible in {}x{}",
            buffer.area.width, buffer.area.height
        );
    }

    fn click(app: &mut TestApp, rect: Rect) -> Buffer {
        let area = app.viewport();
        let mut events = LoopSource::events(
            vec![mouse_click(rect.x, rect.y)],
            Rc::new(RefCell::new(Vec::new())),
        );
        step_tui_loop(app, &mut events, TIMEOUT, &mut |app| {
            draw(app, area.width, area.height);
            Ok(())
        })
        .unwrap();
        draw(app, area.width, area.height)
    }

    fn button(app: &mut TestApp, buffer: &Buffer, text: &str) -> Buffer {
        click(app, caption(buffer, text))
    }

    fn no_execution(app: &TestApp) {
        assert_eq!(app.executor().command_calls(), 0);
        assert_eq!(app.executor().profile_calls(), 0);
    }

    fn eligible(screen: Screen) -> TestApp {
        let mut app = healthy_control_app(screen);
        // Explicitly enable controls absent from the common fixture; current
        // values are real injected snapshot fields, never prototype state.
        app.capabilities.super_battery = true;
        app.capabilities.webcam_block = true;
        app
    }

    fn select_and_edit(app: &mut TestApp, row_index: usize, buffer: &Buffer) -> Buffer {
        let screen = app.state().current_screen();
        let regions = crate::tui::mouse::screen_regions(
            screen,
            crate::tui::shell::shell_split(app.viewport()).1,
            app.profile_row_count(),
        )
        .unwrap();
        let row = regions.rows[row_index];
        click(app, row);
        assert_eq!(app.controls().selected_index(screen), row_index);
        assert!(!app.controls().is_editing());
        no_execution(app);
        let control = crate::tui::editing::control_rows(screen)[row_index];
        let value = if screen == Screen::Devices {
            crate::tui::screens::devices::value_region(row, control, app.live().current_snapshot())
        } else {
            crate::tui::controls::value_region(row, control, app.live().current_snapshot())
        };
        assert!(!value.is_empty());
        assert!(!buffer[(value.x, value.y)].symbol().trim().is_empty());
        let buffer = click(app, value);
        assert!(app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        no_execution(app);
        buffer
    }

    #[test]
    fn every_editable_mouse_control_converges_with_keyboard_at_required_sizes() {
        let cases = [
            (
                Screen::Performance,
                0,
                HardwareCommand::SetShiftMode(ShiftMode::try_from("sport").unwrap()),
            ),
            (
                Screen::Performance,
                1,
                HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap()),
            ),
            (
                Screen::Performance,
                2,
                HardwareCommand::SetCoolerBoost(true),
            ),
            (
                Screen::Performance,
                3,
                HardwareCommand::SetSuperBattery(true),
            ),
            (
                Screen::Fans,
                0,
                HardwareCommand::SetFanMode(FanMode::try_from("silent").unwrap()),
            ),
            (Screen::Fans, 1, HardwareCommand::SetCoolerBoost(true)),
            (
                Screen::Battery,
                0,
                HardwareCommand::SetBatteryThreshold(
                    BatteryThreshold::from_end_percent(90).unwrap(),
                ),
            ),
            (Screen::Battery, 1, HardwareCommand::SetSuperBattery(true)),
            (Screen::Devices, 0, HardwareCommand::SetWebcam(false)),
            (Screen::Devices, 1, HardwareCommand::SetWebcamBlock(true)),
            (Screen::Devices, 2, HardwareCommand::SetKeyboardBacklight(3)),
        ];
        for (width, height) in SIZES {
            for (screen, row, expected) in &cases {
                let mut app = eligible(*screen);
                let initial_refreshes = app.live().history().len();
                let buffer = draw(&mut app, width, height);
                let buffer = select_and_edit(&mut app, *row, &buffer);
                let buffer = button(&mut app, &buffer, "[+]");
                assert_eq!(app.controls().editor().unwrap().draft(), expected);
                no_execution(&app);
                let buffer = button(&mut app, &buffer, "[Review]");
                assert_eq!(app.controls().pending_command(), Some(expected));
                assert!(app.controls().editor().is_none());
                assert_eq!(app.live().history().len(), initial_refreshes);
                no_execution(&app);
                let apply = caption(&buffer, "[Enter] Apply");
                // Wheel and right click cannot authorize a pending mutation.
                for kind in [
                    MouseEventKind::ScrollDown,
                    MouseEventKind::Down(MouseButton::Right),
                ] {
                    assert_eq!(
                        app.mouse_action(MouseEvent {
                            kind,
                            column: apply.x,
                            row: apply.y,
                            modifiers: KeyModifiers::empty()
                        }),
                        None
                    );
                }
                click(&mut app, apply);
                assert_eq!(
                    app.executor().received_commands(),
                    std::slice::from_ref(expected)
                );
                assert_eq!(app.executor().profile_calls(), 0);
                let notice = app.notice().expect("executor success produces a notice");
                assert_eq!(
                    notice.expires_at() - notice.created_at(),
                    crate::tui::confirmation::SUCCESS_TTL
                );
                assert!(app.controls().pending().is_none());
                assert_eq!(app.live().history().len(), initial_refreshes + 1);
                click(&mut app, apply);
                assert_eq!(
                    app.executor().command_calls(),
                    1,
                    "repeated Apply must not execute twice"
                );
                let mut keyboard = eligible(*screen);
                for _ in 0..*row {
                    keyboard.handle_action(AppAction::MoveDown);
                }
                for action in [
                    AppAction::Activate,
                    AppAction::MoveRight,
                    AppAction::Activate,
                ] {
                    keyboard.handle_action(action);
                }
                no_execution(&keyboard);
                keyboard.handle_action(AppAction::Activate);
                assert_eq!(
                    app.executor().received_commands(),
                    keyboard.executor().received_commands()
                );
            }
        }
    }

    #[test]
    fn mouse_editor_cancel_and_review_cancel_never_execute() {
        for (width, height) in SIZES {
            for screen in [
                Screen::Performance,
                Screen::Fans,
                Screen::Battery,
                Screen::Devices,
            ] {
                let mut app = eligible(screen);
                let buffer = draw(&mut app, width, height);
                let buffer = select_and_edit(&mut app, 0, &buffer);
                let buffer = button(&mut app, &buffer, "[+]");
                button(&mut app, &buffer, "[Cancel]");
                assert!(app.controls().editor().is_none());
                no_execution(&app);
                let buffer = draw(&mut app, width, height);
                let buffer = select_and_edit(&mut app, 0, &buffer);
                let buffer = button(&mut app, &buffer, "[Review]");
                let apply = caption(&buffer, "[Enter] Apply");
                button(&mut app, &buffer, "[Esc] Cancel");
                assert!(app.controls().pending().is_none());
                click(&mut app, apply);
                no_execution(&app);
            }
        }
    }

    #[test]
    fn profile_selection_review_apply_share_retained_keyboard_intent() {
        for (width, height) in SIZES {
            let mut app = eligible(Screen::Profiles);
            let buffer = draw(&mut app, width, height);
            let row = crate::tui::screens::profiles::hit_regions(
                crate::tui::shell::shell_split(app.viewport()).1,
                app.profile_row_count(),
            )
            .rows[0];
            click(&mut app, row);
            assert!(app.controls().pending().is_none());
            no_execution(&app);
            let buffer = button(&mut app, &buffer, "[ REVIEW CHANGES ]");
            no_execution(&app);
            let mut keyboard = eligible(Screen::Profiles);
            keyboard.handle_action(AppAction::Activate);
            assert_eq!(app.controls().pending(), keyboard.controls().pending());
            assert!(matches!(
                app.controls().pending(),
                Some(crate::tui::confirmation::PendingMutation::Profile(_))
            ));
            let apply = caption(&buffer, "[Enter] Apply");
            click(&mut app, apply);
            keyboard.handle_action(AppAction::Activate);
            assert_eq!(
                app.executor().received_profiles(),
                keyboard.executor().received_profiles()
            );
            assert_eq!(app.executor().profile_calls(), 1);
            assert_eq!(app.executor().command_calls(), 0);
            click(&mut app, apply);
            assert_eq!(app.executor().profile_calls(), 1);
        }
    }

    #[test]
    fn profile_review_cancel_and_stale_apply_execute_nothing() {
        let mut app = eligible(Screen::Profiles);
        let buffer = draw(&mut app, 80, 24);
        let buffer = button(&mut app, &buffer, "[ REVIEW CHANGES ]");
        let apply = caption(&buffer, "[Enter] Apply");
        button(&mut app, &buffer, "[Esc] Cancel");
        assert!(app.controls().pending().is_none());
        click(&mut app, apply);
        no_execution(&app);
    }

    #[test]
    fn mouse_read_only_and_unsupported_controls_cannot_stage_or_execute() {
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
            Screen::Profiles,
        ] {
            let mut app = read_only_control_app(screen);
            let buffer = draw(&mut app, 80, 24);
            let regions = crate::tui::mouse::screen_regions(
                screen,
                crate::tui::shell::shell_split(app.viewport()).1,
                app.profile_row_count(),
            )
            .unwrap();
            for row in regions.rows {
                for x in row.x..row.right() {
                    click(&mut app, Rect::new(x, row.y, 1, 1));
                }
            }
            if screen == Screen::Profiles {
                button(&mut app, &buffer, "[ REVIEW CHANGES ]");
            }
            assert!(!app.controls().is_editing());
            assert!(app.controls().pending().is_none());
            no_execution(&app);
        }
        let mut app = healthy_control_app(Screen::Devices);
        draw(&mut app, 80, 24);
        // Webcam Block is unsupported; Fn/Win are informational even if discovered.
        let regions = crate::tui::screens::devices::hit_regions(
            crate::tui::shell::shell_split(app.viewport()).1,
        );
        for i in [1, 3, 4] {
            let row = regions.rows[i];
            for x in row.x..row.right() {
                click(&mut app, Rect::new(x, row.y, 1, 1));
            }
        }
        assert!(!app.controls().is_editing());
        no_execution(&app);
    }

    #[test]
    fn battery_mouse_adjustment_keeps_existing_valid_ten_point_windows() {
        let mut mouse = eligible(Screen::Battery);
        let mut keyboard = eligible(Screen::Battery);
        let buffer = draw(&mut mouse, 80, 24);
        let mut buffer = select_and_edit(&mut mouse, 0, &buffer);
        keyboard.handle_action(AppAction::Activate);
        for (button_text, action) in [("[+]", AppAction::MoveRight), ("[-]", AppAction::MoveLeft)] {
            for _ in 0..15 {
                buffer = button(&mut mouse, &buffer, button_text);
                keyboard.handle_action(action);
                let draft = mouse.controls().editor().unwrap().draft();
                assert_eq!(draft, keyboard.controls().editor().unwrap().draft());
                draft
                    .validate(mouse.live().mode(), mouse.capabilities())
                    .unwrap();
                let HardwareCommand::SetBatteryThreshold(window) = draft else {
                    panic!("not a battery window");
                };
                assert_eq!(window.start_percent() + 10, window.end_percent());
                assert!((10..=100).contains(&window.end_percent()));
                no_execution(&mouse);
            }
        }
    }

    #[test]
    fn telemetry_cards_only_focus_and_never_create_intent() {
        for (screen, indices) in [
            (Screen::Performance, vec![1, 2, 3, 4]),
            (Screen::Fans, vec![1, 2, 3]),
            (Screen::Battery, vec![0, 2, 3, 4]),
        ] {
            let mut app = eligible(screen);
            draw(&mut app, 80, 24);
            let regions = crate::tui::mouse::screen_regions(
                screen,
                crate::tui::shell::shell_split(app.viewport()).1,
                5,
            )
            .unwrap();
            for index in indices {
                let card = crate::tui::shell::inset(regions.cards[index]);
                click(&mut app, card);
                assert_eq!(app.state().focused_card(), index);
                assert!(!app.controls().is_editing());
                assert!(app.controls().pending().is_none());
                no_execution(&app);
            }
        }
    }

    #[test]
    fn hidden_underlay_review_cannot_confirm_pending() {
        let mut app = eligible(Screen::Profiles);
        let buffer = draw(&mut app, 160, 50);
        let review = caption(&buffer, "[ REVIEW CHANGES ]");
        button(&mut app, &buffer, "[ REVIEW CHANGES ]");
        click(&mut app, review);
        no_execution(&app);
        assert!(app.controls().pending().is_some());
        app.state_mut().apply(AppAction::ShowHelp);
        let apply = crate::tui::confirmation::confirmation_buttons(
            app.viewport(),
            app.controls().pending().unwrap(),
            app.live().current_snapshot(),
            app.live().mode(),
            app.capabilities(),
        )
        .into_iter()
        .find(|(_, action)| *action == AppAction::Activate)
        .unwrap()
        .0;
        click(&mut app, apply);
        no_execution(&app);
        assert!(app.controls().pending().is_some());
    }

    #[test]
    fn resize_recomputes_visible_review_buttons_and_zero_area_is_inert() {
        let mut app = eligible(Screen::Fans);
        let buffer = draw(&mut app, 160, 50);
        let buffer = select_and_edit(&mut app, 0, &buffer);
        let buffer = button(&mut app, &buffer, "[Review]");
        let old_apply = caption(&buffer, "[Enter] Apply");
        let buffer = draw(&mut app, 80, 24);
        assert_eq!(
            app.mouse_action(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: old_apply.x,
                row: old_apply.y,
                modifiers: KeyModifiers::empty()
            }),
            None
        );
        no_execution(&app);
        draw(&mut app, 0, 0);
        assert!(app.controls().pending().is_some());
        assert_eq!(
            app.mouse_action(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty()
            }),
            None
        );
        draw(&mut app, 80, 24);
        button(&mut app, &buffer, "[Enter] Apply");
        assert_eq!(app.executor().command_calls(), 1);
        draw(&mut app, 0, 0);
        assert_eq!(
            app.mouse_action(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 0,
                row: 0,
                modifiers: KeyModifiers::empty()
            }),
            None
        );
    }

    #[test]
    fn screen_switch_drops_editor_and_old_button_authority() {
        let mut app = eligible(Screen::Performance);
        let buffer = draw(&mut app, 160, 50);
        let buffer = select_and_edit(&mut app, 0, &buffer);
        let old_review = caption(&buffer, "[Review]");
        app.handle_action(AppAction::GoTo(Screen::Settings));
        draw(&mut app, 160, 50);
        click(&mut app, old_review);
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        no_execution(&app);
    }

    #[test]
    fn help_and_notifications_close_buttons_are_non_mutating() {
        let mut app = eligible(Screen::Fans);
        app.handle_action(AppAction::ShowHelp);
        let buffer = draw(&mut app, 80, 24);
        button(&mut app, &buffer, "[Close]");
        assert!(!app.state().help_visible());
        app.notifications_open = true;
        let buffer = draw(&mut app, 80, 24);
        button(&mut app, &buffer, "[Close]");
        assert!(!app.notifications_open());
        no_execution(&app);
    }

    #[test]
    fn mouse_module_has_no_hardware_or_filesystem_execution_dependencies() {
        let source = include_str!("mouse.rs");
        for forbidden in [
            "HardwareCommand",
            "execute_command",
            "apply_profile",
            "write_boundary",
            "msi_ec_write",
            "/sys/",
            "std::fs",
            "std::process",
        ] {
            assert!(
                !source.contains(forbidden),
                "mouse mapper contains {forbidden}"
            );
        }
    }

    #[test]
    fn clipped_controls_and_buttons_never_keep_invisible_edit_targets() {
        let mut clipped_buttons = 0;
        for (width, height) in [(60, 15), (80, 24), (100, 30), (120, 35), (160, 50)] {
            for screen in [
                Screen::Performance,
                Screen::Fans,
                Screen::Battery,
                Screen::Devices,
            ] {
                let mut app = eligible(screen);
                app.handle_action(AppAction::Activate);
                let buffer = draw(&mut app, width, height);
                let count = crate::tui::editing::control_rows(screen).len();
                let regions = crate::tui::mouse::screen_regions(
                    screen,
                    crate::tui::shell::shell_split(app.viewport()).1,
                    5,
                )
                .unwrap();
                let (card, offset) = match screen {
                    Screen::Battery => (1, count),
                    Screen::Devices => (2, 0),
                    _ => (0, count),
                };
                let buttons = crate::tui::controls::editor_button_regions(
                    crate::tui::shell::inset(regions.cards[card]),
                    offset,
                    app.controls(),
                );
                clipped_buttons += 4 - buttons.len();
                for (rect, action) in buttons {
                    let label = match action {
                        AppAction::MoveLeft => "[-]",
                        AppAction::MoveRight => "[+]",
                        AppAction::Activate => "[Review]",
                        AppAction::Cancel => "[Cancel]",
                        other => panic!("unexpected editor action {other:?}"),
                    };
                    assert_eq!(caption(&buffer, label), rect);
                }
                no_execution(&app);
            }
        }
        assert!(clipped_buttons > 0, "exercise actual clipped controls");
        // Zero-height rows must not leak beyond their card into another panel.
        assert!(crate::tui::shell::row_rect(Rect::new(5, 5, 30, 1), 1).is_empty());
        assert!(crate::tui::shell::text_region(Rect::new(5, 5, 4, 1), 0, "[Review]").is_empty());
        let dashboard = crate::tui::mouse::dashboard_regions(Rect::new(0, 0, 80, 24)).unwrap();
        assert!(dashboard.menu_rows.iter().all(|rect| rect.is_empty()));
    }

    #[test]
    fn missing_telemetry_value_click_cannot_open_editor() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = loop_app(vec![Ok(HardwareSnapshot::default())], log);
        app.capabilities = crate::tui::screens::support::full_capabilities();
        app.refresh();
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
        ] {
            app.handle_action(AppAction::GoTo(screen));
            draw(&mut app, 80, 24);
            let regions = crate::tui::mouse::screen_regions(
                screen,
                crate::tui::shell::shell_split(app.viewport()).1,
                5,
            )
            .unwrap();
            for row in regions.rows {
                for x in row.x..row.right() {
                    click(&mut app, Rect::new(x, row.y, 1, 1));
                }
            }
            assert!(!app.controls().is_editing());
            assert!(app.controls().pending().is_none());
            no_execution(&app);
        }
    }

    #[test]
    fn wheel_navigates_existing_rows_and_never_adjusts_an_editor() {
        let mut app = eligible(Screen::Battery);
        let buffer = draw(&mut app, 80, 24);
        let row = crate::tui::screens::battery::hit_regions(
            crate::tui::shell::shell_split(app.viewport()).1,
        )
        .rows[0];
        let event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: row.x,
            row: row.y,
            modifiers: KeyModifiers::empty(),
        };
        for expected in [1, 0, 1, 0] {
            app.handle_action(app.mouse_action(event).unwrap());
            assert_eq!(app.controls().selected_index(Screen::Battery), expected);
            no_execution(&app);
        }
        select_and_edit(&mut app, 0, &buffer);
        let draft = app.controls().editor().unwrap().draft().clone();
        if let Some(action) = app.mouse_action(event) {
            app.handle_action(action);
        }
        assert_eq!(app.controls().editor().unwrap().draft(), &draft);
        no_execution(&app);
    }

    #[test]
    fn command_failure_uses_existing_typed_result_and_notice_deadline() {
        use crate::hardware::CommandValidationError;
        use crate::safety::CommandExecutionError;
        use crate::tui::confirmation::{FAILURE_TTL, NoticeKind};
        let error = CommandExecutionError::Validation(
            CommandValidationError::FanModeNotAdvertised(FanMode::try_from("silent").unwrap()),
        );
        let expected_message = format!("Action failed: {error}");
        let mut app = eligible(Screen::Fans);
        app.executor = crate::tui::executor::FakeTuiExecutor::with_command_error(error);
        let buffer = draw(&mut app, 80, 24);
        let buffer = select_and_edit(&mut app, 0, &buffer);
        let buffer = button(&mut app, &buffer, "[+]");
        let buffer = button(&mut app, &buffer, "[Review]");
        button(&mut app, &buffer, "[Enter] Apply");
        let notice = app.notice().unwrap();
        assert_eq!(notice.kind(), NoticeKind::Failure);
        assert_eq!(notice.message(), expected_message);
        assert_eq!(notice.expires_at() - notice.created_at(), FAILURE_TTL);
        let deadline = notice.expires_at();
        assert!(
            !app.controls
                .expire_notice_if_due(deadline - Duration::from_nanos(1))
        );
        assert!(app.controls.expire_notice_if_due(deadline));
        assert!(app.notice().is_none());
        assert_eq!(
            app.notifications().len(),
            1,
            "expiry preserves error history"
        );
        assert_eq!(app.executor().command_calls(), 1);
    }

    #[test]
    fn utility_overlay_and_palette_mouse_actions_never_reach_hardware() {
        let mut app = eligible(Screen::Fans);
        app.handle_action(AppAction::TogglePalette);
        let buffer = draw(&mut app, 80, 24);
        button(&mut app, &buffer, "Settings");
        assert_eq!(app.state().current_screen(), Screen::Settings);
        assert!(!app.palette().is_open());
        assert!(!app.controls().is_editing());
        assert!(app.controls().pending().is_none());
        no_execution(&app);
    }

    #[test]
    fn custom_profile_mouse_review_uses_retained_catalog_without_reopening() {
        let (_dir, store) = catalog_store();
        write_profile(
            &store,
            "work.toml",
            b"name = \"Mouse Work\"\n\n[performance]\nfan_mode = \"silent\"\n",
        );
        let expected = store.load(&"work".parse().unwrap()).unwrap();
        let mut app = eligible(Screen::Profiles);
        app.profile_catalog = crate::tui::ProfileCatalog::from_store(&store);
        std::fs::remove_dir_all(store.directory()).unwrap();
        let buffer = draw(&mut app, 80, 24);
        button(&mut app, &buffer, "Mouse Work");
        assert_eq!(app.profile_selection().index(), 5);
        no_execution(&app);
        let buffer = draw(&mut app, 80, 24);
        let buffer = button(&mut app, &buffer, "[ REVIEW CHANGES ]");
        no_execution(&app);
        button(&mut app, &buffer, "[Enter] Apply");
        assert_eq!(app.executor().received_profiles(), &[expected]);
        assert_eq!(app.executor().command_calls(), 0);
    }

    #[test]
    fn footer_select_supports_mouse_only_screen_navigation_at_every_required_size() {
        for (width, height) in SIZES {
            for read_only in [false, true] {
                let mut app = if read_only {
                    read_only_control_app(Screen::Dashboard)
                } else {
                    eligible(Screen::Dashboard)
                };
                for (name, screen) in [
                    ("Performance", Screen::Performance),
                    ("Battery", Screen::Battery),
                    ("Profiles", Screen::Profiles),
                    ("Settings", Screen::Settings),
                    ("Dashboard", Screen::Dashboard),
                ] {
                    let buffer = draw(&mut app, width, height);
                    // The footer is always drawn, including when Dashboard's
                    // dense menu card cannot fit its rows.
                    let select = crate::tui::mouse::footer_regions(
                        crate::tui::shell::shell_split(app.viewport()).2,
                        read_only,
                    )
                    .palette;
                    assert!(!select.is_empty());
                    assert_eq!(buffer[(select.x, select.y)].symbol(), "S");
                    let buffer = click(&mut app, select);
                    assert!(app.palette().is_open());
                    let overlay = crate::tui::palette::overlay_area(
                        app.viewport(),
                        crate::tui::palette::PaletteCommand::ALL.len(),
                    );
                    click(&mut app, caption_in(&buffer, overlay, name));
                    assert_eq!(app.state().current_screen(), screen);
                    assert!(!app.palette().is_open());
                    assert!(app.controls().pending().is_none());
                    no_execution(&app);
                }
            }
        }
    }

    #[test]
    fn footer_help_and_quit_match_rendered_columns_in_ready_and_read_only() {
        for (width, height) in SIZES {
            for read_only in [false, true] {
                let mut app = if read_only {
                    read_only_control_app(Screen::Settings)
                } else {
                    eligible(Screen::Settings)
                };
                let buffer = draw(&mut app, width, height);
                let footer = crate::tui::shell::shell_split(app.viewport()).2;
                let regions = crate::tui::mouse::footer_regions(footer, read_only);
                assert_eq!(caption_in(&buffer, footer, "Help "), regions.help);
                assert_eq!(caption_in(&buffer, footer, "Quit "), regions.quit);
                click(&mut app, regions.help);
                assert!(app.state().help_visible());
                click(&mut app, regions.help);
                assert!(!app.state().help_visible());
                click(&mut app, regions.quit);
                assert!(app.state().should_quit());
                no_execution(&app);
            }
        }
    }

    #[test]
    fn notice_expiry_cannot_reveal_an_edit_target_before_redraw() {
        use crate::tui::confirmation::{FAILURE_TTL, Notice};
        let mut app = eligible(Screen::Fans);
        app.controls.set_notice(Notice::failure_at(
            "Previous failure".to_owned(),
            std::time::Instant::now() - FAILURE_TTL - Duration::from_secs(1),
        ));
        let buffer = draw(&mut app, 80, 24);
        let row = crate::tui::screens::fans::hit_regions(
            crate::tui::shell::shell_split(app.viewport()).1,
        )
        .rows[0];
        let value = crate::tui::controls::value_region(
            row,
            crate::tui::editing::ControlId::FanMode,
            app.live().current_snapshot(),
        );
        assert!(crate::tui::mouse::contains(
            crate::tui::confirmation::notice_area(app.viewport()),
            value.x,
            value.y
        ));
        assert!(
            !crate::tui::screens::support::buffer_text(&buffer).contains("Fan Mode: current auto")
        );
        let mut events = LoopSource::events(
            vec![mouse_click(value.x, value.y)],
            Rc::new(RefCell::new(Vec::new())),
        );
        let mut draws = 0;
        step_tui_loop(&mut app, &mut events, TIMEOUT, &mut |app| {
            draws += 1;
            draw(app, 80, 24);
            Ok(())
        })
        .unwrap();
        assert!(
            !app.controls().is_editing(),
            "click was on the visible notice, not the hidden value"
        );
        assert!(app.notice().is_none());
        assert_eq!(draws, 1, "expiry redraws even an otherwise inert click");
        no_execution(&app);
    }
}

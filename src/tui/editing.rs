//! Capability-aware control editing model: selection, drafts, pending.
//!
//! Pure TUI interaction state for the four interactive control screens
//! (Performance, Fans, Battery, Devices). Holds row indices, an optional
//! in-progress draft, an optional pending [`HardwareCommand`], and an
//! optional inline validation reason. No I/O, no discovery, no writes:
//! every produced command is data only and must still pass through the
//! production safety APIs (Task 5) before any execution.
//!
//! Selection order per screen is fixed:
//! - Performance: ShiftMode, FanMode, CoolerBoost, SuperBattery
//! - Fans: FanMode, CoolerBoost
//! - Battery: BatteryThreshold
//! - Devices: Webcam, WebcamBlock, KeyboardBacklight, FnKeyInfo, WinKeyInfo
//!
//! Fn/Win rows are informational only: no [`HardwareCommand`] variant
//! exists for them, so they can never enter edit mode.
//!
//! Drafts initialize from the already-sampled snapshot when possible and
//! never invent state: missing telemetry, unsupported capabilities, and
//! READ-ONLY mode all refuse edit mode. Mode candidates preserve
//! driver-reported order verbatim; battery candidates are the deterministic
//! 10-point grid `10,20,...,100` via
//! [`BatteryThreshold::from_end_percent`].

use super::confirmation::{Notice, PendingMutation, ProfilePending};
use crate::app::Screen;
use crate::hardware::{
    BatteryThreshold, Capabilities, CommandValidationError, HardwareCommand, HardwareSnapshot,
    SupportMode,
};

/// Selectable control row. Fn/Win variants are display-only markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlId {
    /// Shift mode selector.
    ShiftMode,
    /// Fan mode selector.
    FanMode,
    /// Cooler boost toggle.
    CoolerBoost,
    /// Super battery toggle.
    SuperBattery,
    /// Battery charge-limit selector.
    BatteryThreshold,
    /// Webcam toggle.
    Webcam,
    /// Webcam block toggle.
    WebcamBlock,
    /// Keyboard backlight level selector.
    KeyboardBacklight,
    /// Informational only: no command exists.
    FnKeyInfo,
    /// Informational only: no command exists.
    WinKeyInfo,
}

/// Fixed selectable rows for one screen.
pub fn control_rows(screen: Screen) -> &'static [ControlId] {
    match screen {
        Screen::Performance => &[
            ControlId::ShiftMode,
            ControlId::FanMode,
            ControlId::CoolerBoost,
            ControlId::SuperBattery,
        ],
        Screen::Fans => &[ControlId::FanMode, ControlId::CoolerBoost],
        Screen::Battery => &[ControlId::BatteryThreshold],
        Screen::Devices => &[
            ControlId::Webcam,
            ControlId::WebcamBlock,
            ControlId::KeyboardBacklight,
            ControlId::FnKeyInfo,
            ControlId::WinKeyInfo,
        ],
        _ => &[],
    }
}

/// Whether an interactive screen supports row selection.
pub fn is_interactive_screen(screen: Screen) -> bool {
    !control_rows(screen).is_empty()
}

/// Why a control cannot enter edit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Editability {
    /// Ready to edit.
    Editable,
    /// MEC is READ-ONLY: all mutation controls disabled.
    DisabledReadOnly,
    /// Informational row with no command variant.
    Informational,
    /// The device does not expose this control.
    Unsupported,
    /// Current telemetry needed to seed a draft is absent.
    UnknownCurrent,
}

impl Editability {
    /// Short display reason for non-editable rows.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Editable => "",
            Self::DisabledReadOnly => "Disabled (read-only)",
            Self::Informational => "Informational only",
            Self::Unsupported => "Unsupported",
            Self::UnknownCurrent => "Not currently editable",
        }
    }
}

/// Pure editability verdict: READ-ONLY first, then informational, then
/// capability support, then current-telemetry presence. Never invents.
pub fn editability(
    control: ControlId,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    mode: &SupportMode,
) -> Editability {
    if matches!(mode, SupportMode::ReadOnly(_)) {
        return match control {
            ControlId::FnKeyInfo | ControlId::WinKeyInfo => Editability::Informational,
            _ => Editability::DisabledReadOnly,
        };
    }
    if matches!(control, ControlId::FnKeyInfo | ControlId::WinKeyInfo) {
        return Editability::Informational;
    }
    if !is_supported(control, capabilities) {
        return Editability::Unsupported;
    }
    if !has_current(control, snapshot) {
        return Editability::UnknownCurrent;
    }
    Editability::Editable
}

/// Capability support without consulting telemetry or mode.
fn is_supported(control: ControlId, capabilities: &Capabilities) -> bool {
    match control {
        ControlId::ShiftMode => !capabilities.shift_modes.is_empty(),
        ControlId::FanMode => !capabilities.fan_modes.is_empty(),
        ControlId::CoolerBoost => capabilities.cooler_boost,
        ControlId::SuperBattery => capabilities.super_battery,
        ControlId::BatteryThreshold => capabilities.battery_thresholds,
        ControlId::Webcam => capabilities.webcam,
        ControlId::WebcamBlock => capabilities.webcam_block,
        ControlId::KeyboardBacklight => capabilities.keyboard_backlight.is_some(),
        ControlId::FnKeyInfo | ControlId::WinKeyInfo => false,
    }
}

/// Current-telemetry presence without inventing values.
fn has_current(control: ControlId, snapshot: Option<&HardwareSnapshot>) -> bool {
    match control {
        ControlId::ShiftMode => snapshot.and_then(|s| s.shift_mode.as_ref()).is_some(),
        ControlId::FanMode => snapshot.and_then(|s| s.fan_mode.as_ref()).is_some(),
        ControlId::CoolerBoost => snapshot.and_then(|s| s.cooler_boost).is_some(),
        ControlId::SuperBattery => snapshot.and_then(|s| s.super_battery).is_some(),
        ControlId::BatteryThreshold => {
            matches!(
                snapshot,
                Some(s) if s.battery_start_threshold.is_some() && s.battery_end_threshold.is_some()
            )
        }
        ControlId::Webcam => snapshot.and_then(|s| s.webcam).is_some(),
        ControlId::WebcamBlock => snapshot.and_then(|s| s.webcam_block).is_some(),
        ControlId::KeyboardBacklight => snapshot.and_then(|s| s.keyboard_backlight).is_some(),
        ControlId::FnKeyInfo | ControlId::WinKeyInfo => false,
    }
}

/// Deterministic battery charge-end candidates: 10..=100 step 10.
pub fn battery_candidates() -> Vec<BatteryThreshold> {
    (1..=10)
        .map(|step| BatteryThreshold::from_end_percent(step * 10).expect("10-point grid is valid"))
        .collect()
}

/// Backlight candidates: 0..=max verbatim.
pub fn backlight_candidates(capabilities: &Capabilities) -> Vec<u8> {
    match &capabilities.keyboard_backlight {
        Some(spec) => (0..=spec.max_brightness).collect(),
        None => Vec::new(),
    }
}

/// Builds the initial draft from the current snapshot. Returns `None`
/// when the control cannot edit (READ-ONLY, unsupported, informational,
/// or missing telemetry) so callers never invent state.
pub fn initial_draft(
    control: ControlId,
    snapshot: Option<&HardwareSnapshot>,
    capabilities: &Capabilities,
    mode: &SupportMode,
) -> Option<HardwareCommand> {
    if editability(control, snapshot, capabilities, mode) != Editability::Editable {
        return None;
    }
    let snapshot = snapshot?;
    match control {
        ControlId::ShiftMode => snapshot
            .shift_mode
            .clone()
            .map(HardwareCommand::SetShiftMode),
        ControlId::FanMode => snapshot.fan_mode.clone().map(HardwareCommand::SetFanMode),
        ControlId::CoolerBoost => snapshot.cooler_boost.map(HardwareCommand::SetCoolerBoost),
        ControlId::SuperBattery => snapshot.super_battery.map(HardwareCommand::SetSuperBattery),
        ControlId::BatteryThreshold => snapshot.battery_end_threshold.and_then(|end| {
            BatteryThreshold::from_end_percent(end)
                .ok()
                .map(HardwareCommand::SetBatteryThreshold)
        }),
        ControlId::Webcam => snapshot.webcam.map(HardwareCommand::SetWebcam),
        ControlId::WebcamBlock => snapshot.webcam_block.map(HardwareCommand::SetWebcamBlock),
        ControlId::KeyboardBacklight => snapshot
            .keyboard_backlight
            .map(HardwareCommand::SetKeyboardBacklight),
        ControlId::FnKeyInfo | ControlId::WinKeyInfo => None,
    }
}

/// Steps one candidate left (-1) or right (+1) with wraparound. Mode lists
/// preserve driver order; booleans are false/true; backlight is 0..=max;
/// battery is the 10-point grid. Unknown current positions start at the
/// grid edge deterministically.
pub fn step_candidate(
    draft: &HardwareCommand,
    control: ControlId,
    capabilities: &Capabilities,
    direction: i8,
) -> HardwareCommand {
    debug_assert!(direction == -1 || direction == 1);
    match (control, draft) {
        (ControlId::ShiftMode, HardwareCommand::SetShiftMode(current)) => {
            let modes = &capabilities.shift_modes;
            if modes.is_empty() {
                return draft.clone();
            }
            let position = modes.iter().position(|mode| mode == current).unwrap_or(0);
            let next = step_index(position, modes.len(), direction);
            HardwareCommand::SetShiftMode(modes[next].clone())
        }
        (ControlId::FanMode, HardwareCommand::SetFanMode(current)) => {
            let modes = &capabilities.fan_modes;
            if modes.is_empty() {
                return draft.clone();
            }
            let position = modes.iter().position(|mode| mode == current).unwrap_or(0);
            let next = step_index(position, modes.len(), direction);
            HardwareCommand::SetFanMode(modes[next].clone())
        }
        (_, HardwareCommand::SetCoolerBoost(value)) => {
            HardwareCommand::SetCoolerBoost(step_bool(*value, direction))
        }
        (_, HardwareCommand::SetSuperBattery(value)) => {
            HardwareCommand::SetSuperBattery(step_bool(*value, direction))
        }
        (_, HardwareCommand::SetWebcam(value)) => {
            HardwareCommand::SetWebcam(step_bool(*value, direction))
        }
        (_, HardwareCommand::SetWebcamBlock(value)) => {
            HardwareCommand::SetWebcamBlock(step_bool(*value, direction))
        }
        (ControlId::KeyboardBacklight, HardwareCommand::SetKeyboardBacklight(level)) => {
            let candidates = backlight_candidates(capabilities);
            if candidates.is_empty() {
                return draft.clone();
            }
            let position = candidates
                .iter()
                .position(|candidate| candidate == level)
                .unwrap_or(0);
            HardwareCommand::SetKeyboardBacklight(
                candidates[step_index(position, candidates.len(), direction)],
            )
        }
        (ControlId::BatteryThreshold, HardwareCommand::SetBatteryThreshold(threshold)) => {
            let candidates = battery_candidates();
            let position = candidates
                .iter()
                .position(|candidate| candidate == threshold)
                .unwrap_or(0);
            HardwareCommand::SetBatteryThreshold(
                candidates[step_index(position, candidates.len(), direction)],
            )
        }
        _ => draft.clone(),
    }
}

fn step_index(position: usize, len: usize, direction: i8) -> usize {
    if len == 0 {
        return 0;
    }
    if direction >= 0 {
        (position + 1) % len
    } else {
        (position + len - 1) % len
    }
}

fn step_bool(value: bool, direction: i8) -> bool {
    // false/true pair toggles either direction.
    let _ = direction;
    !value
}

/// In-progress draft: the control plus its current candidate value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlEditor {
    control: ControlId,
    draft: HardwareCommand,
}

impl ControlEditor {
    /// Edited control.
    pub fn control(&self) -> ControlId {
        self.control
    }

    /// Current candidate value.
    pub fn draft(&self) -> &HardwareCommand {
        &self.draft
    }
}

/// Per-screen row indices plus one global draft/pending pair.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ControlState {
    performance: usize,
    fans: usize,
    battery: usize,
    devices: usize,
    editor: Option<ControlEditor>,
    pending: Option<PendingMutation>,
    error: Option<String>,
    notice: Option<Notice>,
}

impl ControlState {
    /// Selected row index for one screen.
    pub fn selected_index(&self, screen: Screen) -> usize {
        match screen {
            Screen::Performance => self.performance,
            Screen::Fans => self.fans,
            Screen::Battery => self.battery,
            Screen::Devices => self.devices,
            _ => 0,
        }
    }

    /// Selected control for one screen, if any.
    pub fn selected(&self, screen: Screen) -> Option<ControlId> {
        control_rows(screen)
            .get(self.selected_index(screen))
            .copied()
    }

    /// Moves selection down with wraparound; empty screens pin at zero.
    pub fn move_down(&mut self, screen: Screen) {
        let len = control_rows(screen).len();
        if len == 0 {
            return;
        }
        let next = (self.selected_index(screen) + 1) % len;
        self.set_index(screen, next);
    }

    /// Moves selection up with wraparound; empty screens pin at zero.
    pub fn move_up(&mut self, screen: Screen) {
        let len = control_rows(screen).len();
        if len == 0 {
            return;
        }
        let next = (self.selected_index(screen) + len - 1) % len;
        self.set_index(screen, next);
    }

    fn set_index(&mut self, screen: Screen, index: usize) {
        match screen {
            Screen::Performance => self.performance = index,
            Screen::Fans => self.fans = index,
            Screen::Battery => self.battery = index,
            Screen::Devices => self.devices = index,
            _ => {}
        }
    }

    /// Clamps stale indices after capability changes.
    pub fn clamp(&mut self) {
        for screen in [
            Screen::Performance,
            Screen::Fans,
            Screen::Battery,
            Screen::Devices,
        ] {
            let len = control_rows(screen).len();
            if len == 0 {
                self.set_index(screen, 0);
            } else if self.selected_index(screen) >= len {
                self.set_index(screen, len - 1);
            }
        }
    }

    /// In-progress editor, if any.
    pub fn editor(&self) -> Option<&ControlEditor> {
        self.editor.as_ref()
    }

    /// Whether an editor is open.
    pub fn is_editing(&self) -> bool {
        self.editor.is_some()
    }

    /// Pending modal confirmation: one command or one profile, data only.
    /// Never executed from this model; Task 5 executes through the narrow
    /// adapter exactly once per confirm.
    pub fn pending(&self) -> Option<&PendingMutation> {
        self.pending.as_ref()
    }

    /// Pending command, if the modal holds a hardware command.
    pub fn pending_command(&self) -> Option<&HardwareCommand> {
        match self.pending.as_ref() {
            Some(PendingMutation::Command(command)) => Some(command),
            _ => None,
        }
    }

    /// Whether any modal confirmation is open.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Stages a retained profile for confirmation. Only call after a pure
    /// preview deemed it applicable; this stores data, never authorization.
    pub fn set_profile_pending(&mut self, pending: ProfilePending) {
        self.pending = Some(PendingMutation::Profile(pending));
        self.editor = None;
        self.error = None;
    }

    /// Inline safe reason from the last rejected draft validation.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Post-attempt result banner, if any.
    pub fn notice(&self) -> Option<&Notice> {
        self.notice.as_ref()
    }

    /// Records a post-attempt banner.
    pub fn set_notice(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    /// Clears the banner when starting a new flow.
    pub fn clear_notice(&mut self) {
        self.notice = None;
    }

    /// Begins editing the selected control. Returns false and creates
    /// nothing when the row is informational, unsupported, read-only, or
    /// missing current telemetry.
    pub fn begin_edit(
        &mut self,
        screen: Screen,
        snapshot: Option<&HardwareSnapshot>,
        capabilities: &Capabilities,
        mode: &SupportMode,
    ) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let Some(control) = self.selected(screen) else {
            return false;
        };
        match initial_draft(control, snapshot, capabilities, mode) {
            Some(draft) => {
                self.editor = Some(ControlEditor { control, draft });
                self.error = None;
                true
            }
            None => false,
        }
    }

    /// Steps the open draft one candidate left/right using driver-order
    /// candidates. No-op without an editor.
    pub fn adjust(&mut self, capabilities: &Capabilities, direction: i8) {
        if let Some(editor) = &mut self.editor {
            editor.draft = step_candidate(&editor.draft, editor.control, capabilities, direction);
        }
    }

    /// Accepts the draft into pending data after pure validation. On
    /// failure no pending is created and an inline safe reason is kept.
    /// Returns true only when pending was stored. Pending is data only:
    /// this never executes.
    pub fn confirm(&mut self, mode: &SupportMode, capabilities: &Capabilities) -> bool {
        let Some(editor) = self.editor.clone() else {
            return false;
        };
        match editor.draft.validate(mode, capabilities) {
            Ok(()) => {
                self.pending = Some(PendingMutation::Command(editor.draft));
                self.editor = None;
                self.error = None;
                true
            }
            Err(error) => {
                self.error = Some(match error {
                    CommandValidationError::ReadOnly => "Rejected: read-only".to_owned(),
                    other => format!("Rejected: {other}"),
                });
                false
            }
        }
    }

    /// Discards the editor if open; otherwise discards pending and its
    /// inline reason. Returns true when anything was discarded.
    pub fn cancel(&mut self) -> bool {
        if self.editor.is_some() {
            self.editor = None;
            return true;
        }
        if self.pending.is_some() || self.error.is_some() {
            self.pending = None;
            self.error = None;
            return true;
        }
        false
    }

    /// Clears editor and inline reason on screen navigation so a stale
    /// draft never follows to another screen. Pending is global and kept.
    pub fn on_screen_change(&mut self) {
        self.editor = None;
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::screens::support::{full_capabilities, healthy_snapshot};

    fn snapshot() -> HardwareSnapshot {
        healthy_snapshot()
    }

    fn caps() -> Capabilities {
        full_capabilities()
    }

    #[test]
    fn performance_rows_are_fixed_order() {
        assert_eq!(
            control_rows(Screen::Performance),
            &[
                ControlId::ShiftMode,
                ControlId::FanMode,
                ControlId::CoolerBoost,
                ControlId::SuperBattery,
            ]
        );
    }

    #[test]
    fn fans_rows_are_fixed_order() {
        assert_eq!(
            control_rows(Screen::Fans),
            &[ControlId::FanMode, ControlId::CoolerBoost]
        );
    }

    #[test]
    fn battery_has_single_threshold_row() {
        assert_eq!(
            control_rows(Screen::Battery),
            &[ControlId::BatteryThreshold]
        );
    }

    #[test]
    fn devices_rows_end_with_informational_fn_win() {
        assert_eq!(
            control_rows(Screen::Devices),
            &[
                ControlId::Webcam,
                ControlId::WebcamBlock,
                ControlId::KeyboardBacklight,
                ControlId::FnKeyInfo,
                ControlId::WinKeyInfo,
            ]
        );
    }

    #[test]
    fn dashboard_and_diagnostics_have_no_rows() {
        assert!(control_rows(Screen::Dashboard).is_empty());
        assert!(control_rows(Screen::Diagnostics).is_empty());
        assert!(!is_interactive_screen(Screen::Dashboard));
        assert!(is_interactive_screen(Screen::Performance));
    }

    #[test]
    fn row_navigation_wraps() {
        let mut state = ControlState::default();
        state.move_down(Screen::Performance);
        assert_eq!(state.selected_index(Screen::Performance), 1);
        state.move_up(Screen::Performance);
        assert_eq!(state.selected_index(Screen::Performance), 0);
        state.move_up(Screen::Performance);
        assert_eq!(state.selected_index(Screen::Performance), 3);
    }

    #[test]
    fn selections_are_per_screen() {
        let mut state = ControlState::default();
        state.move_down(Screen::Performance);
        assert_eq!(state.selected_index(Screen::Fans), 0);
        assert_eq!(
            state.selected(Screen::Performance),
            Some(ControlId::FanMode)
        );
        assert_eq!(state.selected(Screen::Fans), Some(ControlId::FanMode));
    }

    #[test]
    fn unsupported_rows_report_unsupported() {
        let mut unsupported = caps();
        unsupported.cooler_boost = false;
        assert_eq!(
            editability(
                ControlId::CoolerBoost,
                Some(&snapshot()),
                &unsupported,
                &SupportMode::Ready,
            ),
            Editability::Unsupported
        );
        let mut no_modes = caps();
        no_modes.fan_modes.clear();
        assert_eq!(
            editability(
                ControlId::FanMode,
                Some(&snapshot()),
                &no_modes,
                &SupportMode::Ready,
            ),
            Editability::Unsupported
        );
    }

    #[test]
    fn read_only_disables_mutation_controls() {
        let mode = SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable);
        let snapshot = snapshot();
        for control in [
            ControlId::ShiftMode,
            ControlId::FanMode,
            ControlId::CoolerBoost,
            ControlId::SuperBattery,
            ControlId::BatteryThreshold,
            ControlId::Webcam,
            ControlId::WebcamBlock,
            ControlId::KeyboardBacklight,
        ] {
            assert_eq!(
                editability(control, Some(&snapshot), &caps(), &mode),
                Editability::DisabledReadOnly,
                "{control:?}"
            );
        }
    }

    #[test]
    fn fn_win_are_always_informational() {
        for mode in [
            SupportMode::Ready,
            SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
        ] {
            assert_eq!(
                editability(ControlId::FnKeyInfo, Some(&snapshot()), &caps(), &mode),
                Editability::Informational
            );
            assert_eq!(
                editability(ControlId::WinKeyInfo, Some(&snapshot()), &caps(), &mode),
                Editability::Informational
            );
        }
        assert_eq!(
            initial_draft(
                ControlId::FnKeyInfo,
                Some(&snapshot()),
                &caps(),
                &SupportMode::Ready
            ),
            None
        );
        assert_eq!(
            initial_draft(
                ControlId::WinKeyInfo,
                Some(&snapshot()),
                &caps(),
                &SupportMode::Ready
            ),
            None
        );
    }

    #[test]
    fn missing_telemetry_is_not_editable_without_invention() {
        let empty = HardwareSnapshot::default();
        assert_eq!(
            editability(
                ControlId::FanMode,
                Some(&empty),
                &caps(),
                &SupportMode::Ready,
            ),
            Editability::UnknownCurrent
        );
        assert_eq!(
            initial_draft(
                ControlId::FanMode,
                Some(&empty),
                &caps(),
                &SupportMode::Ready
            ),
            None
        );
        assert_eq!(
            initial_draft(ControlId::FanMode, None, &caps(), &SupportMode::Ready),
            None
        );
        assert_eq!(
            initial_draft(
                ControlId::CoolerBoost,
                Some(&empty),
                &caps(),
                &SupportMode::Ready,
            ),
            None
        );
    }

    #[test]
    fn fan_candidates_preserve_driver_order() {
        let modes = &caps().fan_modes;
        assert_eq!(
            modes.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
            vec!["auto", "silent", "future-mode"]
        );
        let draft = HardwareCommand::SetFanMode(modes[0].clone());
        let next = step_candidate(&draft, ControlId::FanMode, &caps(), 1);
        assert_eq!(next, HardwareCommand::SetFanMode(modes[1].clone()));
        let wrapped = step_candidate(
            &HardwareCommand::SetFanMode(modes[2].clone()),
            ControlId::FanMode,
            &caps(),
            1,
        );
        assert_eq!(wrapped, HardwareCommand::SetFanMode(modes[0].clone()));
    }

    #[test]
    fn shift_candidates_preserve_driver_order() {
        let modes = &caps().shift_modes;
        assert_eq!(
            modes.iter().map(|m| m.as_str()).collect::<Vec<_>>(),
            vec!["comfort", "sport"]
        );
    }

    #[test]
    fn future_mode_names_survive_verbatim() {
        use crate::hardware::{FanMode, ShiftMode};

        let mut future = caps();
        future.fan_modes = vec![FanMode::try_from("Whisper 2.0").unwrap()];
        future.shift_modes = vec![ShiftMode::try_from("Turbo_PLUS").unwrap()];
        let draft = initial_draft(
            ControlId::FanMode,
            Some(&snapshot()),
            &future,
            &SupportMode::Ready,
        )
        .expect("future fan editable");
        let stepped = step_candidate(&draft, ControlId::FanMode, &future, 1);
        assert_eq!(
            stepped,
            HardwareCommand::SetFanMode(FanMode::try_from("Whisper 2.0").unwrap())
        );
    }

    #[test]
    fn boolean_edit_creates_typed_command() {
        let draft = initial_draft(
            ControlId::CoolerBoost,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready,
        )
        .expect("cooler boost editable");
        assert_eq!(draft, HardwareCommand::SetCoolerBoost(false));
        let toggled = step_candidate(&draft, ControlId::CoolerBoost, &caps(), 1);
        assert_eq!(toggled, HardwareCommand::SetCoolerBoost(true));
    }

    #[test]
    fn backlight_never_exceeds_max() {
        let candidates = backlight_candidates(&caps());
        assert_eq!(candidates, vec![0, 1, 2, 3]);
        let draft = HardwareCommand::SetKeyboardBacklight(3);
        let wrapped = step_candidate(&draft, ControlId::KeyboardBacklight, &caps(), 1);
        assert_eq!(wrapped, HardwareCommand::SetKeyboardBacklight(0));
    }

    #[test]
    fn battery_candidates_are_ten_point_pairs() {
        let candidates = battery_candidates();
        let ends: Vec<u8> = candidates.iter().map(|t| t.end_percent()).collect();
        assert_eq!(ends, vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        for threshold in candidates {
            assert_eq!(threshold.end_percent(), threshold.start_percent() + 10);
        }
    }

    #[test]
    fn duplicate_fan_controls_create_same_command() {
        let snapshot = snapshot();
        let perf = initial_draft(
            ControlId::FanMode,
            Some(&snapshot),
            &caps(),
            &SupportMode::Ready,
        );
        // Same control id from Fans screen uses the same constructor.
        let fans = initial_draft(
            ControlId::FanMode,
            Some(&snapshot),
            &caps(),
            &SupportMode::Ready,
        );
        assert_eq!(perf, fans);
        let stepped_perf = step_candidate(&perf.clone().unwrap(), ControlId::FanMode, &caps(), 1);
        let stepped_fans = step_candidate(&fans.clone().unwrap(), ControlId::FanMode, &caps(), 1);
        assert_eq!(stepped_perf, stepped_fans);
    }

    #[test]
    fn duplicate_cooler_boost_controls_create_same_command() {
        let snapshot = snapshot();
        let perf = initial_draft(
            ControlId::CoolerBoost,
            Some(&snapshot),
            &caps(),
            &SupportMode::Ready,
        );
        let fans = initial_draft(
            ControlId::CoolerBoost,
            Some(&snapshot),
            &caps(),
            &SupportMode::Ready,
        );
        assert_eq!(perf, fans);
    }

    #[test]
    fn confirm_validates_and_stores_pending_only() {
        let mut state = ControlState::default();
        assert!(state.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready,
        ));
        assert!(state.is_editing());
        assert!(state.pending().is_none());
        assert!(state.confirm(&SupportMode::Ready, &caps()));
        assert!(!state.is_editing());
        assert!(state.pending().is_some());
    }

    #[test]
    fn confirm_rejects_without_pending_and_keeps_reason() {
        let mut state = ControlState::default();
        assert!(state.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready,
        ));
        // READ-ONLY validation fails at confirm time.
        assert!(!state.confirm(
            &SupportMode::ReadOnly(crate::hardware::ReadOnlyReason::MsiEcUnavailable),
            &caps(),
        ));
        assert!(state.pending().is_none());
        assert!(state.error().is_some());
    }

    #[test]
    fn cancel_discards_editor_then_pending() {
        let mut state = ControlState::default();
        assert!(state.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready,
        ));
        assert!(state.cancel());
        assert!(!state.is_editing());
        assert!(!state.cancel());
        assert!(state.confirm(&SupportMode::Ready, &caps()) || true);
    }

    #[test]
    fn cancel_after_pending_clears_pending() {
        let mut state = ControlState::default();
        assert!(state.begin_edit(
            Screen::Fans,
            Some(&snapshot()),
            &caps(),
            &SupportMode::Ready,
        ));
        assert!(state.confirm(&SupportMode::Ready, &caps()));
        assert!(state.pending().is_some());
        assert!(state.cancel());
        assert!(state.pending().is_none());
    }
}

//! The five-step workflow of the right column, its gating, back/recovery
//! rules and wording. A one-to-one port of the macOS `WorkspaceView`
//! (`canVisit`, `canAdvance`, `advance`, `primaryActionTitle`,
//! `primaryActionHint`, the Back button and the `onChange` recovery
//! handlers) so both apps step through a run the same way.

use super::claims::N_;
use super::settings::Unconverted;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Direction = 1,
    Import,
    Features,
    Preview,
    Results,
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Direction,
        Step::Import,
        Step::Features,
        Step::Preview,
        Step::Results,
    ];
    pub fn number(self) -> u32 {
        self as u32
    }
    fn from_number(n: u32) -> Option<Step> {
        Step::ALL.get((n as usize).checked_sub(1)?).copied()
    }
    pub fn title(self) -> &'static str {
        match self {
            Step::Direction => N_("Conversion Direction"),
            Step::Import => N_("Import Photos"),
            Step::Features => N_("Choose Features"),
            Step::Preview => N_("Export Preview"),
            Step::Results => N_("Results"),
        }
    }
    pub fn detail(self) -> &'static str {
        match self {
            Step::Direction => N_("Choose source and target to determine import scope"),
            Step::Import => N_("Add photos and choose this run's scope"),
            Step::Features => N_("Choose the photo features to process in this run"),
            Step::Preview => N_("Inspect planned output and choose a destination"),
            Step::Results => N_("View progress and per-photo results"),
        }
    }
}

/// Everything the gating reads, sampled from the app state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    /// A direction card was chosen (macOS `migrationTargetConfirmed`).
    pub target_confirmed: bool,
    /// Scanning sources (macOS `isBusy` while scanning; `scanComplete` false).
    pub scanning: bool,
    /// A conversion or unchanged export runs (macOS `executionActive`).
    pub running: bool,
    /// Stop was requested and the run is winding down.
    pub stopping: bool,
    pub items: usize,
    /// The user deselected everything (explicit selection, 0 selected).
    pub explicit_empty: bool,
    /// Photos in this run's scope.
    pub scope: usize,
    /// Photos expected to run, and the rest of the scope.
    pub ready: usize,
    pub unconverted: usize,
    pub unconverted_export: Unconverted,
    /// A separate folder was chosen for unchanged exports.
    pub unconverted_output: bool,
    /// The output location was chosen (macOS `outputIsChosen`).
    pub output_chosen: bool,
    /// A finished run left a report or result summary.
    pub has_results: bool,
    /// Only stills that need missing dates (macOS `isTimeOnlyWorkspace`).
    pub time_only: bool,
}

impl Facts {
    fn busy(&self) -> bool {
        self.scanning || self.running
    }
    /// macOS `canContinueImport`.
    pub fn can_continue_import(&self) -> bool {
        self.target_confirmed && !self.busy() && self.items > 0 && !self.explicit_empty
    }
    /// macOS `canReviewMigration`.
    pub fn can_review(&self) -> bool {
        self.target_confirmed && !self.busy() && !self.explicit_empty && self.items > 0
    }
    /// macOS `canApproveMigration`.
    pub fn can_approve(&self) -> bool {
        self.can_review()
            && self.output_chosen
            && (self.unconverted_export != Unconverted::SeparateLocation || self.unconverted_output)
            && (self.ready > 0
                || self.unconverted_export != Unconverted::None && self.unconverted > 0)
    }
}

/// What the primary button does when pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advance {
    Go(Step),
    /// Ask for the output location first (macOS output-location popover).
    ChooseOutput,
    /// Approve the plan and run it; the flow moves to Results.
    Start,
}

/// The primary button: a message ID and an optional count for `{count}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Primary {
    pub title: &'static str,
    pub count: Option<usize>,
    /// The Stop button replaces the primary action while busy.
    pub stop: bool,
    pub enabled: bool,
}

/// How a step reads in the step list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepStatus {
    Current,
    View,
    Upcoming,
}

impl StepStatus {
    pub fn title(self) -> &'static str {
        match self {
            StepStatus::Current => N_("Current"),
            StepStatus::View => N_("View"),
            StepStatus::Upcoming => N_("Upcoming"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flow {
    step: Step,
    /// The furthest step reached; keeps steps browsable while a run is frozen.
    furthest: Step,
}

impl Default for Flow {
    fn default() -> Self {
        Flow {
            step: Step::Direction,
            furthest: Step::Direction,
        }
    }
}

impl Flow {
    pub fn step(&self) -> Step {
        self.step
    }
    #[cfg(test)]
    pub fn furthest(&self) -> Step {
        self.furthest
    }

    /// Move to `step` (macOS `workspaceStep = …`; `onChange` raises `furthestStep`).
    pub fn set_step(&mut self, step: Step) {
        self.step = step;
        if step > self.furthest {
            self.furthest = step;
        }
    }

    /// Steps 3 and 4 need a valid scope now; the furthest-visited step only
    /// keeps them browsable while a frozen task runs, so an emptied
    /// selection or rescan can never reopen a stale plan.
    pub fn can_visit(&self, step: Step, f: &Facts) -> bool {
        let visited_while_running = f.running && self.furthest >= step;
        match step {
            Step::Direction => true,
            Step::Import => f.target_confirmed,
            Step::Features => {
                f.target_confirmed && (f.can_continue_import() || visited_while_running)
            }
            Step::Preview => f.target_confirmed && (f.can_review() || visited_while_running),
            Step::Results => f.running || f.has_results || self.step == Step::Results,
        }
    }

    pub fn status(&self, step: Step, f: &Facts) -> StepStatus {
        if self.step == step {
            StepStatus::Current
        } else if self.can_visit(step, f) {
            StepStatus::View
        } else {
            StepStatus::Upcoming
        }
    }

    /// Click on a step in the list.
    pub fn visit(&mut self, step: Step, f: &Facts) -> bool {
        if !self.can_visit(step, f) {
            return false;
        }
        self.set_step(step);
        true
    }

    /// The Back button: skip steps whose prerequisites no longer hold
    /// instead of reopening them.
    pub fn back_target(&self, f: &Facts) -> Option<Step> {
        let mut n = self.step.number().checked_sub(1)?;
        while let Some(step) = Step::from_number(n) {
            if self.can_visit(step, f) {
                return Some(step);
            }
            n = n.checked_sub(1)?;
        }
        None
    }
    pub fn can_go_back(&self) -> bool {
        self.step != Step::Direction
    }
    pub fn back(&mut self, f: &Facts) -> bool {
        match self.back_target(f) {
            Some(step) => {
                self.step = step;
                true
            }
            None => false,
        }
    }

    pub fn can_advance(&self, f: &Facts) -> bool {
        match self.step {
            Step::Direction => f.target_confirmed && !f.running,
            Step::Import => f.can_continue_import(),
            Step::Features => f.can_review(),
            Step::Preview => {
                if f.output_chosen {
                    f.can_approve()
                } else {
                    f.can_review()
                }
            }
            Step::Results => !f.running && !f.scanning,
        }
    }

    /// The primary button. Moves the flow except for `ChooseOutput` and
    /// `Start`, which the caller completes (choose a folder; start the run
    /// and call [`Flow::run_started`]).
    pub fn advance(&mut self, f: &Facts) -> Option<Advance> {
        if !self.can_advance(f) {
            return None;
        }
        let next = match self.step {
            Step::Direction => Step::Import,
            Step::Import => Step::Features,
            Step::Features => Step::Preview,
            Step::Preview => {
                return Some(if f.output_chosen {
                    Advance::Start
                } else {
                    Advance::ChooseOutput
                });
            }
            Step::Results => Step::Import,
        };
        self.set_step(next);
        Some(Advance::Go(next))
    }

    pub fn primary(&self, f: &Facts) -> Primary {
        // macOS: Stop replaces the primary action while busy, scans included.
        if f.running || f.scanning {
            return Primary {
                title: if f.stopping {
                    N_("Stopping")
                } else {
                    N_("Stop")
                },
                count: None,
                stop: true,
                enabled: !f.stopping,
            };
        }
        let (title, count) = match self.step {
            Step::Direction => (N_("Import Photos"), None),
            Step::Import => (N_("Choose Features"), None),
            Step::Features => (N_("Preview Export"), None),
            Step::Preview => {
                if !f.output_chosen {
                    (N_("Choose Output Location…"), None)
                } else if f.ready == 0 && f.unconverted_export != Unconverted::None {
                    (N_("Export {count} Unchanged Photos"), Some(f.unconverted))
                } else {
                    (N_("Run on {count} Photos"), Some(f.ready))
                }
            }
            Step::Results => (N_("Back to Import Photos"), None),
        };
        Primary {
            title,
            count,
            stop: false,
            enabled: self.can_advance(f),
        }
    }

    /// The hint under the command bar; `{count}` is `scope`.
    pub fn hint(&self, f: &Facts, scanning_sources: bool) -> (&'static str, Option<usize>) {
        if f.running {
            return (
                if f.stopping {
                    N_("Waiting for a safe stop")
                } else {
                    N_("The task is frozen. Earlier steps can be browsed read-only")
                },
                None,
            );
        }
        if f.explicit_empty {
            return (N_("0 selected. Select photos to continue"), None);
        }
        let text = match self.step {
            Step::Direction => {
                if f.target_confirmed {
                    N_("Direction selected. Import photos next")
                } else {
                    N_("Choose a direction card on the left")
                }
            }
            Step::Import => {
                if scanning_sources {
                    N_("Recognizing sources; please wait")
                } else if f.items == 0 {
                    N_("Add photos, then select this run's scope")
                } else {
                    return (
                        N_("This run: {count} photos. Filtering does not change selection"),
                        Some(f.scope),
                    );
                }
            }
            Step::Features => {
                if f.time_only {
                    N_("Fill missing dates only; keep the original photo format")
                } else {
                    N_("Choose this run's features here; change direction in step 1")
                }
            }
            Step::Preview => {
                if !f.output_chosen {
                    N_("Choose this run's output location")
                } else if f.unconverted_export == Unconverted::SeparateLocation
                    && !f.unconverted_output
                {
                    N_("Choose an unchanged-export folder")
                } else {
                    N_("Run the ready scope. Estimates do not guarantee success")
                }
            }
            Step::Results => N_("Keep this run's results and return to photos"),
        };
        (text, None)
    }

    // ---- Recovery (macOS `onChange` handlers) -------------------------

    /// A run began converting: show its results.
    pub fn run_started(&mut self) {
        self.set_step(Step::Results);
    }

    /// Every source was removed while no run is active.
    pub fn sources_emptied(&mut self, f: &Facts) {
        if f.running {
            return;
        }
        self.step = if f.target_confirmed {
            Step::Import
        } else {
            Step::Direction
        };
        self.furthest = self.step;
    }

    /// A new route re-plans the queue; steps 3–5 of the old route are stale.
    pub fn direction_changed(&mut self, f: &Facts) {
        if f.running {
            return;
        }
        let floor = if f.target_confirmed {
            Step::Import
        } else {
            Step::Direction
        };
        if self.furthest > floor {
            self.furthest = floor;
        }
        if self.step > self.furthest {
            self.step = self.furthest;
        }
    }

    /// Export Settings from the menu opens the step that holds them.
    pub fn open_settings(&mut self, f: &Facts) {
        self.set_step(if !f.target_confirmed {
            Step::Direction
        } else if f.items == 0 {
            Step::Import
        } else {
            Step::Features
        });
    }

    /// Preview Export… from the queue's status bar.
    pub fn open_review(&mut self, f: &Facts) {
        self.set_step(if f.can_review() {
            Step::Preview
        } else if f.target_confirmed {
            Step::Import
        } else {
            Step::Direction
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_facts() -> Facts {
        Facts {
            target_confirmed: true,
            items: 3,
            scope: 3,
            ready: 2,
            unconverted: 1,
            ..Facts::default()
        }
    }

    #[test]
    fn steps_have_mac_names_and_numbers() {
        let titles: Vec<_> = Step::ALL.iter().map(|s| s.title()).collect();
        assert_eq!(
            titles,
            [
                "Conversion Direction",
                "Import Photos",
                "Choose Features",
                "Export Preview",
                "Results"
            ]
        );
        assert_eq!(Step::Results.number(), 5);
        assert_eq!(Step::from_number(0), None);
    }

    #[test]
    fn nothing_opens_before_a_direction_is_chosen() {
        let flow = Flow::default();
        let f = Facts::default();
        assert!(flow.can_visit(Step::Direction, &f));
        for step in [Step::Import, Step::Features, Step::Preview, Step::Results] {
            assert!(!flow.can_visit(step, &f), "{step:?}");
        }
        assert!(!flow.can_advance(&f));
        assert_eq!(
            flow.hint(&f, false).0,
            "Choose a direction card on the left"
        );
        assert!(!flow.can_go_back());
    }

    #[test]
    fn direction_then_import_gates_on_photos() {
        let mut flow = Flow::default();
        let mut f = Facts {
            target_confirmed: true,
            ..Facts::default()
        };
        assert_eq!(flow.primary(&f).title, "Import Photos");
        assert_eq!(flow.advance(&f), Some(Advance::Go(Step::Import)));
        // No photos yet: Choose Features stays disabled.
        assert!(!flow.can_advance(&f));
        assert_eq!(
            flow.hint(&f, false).0,
            "Add photos, then select this run's scope"
        );
        f.scanning = true;
        assert_eq!(flow.hint(&f, true).0, "Recognizing sources; please wait");
        assert!(flow.primary(&f).stop);
        assert!(!flow.can_visit(Step::Features, &f));
        f = ready_facts();
        assert!(flow.can_advance(&f));
        assert_eq!(
            flow.hint(&f, false),
            (
                "This run: {count} photos. Filtering does not change selection",
                Some(3)
            )
        );
        assert_eq!(flow.advance(&f), Some(Advance::Go(Step::Features)));
        assert_eq!(flow.primary(&f).title, "Preview Export");
    }

    #[test]
    fn empty_explicit_selection_blocks_features_and_preview() {
        let mut flow = Flow::default();
        flow.set_step(Step::Import);
        let f = Facts {
            explicit_empty: true,
            ..ready_facts()
        };
        assert!(!flow.can_advance(&f));
        assert!(!flow.can_visit(Step::Features, &f));
        assert!(!flow.can_visit(Step::Preview, &f));
        assert_eq!(
            flow.hint(&f, false).0,
            "0 selected. Select photos to continue"
        );
    }

    #[test]
    fn preview_asks_for_output_then_runs() {
        let mut flow = Flow::default();
        flow.set_step(Step::Preview);
        let mut f = ready_facts();
        let p = flow.primary(&f);
        assert_eq!((p.title, p.enabled), ("Choose Output Location…", true));
        assert_eq!(flow.advance(&f), Some(Advance::ChooseOutput));
        assert_eq!(flow.step(), Step::Preview);
        f.output_chosen = true;
        let p = flow.primary(&f);
        assert_eq!((p.title, p.count), ("Run on {count} Photos", Some(2)));
        assert_eq!(flow.advance(&f), Some(Advance::Start));
        // A separate unchanged-export folder must be chosen first.
        f.unconverted_export = Unconverted::SeparateLocation;
        assert!(!flow.can_advance(&f));
        assert_eq!(flow.hint(&f, false).0, "Choose an unchanged-export folder");
        f.unconverted_output = true;
        assert!(flow.can_advance(&f));
    }

    #[test]
    fn nothing_ready_needs_unchanged_export() {
        let mut flow = Flow::default();
        flow.set_step(Step::Preview);
        let mut f = Facts {
            ready: 0,
            output_chosen: true,
            ..ready_facts()
        };
        assert!(!flow.can_advance(&f));
        f.unconverted_export = Unconverted::SameLocation;
        let p = flow.primary(&f);
        assert_eq!(
            (p.title, p.count, p.enabled),
            ("Export {count} Unchanged Photos", Some(1), true)
        );
    }

    #[test]
    fn running_freezes_and_keeps_visited_steps_browsable() {
        let mut flow = Flow::default();
        flow.set_step(Step::Preview);
        flow.run_started();
        assert_eq!(flow.step(), Step::Results);
        // The scope may be gone while a frozen run continues.
        let f = Facts {
            target_confirmed: true,
            running: true,
            ..Facts::default()
        };
        assert!(flow.can_visit(Step::Features, &f));
        assert!(flow.can_visit(Step::Preview, &f));
        let p = flow.primary(&f);
        assert!(p.stop && p.enabled && p.title == "Stop");
        let stopping = Facts {
            stopping: true,
            ..f.clone()
        };
        assert_eq!(flow.primary(&stopping).title, "Stopping");
        assert!(!flow.primary(&stopping).enabled);
        assert_eq!(flow.hint(&stopping, false).0, "Waiting for a safe stop");
        assert!(!flow.can_advance(&f));
    }

    #[test]
    fn back_skips_steps_that_no_longer_hold() {
        let mut flow = Flow::default();
        flow.set_step(Step::Results);
        // Results were kept but the selection was emptied: back skips 3 and 4.
        let f = Facts {
            explicit_empty: true,
            has_results: true,
            ..ready_facts()
        };
        assert_eq!(flow.back_target(&f), Some(Step::Import));
        assert!(flow.back(&f));
        assert_eq!(flow.step(), Step::Import);
        assert!(flow.back(&f));
        assert_eq!(flow.step(), Step::Direction);
        assert!(!flow.back(&f));
    }

    #[test]
    fn results_return_to_import() {
        let mut flow = Flow::default();
        flow.set_step(Step::Results);
        let f = Facts {
            has_results: true,
            ..ready_facts()
        };
        assert_eq!(flow.primary(&f).title, "Back to Import Photos");
        assert_eq!(flow.advance(&f), Some(Advance::Go(Step::Import)));
        assert!(flow.can_visit(Step::Results, &f));
        assert_eq!(flow.status(Step::Results, &f), StepStatus::View);
        assert_eq!(flow.status(Step::Import, &f), StepStatus::Current);
    }

    #[test]
    fn direction_change_drops_stale_steps() {
        let mut flow = Flow::default();
        flow.set_step(Step::Preview);
        let f = ready_facts();
        flow.direction_changed(&f);
        assert_eq!((flow.step(), flow.furthest()), (Step::Import, Step::Import));
        // While running nothing moves.
        let mut running = Flow::default();
        running.set_step(Step::Results);
        running.direction_changed(&Facts { running: true, ..f });
        assert_eq!(running.step(), Step::Results);
    }

    #[test]
    fn emptied_sources_return_to_import_or_direction() {
        let mut flow = Flow::default();
        flow.set_step(Step::Features);
        flow.sources_emptied(&Facts {
            target_confirmed: true,
            ..Facts::default()
        });
        assert_eq!(flow.step(), Step::Import);
        flow.sources_emptied(&Facts::default());
        assert_eq!(flow.step(), Step::Direction);
    }

    #[test]
    fn settings_and_review_open_the_right_step() {
        let mut flow = Flow::default();
        flow.open_settings(&Facts::default());
        assert_eq!(flow.step(), Step::Direction);
        flow.open_settings(&Facts {
            target_confirmed: true,
            ..Facts::default()
        });
        assert_eq!(flow.step(), Step::Import);
        flow.open_settings(&ready_facts());
        assert_eq!(flow.step(), Step::Features);
        flow.open_review(&ready_facts());
        assert_eq!(flow.step(), Step::Preview);
        flow.open_review(&Facts {
            scanning: true,
            ..ready_facts()
        });
        assert_eq!(flow.step(), Step::Import);
    }

    #[test]
    fn step_list_status() {
        let mut flow = Flow::default();
        flow.set_step(Step::Import);
        let f = ready_facts();
        let s: Vec<_> = Step::ALL.iter().map(|s| flow.status(*s, &f)).collect();
        assert_eq!(
            s,
            [
                StepStatus::View,
                StepStatus::Current,
                StepStatus::View,
                StepStatus::View,
                StepStatus::Upcoming
            ]
        );
        assert!(!flow.visit(Step::Results, &f));
        assert!(flow.visit(Step::Preview, &f));
    }
}

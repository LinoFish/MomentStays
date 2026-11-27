//! GTK 4 / libadwaita user interface, laid out like the macOS app.
//!
//! The macOS window has two columns under one merged title bar: the photo
//! queue (step title and detail, result menu; feature tabs and browser
//! tools; grid or list with the photo inspector split on its right; status
//! bar) and the 300 pt workflow column (step list, Back + primary action,
//! hint, and the current step's panel). Here the window is an
//! `AdwOverlaySplitView` with the workflow column as its end sidebar; each
//! column is an `AdwToolbarView` whose `AdwHeaderBar` takes the macOS
//! title-bar row, so the window buttons stay in the header bars and there
//! is no extra title row. The step logic lives in `model::flow`.

mod burst;
mod cover;
mod direction;
mod flow;
mod inspector;
mod queue;
mod run;
mod sources;
mod thumbs;
mod window;

use crate::model::flow::{Facts, Flow};
use crate::model::plan::{BurstGroup, Plan};
use crate::model::queue::{Base, Queue};
use crate::model::settings::{ExportSettings, Unconverted};
use crate::model::sources::Sources;
use crate::model::summary::{ProgressSnapshot, RunSummary};
use gtk::{gio, glib};
use momentstays_core::engine::StopToken;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub use window::{about, build};

/// Widgets the handlers update.
pub(crate) struct Widgets {
    pub window: adw::ApplicationWindow,
    pub toasts: adw::ToastOverlay,
    pub canvas: window::Canvas,
    pub queue: queue::QueueView,
    pub inspector: inspector::Inspector,
    pub direction: direction::DirectionCanvas,
    pub flow: flow::FlowColumn,
}

/// The finished run shown in step 5.
#[derive(Clone, Debug, Default)]
pub(crate) struct LastRun {
    pub summary: RunSummary,
    /// Unchanged export counts (copied, resumed, skipped, failed), if any ran.
    pub originals: Option<[usize; 4]>,
    pub output: PathBuf,
}

/// Application state plus widgets. Single window, kept for the app's life.
pub(crate) struct App {
    pub w: Widgets,
    pub sources: RefCell<Sources>,
    pub queue: RefCell<Queue>,
    pub settings: RefCell<ExportSettings>,
    pub flow: Cell<Flow>,
    /// A direction card was chosen (macOS `migrationTargetConfirmed`).
    pub target_confirmed: Cell<bool>,
    pub unconverted: Cell<Unconverted>,
    /// The location remembered while unchanged export is off.
    pub unconverted_location: Cell<Unconverted>,
    pub unconverted_output: RefCell<Option<PathBuf>>,
    pub output: RefCell<Option<PathBuf>>,
    pub groups: RefCell<Vec<BurstGroup>>,
    /// Bumped by every scan; a finished scan only applies if it is current.
    pub scan_generation: Cell<u64>,
    pub scanning: Cell<bool>,
    pub scan_token: RefCell<Option<Arc<StopToken>>>,
    pub running: RefCell<Option<Arc<StopToken>>>,
    pub stopping: Cell<bool>,
    pub progress: RefCell<Option<ProgressSnapshot>>,
    pub last: RefCell<Option<LastRun>>,
    pub error: RefCell<Option<String>>,
    pub monitors: RefCell<HashMap<PathBuf, gio::FileMonitor>>,
    pub rescan_timer: RefCell<Option<glib::SourceId>>,
    /// Set while widgets are refilled, so their signals do not write back.
    pub binding: Cell<bool>,
    pub thumbs: Rc<thumbs::Thumbnails>,
    pub prefers_grid: Cell<bool>,
    pub search_expanded: Cell<bool>,
    /// What the current step panel was built from; rebuilt when it changes.
    pub panel_key: RefCell<String>,
}

impl App {
    pub fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(5);
        self.w.toasts.add_toast(toast);
    }

    /// A conversion or unchanged export runs (macOS `executionActive`).
    pub fn busy(&self) -> bool {
        self.running.borrow().is_some()
    }

    /// The browser's base for the current step.
    pub fn base(&self) -> Base {
        match self.flow.get().step() {
            crate::model::flow::Step::Preview => Base::Scope,
            crate::model::flow::Step::Results => Base::Results,
            _ => Base::All,
        }
    }

    pub fn with_plan<R>(&self, f: impl FnOnce(&Plan) -> R) -> R {
        let queue = self.queue.borrow();
        let settings = self.settings.borrow();
        let groups = self.groups.borrow();
        let plan = Plan {
            queue: &queue,
            settings: &settings,
            groups: &groups,
            unconverted: self.unconverted.get(),
        };
        f(&plan)
    }

    pub fn facts(&self) -> Facts {
        let (scope, ready, unconverted, time_only) = self.with_plan(|p| {
            (
                p.scope().len(),
                p.ready().len(),
                p.unconverted_items().len(),
                p.time_only(),
            )
        });
        let queue = self.queue.borrow();
        Facts {
            target_confirmed: self.target_confirmed.get(),
            scanning: self.scanning.get(),
            running: self.busy(),
            stopping: self.stopping.get(),
            items: queue.len(),
            explicit_empty: queue.has_explicit_selection() && queue.selected_count() == 0,
            scope,
            ready,
            unconverted,
            unconverted_export: self.unconverted.get(),
            unconverted_output: self.unconverted_output.borrow().is_some(),
            output_chosen: self.output.borrow().is_some(),
            has_results: self.last.borrow().is_some() || queue.has_results(),
            time_only,
        }
    }

    pub fn update_flow(&self, f: impl FnOnce(&mut Flow)) {
        let mut flow = self.flow.get();
        f(&mut flow);
        self.flow.set(flow);
    }
}

pub(crate) type Shared = Rc<App>;

/// Bring every view up to date with the state.
pub(crate) fn sync(app: &Shared) {
    queue::refresh(app);
    inspector::show(app);
    flow::sync(app);
    window::sync(app);
}

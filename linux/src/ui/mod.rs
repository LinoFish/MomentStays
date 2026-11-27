//! GTK 4 / libadwaita user interface.
//!
//! One `AdwApplicationWindow` with client-side decorations. Panes are an
//! outer `AdwOverlaySplitView` (source folders) around an inner one
//! (photo queue | inspector); each pane is an `AdwToolbarView` whose
//! `AdwHeaderBar` holds that pane's controls and the window buttons.
//! Breakpoints collapse the inspector, then the sidebar, on narrow windows.

mod grid;
mod inspector;
mod prefs;
mod run;
mod sidebar;
mod thumbs;
mod window;

use crate::model::queue::Queue;
use crate::model::settings::ExportSettings;
use crate::model::sources::Sources;
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
    pub inner: adw::OverlaySplitView,
    // Sidebar
    pub folder_list: gtk::ListBox,
    pub sidebar_stack: gtk::Stack,
    // Content header
    pub to_apple: gtk::ToggleButton,
    pub to_android: gtk::ToggleButton,
    pub output_button: gtk::Button,
    pub output_label: gtk::Label,
    pub start_button: gtk::Button,
    pub search_bar: gtk::SearchBar,
    pub search_entry: gtk::SearchEntry,
    pub filters: Vec<(crate::model::queue::Filter, gtk::ToggleButton)>,
    // Content
    pub content_stack: gtk::Stack,
    pub empty_page: adw::StatusPage,
    pub store: gio::ListStore,
    pub selection: gtk::SingleSelection,
    pub status_label: gtk::Label,
    pub progress_bar: gtk::ProgressBar,
    pub progress_box: gtk::Box,
    pub stop_button: gtk::Button,
    // Inspector
    pub inspector: inspector::Inspector,
}

/// Application state plus widgets. Single window, kept for the app's life.
pub(crate) struct App {
    pub w: Widgets,
    pub sources: RefCell<Sources>,
    pub queue: RefCell<Queue>,
    pub settings: RefCell<ExportSettings>,
    pub output: RefCell<Option<PathBuf>>,
    /// Bumped by every scan; a finished scan only applies if it is current.
    pub scan_generation: Cell<u64>,
    pub scanning: Cell<bool>,
    pub running: RefCell<Option<Arc<StopToken>>>,
    pub monitors: RefCell<HashMap<PathBuf, gio::FileMonitor>>,
    pub rescan_timer: RefCell<Option<glib::SourceId>>,
    /// Set while the grid is being refilled, so cell updates do not write back.
    pub binding: Cell<bool>,
    pub thumbs: Rc<thumbs::Thumbnails>,
}

impl App {
    pub fn toast(&self, text: &str) {
        let toast = adw::Toast::new(text);
        toast.set_timeout(5);
        self.w.toasts.add_toast(toast);
    }

    pub fn busy(&self) -> bool {
        self.running.borrow().is_some()
    }
}

pub(crate) type Shared = Rc<App>;

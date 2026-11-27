//! Source folders: add (portal folder chooser), remove, monitoring.

use super::Shared;
use crate::i18n::{fill, gettext};
use crate::model::sources::{AddError, AddOutcome};
use adw::prelude::*;
use gtk::{gio, glib};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Open the folder chooser (xdg-desktop-portal FileChooser inside Flatpak).
pub fn choose_folder(app: &Shared) {
    if app.busy() {
        return;
    }
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Add Source Folder"))
        .accept_label(gettext("_Add"))
        .modal(true)
        .build();
    let a = app.clone();
    dialog.select_folder(Some(&app.w.window), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else { return };
        let Some(path) = file.path() else {
            a.toast(&gettext("This folder cannot be opened as a local folder."));
            return;
        };
        add_folder(&a, path);
    });
}

pub fn add_folder(app: &Shared, path: PathBuf) {
    let outcome = app.sources.borrow_mut().add(path);
    match outcome {
        Ok(AddOutcome::Added) => {}
        Ok(AddOutcome::Merged(_)) => {
            app.toast(&gettext(
                "Folders inside the new folder were merged into it.",
            ));
        }
        Err(AddError::AlreadyCovered(existing)) => {
            app.toast(&fill(
                &gettext("Already included in “{folder}”."),
                &[("folder", &display_name(&existing))],
            ));
            return;
        }
    }
    sync_monitors(app);
    refresh(app);
    super::grid::scan(app);
}

fn remove_folder(app: &Shared, path: &Path) {
    if app.busy() {
        return;
    }
    app.sources.borrow_mut().remove(path);
    sync_monitors(app);
    refresh(app);
    super::grid::scan(app);
}

fn set_monitoring(app: &Shared, path: &Path, on: bool) {
    if app.sources.borrow_mut().set_monitoring(path, on) {
        sync_monitors(app);
    }
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Rebuild the folder rows from the model.
pub fn refresh(app: &Shared) {
    let list = &app.w.folder_list;
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let folders = app.sources.borrow().folders().to_vec();
    app.w
        .sidebar_stack
        .set_visible_child_name(if folders.is_empty() { "empty" } else { "list" });
    for folder in folders {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&display_name(&folder.path)))
            .subtitle(glib::markup_escape_text(&folder.path.to_string_lossy()))
            .subtitle_lines(1)
            .build();
        row.set_tooltip_text(Some(&folder.path.to_string_lossy()));
        row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));

        let monitor = gtk::Switch::builder()
            .active(folder.monitoring)
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Watch for Changes"))
            .build();
        monitor.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Watch for Changes",
        ))]);
        let a = app.clone();
        let p = folder.path.clone();
        monitor.connect_active_notify(move |s| set_monitoring(&a, &p, s.is_active()));
        row.add_suffix(&monitor);

        let remove = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Remove Folder"))
            .css_classes(["flat", "circular"])
            .build();
        let a = app.clone();
        let p = folder.path.clone();
        remove.connect_clicked(move |_| remove_folder(&a, &p));
        row.add_suffix(&remove);
        list.append(&row);
    }
}

/// Keep one GIO directory monitor per watched folder. GFileMonitor watches
/// the folder itself, not its subfolders; changes there trigger a
/// debounced rescan of all sources.
fn sync_monitors(app: &Shared) {
    let wanted: Vec<PathBuf> = app
        .sources
        .borrow()
        .folders()
        .iter()
        .filter(|f| f.monitoring)
        .map(|f| f.path.clone())
        .collect();
    let mut monitors = app.monitors.borrow_mut();
    monitors.retain(|path, monitor| {
        let keep = wanted.contains(path);
        if !keep {
            monitor.cancel();
        }
        keep
    });
    for path in wanted {
        if monitors.contains_key(&path) {
            continue;
        }
        let file = gio::File::for_path(&path);
        match file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            Ok(monitor) => {
                let a = app.clone();
                monitor.connect_changed(move |_, _, _, event| {
                    use gio::FileMonitorEvent as E;
                    if matches!(
                        event,
                        E::Created
                            | E::Deleted
                            | E::MovedIn
                            | E::MovedOut
                            | E::Renamed
                            | E::ChangesDoneHint
                    ) {
                        schedule_rescan(&a);
                    }
                });
                monitors.insert(path, monitor);
            }
            Err(e) => app.toast(&fill(
                &gettext("Cannot watch “{folder}”: {error}"),
                &[("folder", &display_name(&path)), ("error", e.message())],
            )),
        }
    }
}

fn schedule_rescan(app: &Shared) {
    if let Some(id) = app.rescan_timer.borrow_mut().take() {
        id.remove();
    }
    let a = app.clone();
    let id = glib::timeout_add_local_once(Duration::from_millis(1500), move || {
        a.rescan_timer.borrow_mut().take();
        if !a.busy() {
            super::grid::scan(&a);
        }
    });
    *app.rescan_timer.borrow_mut() = Some(id);
}

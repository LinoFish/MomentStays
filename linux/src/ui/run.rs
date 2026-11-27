//! Output folder, running a conversion off the main thread, progress and
//! the result summary.

use super::{sidebar::display_name, window::update_status, Shared};
use crate::engine::{self, SharedProgress};
use crate::i18n::{fill, gettext};
use crate::model::settings::batch_request;
use crate::model::summary::RunSummary;
use adw::prelude::*;
use gtk::{gio, glib};
use momentstays_core::engine::StopToken;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Choose the output folder (portal); optionally start once chosen.
pub fn choose_output(app: &Shared, then_start: bool) {
    if app.busy() {
        return;
    }
    let dialog = gtk::FileDialog::builder()
        .title(gettext("Choose Output Folder"))
        .accept_label(gettext("_Select"))
        .modal(true)
        .build();
    if let Some(current) = app.output.borrow().as_ref() {
        dialog.set_initial_folder(Some(&gio::File::for_path(current)));
    }
    let a = app.clone();
    dialog.select_folder(Some(&app.w.window), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else { return };
        let Some(path) = file.path() else {
            a.toast(&gettext("This folder cannot be opened as a local folder."));
            return;
        };
        if a.sources.borrow().conflicts_with_output(&path) {
            a.toast(&gettext(
                "Choose an output folder outside the source folders.",
            ));
            return;
        }
        set_output(&a, path);
        if then_start {
            start(&a);
        }
    });
}

fn set_output(app: &Shared, path: PathBuf) {
    app.w.output_label.set_label(&display_name(&path));
    app.w
        .output_button
        .set_tooltip_text(Some(&path.to_string_lossy()));
    *app.output.borrow_mut() = Some(path);
}

pub fn start(app: &Shared) {
    if app.busy() || app.scanning.get() {
        return;
    }
    let Some(output) = app.output.borrow().clone() else {
        choose_output(app, true);
        return;
    };
    if app.sources.borrow().conflicts_with_output(&output) {
        app.toast(&gettext(
            "Choose an output folder outside the source folders.",
        ));
        return;
    }
    let (sources, rows) = app.queue.borrow().selection();
    if sources.is_empty() {
        app.toast(&gettext("No photos are included."));
        return;
    }
    let req = batch_request(&app.settings.borrow(), &sources, &rows, &output);
    let token = Arc::new(StopToken::new());
    let progress = Arc::new(SharedProgress::default());
    *app.running.borrow_mut() = Some(token.clone());
    set_running_ui(app, true);

    // Poll the shared progress; the core reports from its worker threads.
    let a = app.clone();
    let p = progress.clone();
    let poll = glib::timeout_add_local(Duration::from_millis(150), move || {
        let snap = p.snapshot();
        a.w.progress_bar.set_fraction(snap.fraction());
        a.w.status_label.set_label(&fill(
            &gettext("Converting {done} of {total}…"),
            &[
                ("done", &snap.done.to_string()),
                ("total", &snap.total.to_string()),
            ],
        ));
        a.w.progress_bar.set_tooltip_text(Some(&snap.current));
        glib::ControlFlow::Continue
    });

    let a = app.clone();
    glib::spawn_future_local(async move {
        let result = gio::spawn_blocking(move || engine::run(&req, progress, token))
            .await
            .unwrap_or_else(|_| Err(gettext("The conversion stopped unexpectedly.")));
        poll.remove();
        a.running.borrow_mut().take();
        set_running_ui(&a, false);
        update_status(&a);
        match result {
            Ok(reply) => show_summary(&a, &RunSummary::from_result(&reply), &output),
            Err(message) => a.toast(&message),
        }
    });
}

pub fn stop(app: &Shared) {
    if let Some(token) = app.running.borrow().as_ref() {
        token.cancel();
        app.w.stop_button.set_sensitive(false);
        app.w.status_label.set_label(&gettext("Stopping safely…"));
    }
}

fn set_running_ui(app: &Shared, running: bool) {
    let w = &app.w;
    w.progress_box.set_visible(running);
    w.progress_bar.set_fraction(0.0);
    w.stop_button.set_sensitive(running);
    w.start_button.set_sensitive(!running);
    w.output_button.set_sensitive(!running);
    w.to_apple.set_sensitive(!running);
    w.to_android.set_sensitive(!running);
    w.folder_list.set_sensitive(!running);
    for name in [
        "add-folder",
        "preferences",
        "include-shown",
        "exclude-shown",
    ] {
        if let Some(action) = w
            .window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(!running);
        }
    }
}

fn show_summary(app: &Shared, s: &RunSummary, output: &Path) {
    let heading = if s.cancelled {
        gettext("Conversion Stopped")
    } else if s.ok() {
        gettext("Conversion Finished")
    } else {
        gettext("Finished with Problems")
    };
    let mut body = fill(
        &gettext("Converted: {converted}\nVerified existing: {resumed}\nCopied unchanged: {copied}\nSkipped: {skipped}\nFailed: {failed}"),
        &[
            ("converted", &s.converted.to_string()),
            ("resumed", &s.resumed.to_string()),
            ("copied", &s.copied.to_string()),
            ("skipped", &s.skipped.to_string()),
            ("failed", &s.failed.to_string()),
        ],
    );
    for f in s.failures.iter().take(3) {
        let name = Path::new(&f.source)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.source.clone());
        body.push_str(&format!("\n\n{name}: {}", f.detail));
    }
    if s.failures.len() > 3 {
        body.push_str("\n\n");
        body.push_str(&gettext(
            "See the report in the output folder for every photo.",
        ));
    }
    let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
    dialog.add_response("close", &gettext("_Close"));
    dialog.add_response("open", &gettext("_Open Output Folder"));
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("open"));
    dialog.set_close_response("close");
    let window = app.w.window.clone();
    let folder = output.to_path_buf();
    let a = app.clone();
    dialog.connect_response(None, move |_, response| {
        if response == "open" {
            let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&folder)));
            let a = a.clone();
            launcher.launch(Some(&window), gio::Cancellable::NONE, move |r| {
                if let Err(e) = r {
                    a.toast(e.message());
                }
            });
        }
    });
    dialog.present(Some(&app.w.window));
    if s.produced() > 0 {
        app.toast(&fill(
            &gettext("Results are in “{folder}”."),
            &[("folder", &display_name(output))],
        ));
    }
}

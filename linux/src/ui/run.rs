//! Output locations, running the approved plan off the main thread
//! (conversion, then the unchanged export), progress and results.

use super::{sources::display_name, LastRun, Shared};
use crate::engine::{self, SharedProgress};
use crate::i18n::{fill, gettext};
use crate::model::plan::burst_choices;
use crate::model::settings::{
    batch_request, originals_export_request, originals_plan_request, Unconverted,
};
use crate::model::summary::RunSummary;
use adw::prelude::*;
use gtk::{gio, glib};
use momentstays_core::engine::StopToken;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn pick_folder(
    app: &Shared,
    title: &str,
    initial: Option<PathBuf>,
    done: impl Fn(&Shared, PathBuf) + 'static,
) {
    if app.busy() {
        return;
    }
    let dialog = gtk::FileDialog::builder()
        .title(title)
        .accept_label(gettext("_Select"))
        .modal(true)
        .build();
    if let Some(current) = initial {
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
        done(&a, path);
        super::sync(&a);
    });
}

/// Choose this run's output folder (portal); optionally start once chosen.
pub fn choose_output(app: &Shared, then_start: bool) {
    let initial = app.output.borrow().clone();
    pick_folder(
        app,
        &gettext("Choose Output Folder"),
        initial,
        move |a, path| {
            *a.output.borrow_mut() = Some(path);
            if then_start {
                start(a);
            }
        },
    );
}

pub fn choose_unconverted_output(app: &Shared) {
    let initial = app.unconverted_output.borrow().clone();
    pick_folder(
        app,
        &gettext("Choose an unchanged-export folder"),
        initial,
        |a, path| {
            *a.unconverted_output.borrow_mut() = Some(path);
        },
    );
}

pub fn install(app: &Shared) {
    let action = gio::SimpleAction::new("choose-unconverted-output", None);
    let a = app.clone();
    action.connect_activate(move |_, _| choose_unconverted_output(&a));
    app.w.window.add_action(&action);
}

struct Job {
    batch: Option<Value>,
    originals: Option<(Vec<Value>, PathBuf)>,
    /// Sources of the photos expected to run (failed ones may go unchanged).
    ready_rows: Vec<Value>,
    existing: crate::model::settings::ExistingOutput,
    direction: crate::model::settings::Direction,
}

struct Outcome {
    batch: Option<Result<Value, String>>,
    originals: Option<Result<Value, String>>,
}

fn run_job(job: Job, progress: Arc<SharedProgress>, token: Arc<StopToken>) -> Outcome {
    let batch = job
        .batch
        .as_ref()
        .map(|req| engine::run(req, progress.clone(), token.clone()));
    let originals = job.originals.map(|(mut rows, root)| {
        // Photos that failed to convert are exported unchanged too (macOS
        // `remainingOriginals`).
        if let Some(Ok(reply)) = &batch {
            let failed: HashSet<&str> = reply["report_data"]["files"]
                .as_array()
                .map(|f| {
                    f.iter()
                        .filter(|f| f["status"] == "failed")
                        .filter_map(|f| f["source"].as_str())
                        .collect()
                })
                .unwrap_or_default();
            rows.extend(
                job.ready_rows
                    .iter()
                    .filter(|r| r["source"].as_str().is_some_and(|s| failed.contains(s)))
                    .cloned(),
            );
        }
        if rows.is_empty() || token.is_cancelled() {
            return Ok(serde_json::json!({"counts": {}, "report_data": {"files": []}}));
        }
        let plan = engine::run(
            &originals_plan_request(job.direction, &rows),
            Arc::default(),
            token.clone(),
        )?;
        let mut reply = engine::run(
            &originals_export_request(&plan["plans"], &root, job.existing),
            progress.clone(),
            token.clone(),
        )?;
        // Resources the plan could not freeze count as failed.
        if let (Some(files), Some(failed)) = (
            reply["report_data"]["files"].as_array_mut(),
            plan["files"].as_array(),
        ) {
            files.extend(failed.iter().cloned());
            if let Some(n) = reply["counts"]["failed"].as_u64() {
                reply["counts"]["failed"] = Value::from(n + failed.len() as u64);
            }
        }
        Ok(reply)
    });
    Outcome { batch, originals }
}

/// Approve the plan and run it (macOS `approveMigrationReview`).
pub fn start(app: &Shared) {
    if app.busy() || app.scanning.get() {
        return;
    }
    let facts = app.facts();
    if !facts.can_approve() {
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
    let choices = app.with_plan(burst_choices);
    app.settings.borrow_mut().burst_choices = choices;
    let (ready, unconverted, scope) = app.with_plan(|p| {
        let ready: Vec<(PathBuf, Value)> = p
            .ready()
            .iter()
            .map(|i| (i.source.clone(), i.row.clone()))
            .collect();
        let unconverted: Vec<Value> = p
            .unconverted_items()
            .iter()
            .map(|i| i.row.clone())
            .collect();
        let scope: HashSet<PathBuf> = p.scope().iter().map(|i| i.source.clone()).collect();
        (ready, unconverted, scope)
    });
    let settings = app.settings.borrow().clone();
    let originals_root = match app.unconverted.get() {
        Unconverted::None => None,
        Unconverted::SameLocation => Some(output.clone()),
        Unconverted::SeparateLocation => app.unconverted_output.borrow().clone(),
    };
    let (sources, rows): (Vec<PathBuf>, Vec<Value>) = ready.iter().cloned().unzip();
    let job = Job {
        batch: (!sources.is_empty()).then(|| batch_request(&settings, &sources, &rows, &output)),
        originals: originals_root.map(|root| (unconverted, root)),
        ready_rows: rows,
        existing: settings.existing_output,
        direction: settings.direction,
    };
    let token = Arc::new(StopToken::new());
    let progress = Arc::new(SharedProgress::default());
    *app.running.borrow_mut() = Some(token.clone());
    app.stopping.set(false);
    *app.progress.borrow_mut() = Some(Default::default());
    app.error.borrow_mut().take();
    app.last.borrow_mut().take();
    app.update_flow(|f| f.run_started());
    super::sync(app);

    // Poll the shared progress; the core reports from its worker threads.
    let a = app.clone();
    let p = progress.clone();
    let poll = glib::timeout_add_local(Duration::from_millis(150), move || {
        let snap = p.snapshot();
        if a.progress.borrow().as_ref() != Some(&snap) {
            *a.progress.borrow_mut() = Some(snap);
            super::window::sync(&a);
            super::flow::sync(&a);
        }
        glib::ControlFlow::Continue
    });

    let a = app.clone();
    glib::spawn_future_local(async move {
        let outcome = gio::spawn_blocking(move || run_job(job, progress, token))
            .await
            .unwrap_or(Outcome {
                batch: Some(Err(gettext("The conversion stopped unexpectedly."))),
                originals: None,
            });
        poll.remove();
        a.running.borrow_mut().take();
        a.stopping.set(false);
        let mut files: Vec<Value> = Vec::new();
        let mut summary = RunSummary::default();
        let mut error = None;
        match outcome.batch {
            Some(Ok(reply)) => {
                summary = RunSummary::from_result(&reply);
                files.extend(
                    reply["report_data"]["files"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
            }
            Some(Err(e)) => error = Some(e),
            None => {}
        }
        let mut originals = None;
        match outcome.originals {
            Some(Ok(reply)) => {
                let n = |k: &str| reply["counts"][k].as_u64().unwrap_or(0) as usize;
                originals = Some([n("copied"), n("resumed"), n("skipped"), n("failed")]);
                summary.cancelled |= reply["cancelled"] == true;
                for f in reply["report_data"]["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let sources: Vec<Value> = f["sources"]
                        .as_array()
                        .cloned()
                        .unwrap_or_else(|| vec![f["source"].clone()]);
                    for s in sources {
                        let mut entry = f.clone();
                        entry["source"] = s;
                        files.push(entry);
                    }
                }
            }
            Some(Err(e)) => error = Some(e),
            None => {}
        }
        a.queue.borrow_mut().set_results(scope, &files);
        *a.last.borrow_mut() = Some(LastRun {
            summary: summary.clone(),
            originals,
            output: output.clone(),
        });
        *a.error.borrow_mut() = error.clone();
        a.progress.borrow_mut().take();
        super::sync(&a);
        if let Some(e) = error {
            a.toast(&e);
        } else if summary.produced() > 0 || originals.is_some_and(|o| o[0] + o[1] > 0) {
            a.toast(&fill(
                &gettext("Results are in “{folder}”."),
                &[("folder", &display_name(&output))],
            ));
        }
    });
}

pub fn stop(app: &Shared) {
    if let Some(token) = app.running.borrow().as_ref() {
        token.cancel();
        app.stopping.set(true);
    }
    super::sync(app);
}

fn launch(app: &Shared, path: &Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    let a = app.clone();
    launcher.launch(Some(&app.w.window), gio::Cancellable::NONE, move |r| {
        if let Err(e) = r {
            a.toast(e.message());
        }
    });
}

pub fn open_output(app: &Shared) {
    let path = app
        .last
        .borrow()
        .as_ref()
        .map(|l| l.output.clone())
        .or_else(|| app.output.borrow().clone());
    if let Some(p) = path {
        launch(app, &p);
    }
}

pub fn open_report(app: &Shared) {
    let report = app
        .last
        .borrow()
        .as_ref()
        .and_then(|l| l.summary.report.clone());
    if let Some(r) = report {
        launch(app, &r);
    }
}

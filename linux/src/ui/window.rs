//! Window layout (macOS `WorkspaceView`): the queue canvas and the
//! workflow column, header rows, task feedback, status bar, breakpoints.

use super::{direction, flow, inspector, queue, App, Shared, Widgets};
use crate::config::{APP_ID, NATIVE_CODECS, VERSION};
use crate::i18n::{fill, gettext};
use crate::model::claims;
use crate::model::flow::Step;
use crate::model::queue::Base;
use crate::model::settings::Unconverted;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The queue column's chrome around the browser.
pub struct Canvas {
    pub title: gtk::Label,
    pub detail: gtk::Label,
    pub stack: gtk::Stack,
    pub queue_bars: gtk::Box,
    pub feedback: gtk::Revealer,
    feedback_icon: gtk::Image,
    feedback_label: gtk::Label,
    feedback_percent: gtk::Label,
    feedback_button: gtk::Button,
    feedback_progress: gtk::ProgressBar,
    feedback_secondary: gtk::Label,
    review_hint: gtk::Box,
    review_icon: gtk::Image,
    review_label: gtk::Label,
    pub status_bar: gtk::Box,
    selection_summary: gtk::Label,
    restore_scope: gtk::Button,
    showing: gtk::Label,
    preview_export: gtk::Button,
}

fn result_menu() -> gio::Menu {
    // macOS result menu (`checkmark.rectangle.stack`); the app menu's
    // Keyboard Shortcuts and About live here too, as GNOME has no menu bar.
    let menu = gio::Menu::new();
    let results = gio::Menu::new();
    results.append(
        Some(&gettext("Open Output Folder")),
        Some("win.open-output"),
    );
    results.append(
        Some(&gettext("Processing Report…")),
        Some("win.open-report"),
    );
    menu.append_section(None, &results);
    let app = gio::Menu::new();
    app.append(Some(&gettext("_Export Settings")), Some("win.preferences"));
    app.append(Some(&gettext("_Keyboard Shortcuts")), Some("win.shortcuts"));
    app.append(Some(&gettext("_About MomentStays")), Some("app.about"));
    menu.append_section(None, &app);
    menu
}

fn caption(text: &str) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build();
    l.add_css_class("caption");
    l.add_css_class("dim-label");
    l
}

pub fn build(application: &adw::Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title(gettext("MomentStays"))
        .default_width(1120)
        .default_height(720)
        .width_request(360)
        .height_request(294)
        .build();

    // ---- Queue column header (the title-bar row) ------------------------
    let title = gtk::Label::builder().xalign(0.0).build();
    title.add_css_class("heading");
    let detail = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .hexpand(true)
        .build();
    detail.add_css_class("caption");
    detail.add_css_class("dim-label");
    let title_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    title_box.set_hexpand(true);
    title_box.append(&title);
    title_box.append(&detail);
    let result_button = gtk::MenuButton::builder()
        .icon_name("task-due-symbolic")
        .menu_model(&result_menu())
        .tooltip_text(gettext(
            "Export, share, reveal outputs, reports, and recent results without converting again",
        ))
        .primary(true)
        .build();
    result_button.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Results and Delivery",
    ))]);
    let flow_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text(gettext("Workflow"))
        .visible(false)
        .build();
    let header = adw::HeaderBar::new();
    header.set_show_end_title_buttons(false);
    header.set_title_widget(Some(&title_box));
    header.pack_end(&flow_toggle);
    header.pack_end(&result_button);

    // ---- Queue toolbar, feedback, hint ----------------------------------
    let queue_view = queue::QueueView::new(&window);

    let feedback_icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    let feedback_label = gtk::Label::builder()
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let feedback_percent = gtk::Label::new(None);
    feedback_percent.add_css_class("numeric");
    feedback_percent.add_css_class("dim-label");
    let feedback_button = gtk::Button::new();
    feedback_button.add_css_class("flat");
    let feedback_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    feedback_row.append(&feedback_icon);
    feedback_row.append(&feedback_label);
    feedback_row.append(&feedback_percent);
    feedback_row.append(&feedback_button);
    let feedback_progress = gtk::ProgressBar::new();
    let feedback_secondary = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .build();
    feedback_secondary.add_css_class("caption");
    feedback_secondary.add_css_class("dim-label");
    let feedback_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
    feedback_box.set_margin_start(16);
    feedback_box.set_margin_end(16);
    feedback_box.set_margin_top(8);
    feedback_box.set_margin_bottom(8);
    feedback_box.append(&feedback_row);
    feedback_box.append(&feedback_progress);
    feedback_box.append(&feedback_secondary);
    feedback_progress
        .update_property(&[gtk::accessible::Property::Label(&gettext("Task progress"))]);
    let feedback = gtk::Revealer::builder()
        .child(&feedback_box)
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .build();

    let review_icon = gtk::Image::from_icon_name("view-reveal-symbolic");
    let review_label = caption("");
    review_label.set_hexpand(true);
    let review_hint = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    review_hint.set_margin_start(12);
    review_hint.set_margin_end(12);
    review_hint.set_margin_top(4);
    review_hint.set_margin_bottom(4);
    review_hint.append(&review_icon);
    review_hint.append(&review_label);

    let banner = adw::Banner::new(&gettext(claims::NO_NATIVE_CODECS));
    banner.set_revealed(!NATIVE_CODECS);

    // Bars that belong to the queue (hidden in step 1, as on macOS).
    let queue_bars = gtk::Box::new(gtk::Orientation::Vertical, 0);
    queue_bars.append(&queue_view.heading);
    queue_bars.append(&queue_view.search_below);
    queue_bars.append(&feedback);
    queue_bars.append(&review_hint);

    // ---- Status bar -----------------------------------------------------
    let selection_summary = gtk::Label::builder()
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    selection_summary.add_css_class("caption");
    let restore_scope = gtk::Button::with_label(&gettext("Restore Default Scope"));
    restore_scope.add_css_class("flat");
    restore_scope.add_css_class("caption");
    restore_scope.set_action_name(Some("win.restore-scope"));
    let showing = gtk::Label::builder().hexpand(true).xalign(1.0).build();
    showing.add_css_class("caption");
    showing.add_css_class("dim-label");
    showing.add_css_class("numeric");
    let preview_export = gtk::Button::with_label(&gettext("Preview Export…"));
    preview_export.add_css_class("flat");
    preview_export.add_css_class("caption");
    preview_export.set_action_name(Some("win.review"));
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    status_bar.set_margin_start(12);
    status_bar.set_margin_end(12);
    status_bar.set_margin_top(2);
    status_bar.set_margin_bottom(2);
    status_bar.append(&selection_summary);
    status_bar.append(&restore_scope);
    status_bar.append(&showing);
    status_bar.append(&preview_export);

    // ---- Queue column ---------------------------------------------------
    let direction = direction::DirectionCanvas::new();
    let inspector = inspector::Inspector::new();
    inspector.root.prepend(&queue_view.inspector_back);
    queue_view
        .inspector_split
        .set_sidebar(Some(&inspector.root));
    let stack = gtk::Stack::new();
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    stack.add_named(&direction.root, Some("direction"));
    stack.add_named(&queue_view.inspector_split, Some("queue"));

    let canvas_view = adw::ToolbarView::new();
    canvas_view.add_top_bar(&header);
    canvas_view.add_top_bar(&banner);
    canvas_view.add_top_bar(&queue_bars);
    canvas_view.set_content(Some(&stack));
    canvas_view.add_bottom_bar(&status_bar);

    // ---- Workflow column -------------------------------------------------
    let flow_column = flow::FlowColumn::new();

    // macOS: the workflow column is 300 pt, 264 pt below 900 pt.
    let split = adw::OverlaySplitView::builder()
        .content(&canvas_view)
        .sidebar(&flow_column.root)
        .sidebar_position(gtk::PackType::End)
        .min_sidebar_width(300.0)
        .max_sidebar_width(300.0)
        .sidebar_width_fraction(1.0)
        .enable_hide_gesture(false)
        .enable_show_gesture(false)
        .build();
    flow_toggle
        .bind_property("active", &split, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();
    split.set_show_sidebar(true);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&split));
    window.set_content(Some(&toasts));

    breakpoints(&window, &split, &header, &flow_toggle, &queue_view);

    let canvas = Canvas {
        title,
        detail,
        stack,
        queue_bars,
        feedback,
        feedback_icon,
        feedback_label,
        feedback_percent,
        feedback_button,
        feedback_progress,
        feedback_secondary,
        review_hint,
        review_icon,
        review_label,
        status_bar,
        selection_summary,
        restore_scope,
        showing,
        preview_export,
    };

    let app: Shared = Rc::new(App {
        w: Widgets {
            window: window.clone(),
            toasts,
            canvas,
            queue: queue_view,
            inspector,
            direction,
            flow: flow_column,
        },
        sources: RefCell::default(),
        queue: RefCell::default(),
        settings: RefCell::new(crate::model::settings::ExportSettings {
            photo_time_zone: glib::TimeZone::local().identifier().to_string(),
            ..Default::default()
        }),
        flow: Cell::default(),
        target_confirmed: Cell::new(false),
        unconverted: Cell::new(Unconverted::None),
        unconverted_location: Cell::new(Unconverted::SameLocation),
        unconverted_output: RefCell::default(),
        output: RefCell::default(),
        groups: RefCell::default(),
        scan_generation: Cell::new(0),
        scanning: Cell::new(false),
        scan_token: RefCell::default(),
        running: RefCell::default(),
        stopping: Cell::new(false),
        progress: RefCell::default(),
        last: RefCell::default(),
        error: RefCell::default(),
        monitors: RefCell::default(),
        rescan_timer: RefCell::default(),
        binding: Cell::new(false),
        thumbs: Rc::default(),
        prefers_grid: Cell::new(true),
        search_expanded: Cell::new(false),
        panel_key: RefCell::default(),
    });

    queue::connect(&app);
    flow::connect(&app);
    direction::connect(&app);
    inspector::connect(&app);
    let a = app.clone();
    app.w.canvas.feedback_button.connect_clicked(move |_| {
        if let Some(message) = a.error.borrow().clone() {
            let dialog =
                adw::AlertDialog::new(Some(&gettext("Cannot Complete Operation")), Some(&message));
            dialog.add_response("ok", &gettext("OK"));
            dialog.present(Some(&a.w.window));
        } else {
            super::run::open_report(&a);
        }
    });
    install_actions(&app);
    super::run::install(&app);
    demo(&app);
    super::sync(&app);
    window
}

fn obj(w: &impl IsA<glib::Object>) -> glib::Object {
    w.clone().upcast()
}

fn breakpoints(
    window: &adw::ApplicationWindow,
    split: &adw::OverlaySplitView,
    header: &adw::HeaderBar,
    flow_toggle: &gtk::ToggleButton,
    q: &queue::QueueView,
) {
    // Queue narrower than 860 pt (window < 1160): search moves below the tools.
    if let Ok(c) = adw::BreakpointCondition::parse("max-width: 1160sp") {
        let bp = adw::Breakpoint::new(c);
        bp.add_setters(&[
            (&obj(&q.search_inline), "visible", false.to_value()),
            (&obj(&q.search_below), "visible", true.to_value()),
        ]);
        window.add_breakpoint(bp);
    }
    // Queue narrower than 660 pt (window < 960): tabs and tools on two rows.
    if let Ok(c) = adw::BreakpointCondition::parse("max-width: 960sp") {
        let bp = adw::Breakpoint::new(c);
        bp.add_setters(&[
            (&obj(&q.search_inline), "visible", false.to_value()),
            (&obj(&q.search_below), "visible", true.to_value()),
            (
                &obj(&q.heading),
                "orientation",
                gtk::Orientation::Vertical.to_value(),
            ),
        ]);
        window.add_breakpoint(bp);
    }
    // Window below 900 pt: the workflow column is 264 pt and the queue is
    // under 600 pt, so the inspector replaces the photo list (macOS
    // `narrowInspectorExpanded`).
    if let Ok(c) = adw::BreakpointCondition::parse("max-width: 900sp") {
        let bp = adw::Breakpoint::new(c);
        bp.add_setters(&[
            (&obj(split), "min-sidebar-width", 264.0.to_value()),
            (&obj(split), "max-sidebar-width", 264.0.to_value()),
            (&obj(&q.search_inline), "visible", false.to_value()),
            (&obj(&q.search_below), "visible", true.to_value()),
            (
                &obj(&q.heading),
                "orientation",
                gtk::Orientation::Vertical.to_value(),
            ),
            (&obj(&q.inspector_split), "collapsed", true.to_value()),
            (
                &obj(&q.inspector_split),
                "max-sidebar-width",
                10000.0.to_value(),
            ),
            (
                &obj(&q.inspector_split),
                "sidebar-width-fraction",
                1.0.to_value(),
            ),
            (&obj(&q.inspector_back), "visible", true.to_value()),
        ]);
        window.add_breakpoint(bp);
    }
    // Phone width (narrower than any macOS window): the workflow column
    // overlays the queue and a header button shows it.
    if let Ok(c) = adw::BreakpointCondition::parse("max-width: 600sp") {
        let bp = adw::Breakpoint::new(c);
        bp.add_setters(&[
            (&obj(split), "collapsed", true.to_value()),
            (&obj(split), "min-sidebar-width", 264.0.to_value()),
            (&obj(split), "max-sidebar-width", 320.0.to_value()),
            (&obj(header), "show-end-title-buttons", true.to_value()),
            (&obj(flow_toggle), "visible", true.to_value()),
            (&obj(&q.search_inline), "visible", false.to_value()),
            (&obj(&q.search_below), "visible", true.to_value()),
            (
                &obj(&q.heading),
                "orientation",
                gtk::Orientation::Vertical.to_value(),
            ),
            (&obj(&q.inspector_split), "collapsed", true.to_value()),
            (
                &obj(&q.inspector_split),
                "max-sidebar-width",
                10000.0.to_value(),
            ),
            (
                &obj(&q.inspector_split),
                "sidebar-width-fraction",
                1.0.to_value(),
            ),
            (&obj(&q.inspector_back), "visible", true.to_value()),
        ]);
        window.add_breakpoint(bp);
    }
}

/// Header row, feedback, review hint and status bar for the current state.
pub(crate) fn sync(app: &Shared) {
    let c = &app.w.canvas;
    let flow = app.flow.get();
    let step = flow.step();
    c.title.set_label(&gettext(step.title()));
    c.detail.set_label(&gettext(step.detail()));
    c.detail.set_tooltip_text(Some(&gettext(step.detail())));
    let queue_visible = step != Step::Direction;
    c.stack
        .set_visible_child_name(if queue_visible { "queue" } else { "direction" });
    c.queue_bars.set_visible(queue_visible);
    c.status_bar.set_visible(queue_visible);

    // Task feedback (macOS `WorkspaceTaskFeedback`).
    let error = app.error.borrow().clone();
    let running = app.busy();
    let scanning = app.scanning.get();
    let last = app.last.borrow().clone();
    let failed = last.as_ref().map_or(0, |l| l.summary.failed);
    let (visible, icon, message) = if let Some(e) = &error {
        (true, "dialog-error-symbolic", e.clone())
    } else if running {
        (
            true,
            "view-refresh-symbolic",
            if app.stopping.get() {
                gettext("Stopping")
            } else {
                let p = app.progress.borrow().clone().unwrap_or_default();
                fill(
                    &gettext("Processed {done} / {total}"),
                    &[
                        ("done", &p.done.to_string()),
                        ("total", &p.total.to_string()),
                    ],
                )
            },
        )
    } else if scanning {
        (true, "view-refresh-symbolic", gettext("Recognizing photos"))
    } else if failed > 0 {
        (
            true,
            "dialog-warning-symbolic",
            gettext("Finished; some photos need attention"),
        )
    } else if let (Some(l), Step::Results) = (&last, step) {
        (
            true,
            if l.summary.cancelled {
                "media-playback-stop-symbolic"
            } else {
                "object-select-symbolic"
            },
            gettext(if l.summary.cancelled {
                "Stopped"
            } else {
                "Conversion Complete"
            }),
        )
    } else {
        (false, "dialog-information-symbolic", String::new())
    };
    c.feedback.set_reveal_child(visible);
    c.feedback_icon.set_icon_name(Some(icon));
    c.feedback_label.set_label(&message);
    c.feedback_label.set_tooltip_text(Some(&message));
    if error.is_some() {
        c.feedback_label.add_css_class("error");
    } else {
        c.feedback_label.remove_css_class("error");
    }
    let busy = running || scanning;
    c.feedback_progress.set_visible(busy);
    c.feedback_secondary.set_visible(busy);
    let progress = app.progress.borrow().clone();
    match (&progress, running) {
        (Some(p), true) if p.total > 0 => {
            c.feedback_progress.set_fraction(p.fraction());
            c.feedback_percent
                .set_label(&format!("{}%", (p.fraction() * 100.0).round() as i64));
            c.feedback_percent.set_visible(true);
            c.feedback_secondary.set_label(&p.current);
        }
        _ => {
            c.feedback_progress.pulse();
            c.feedback_percent.set_visible(false);
            c.feedback_secondary
                .set_label(&gettext(if app.stopping.get() {
                    "Finishing the current operation"
                } else if scanning {
                    "Recognizing photos"
                } else {
                    "Preparing photos"
                }));
        }
    }
    let button = if error.is_some() {
        Some(gettext("Show Reason"))
    } else if failed > 0 && !running {
        Some(gettext("View Report"))
    } else {
        None
    };
    c.feedback_button.set_visible(button.is_some());
    c.feedback_button.set_label(&button.unwrap_or_default());

    // Review hint (macOS `reviewHint`) in steps 4 and 5.
    let ready = app.with_plan(|p| p.ready().len());
    match step {
        Step::Preview => {
            c.review_hint.set_visible(true);
            c.review_icon.set_icon_name(Some("view-reveal-symbolic"));
            c.review_label.set_label(&fill(
                &gettext("{count} photos expected to run. Click a photo for output resources and reasons. Estimates do not guarantee success."),
                &[("count", &ready.to_string())],
            ));
        }
        Step::Results => {
            c.review_hint.set_visible(true);
            c.review_icon.set_icon_name(Some("task-due-symbolic"));
            c.review_label.set_label(&gettext(
                "File generation and unchanged exports are recorded separately. Click a photo for its actual result and reason.",
            ));
        }
        _ => c.review_hint.set_visible(false),
    }

    // Status bar (macOS `queueStatusBar`).
    let base = app.base();
    let queue = app.queue.borrow();
    let shown = queue.visible(base).len();
    let total = queue.base_count(base);
    let summary = match step {
        Step::Preview => fill(
            &gettext("Export estimate · {count} photos · Source thumbnails"),
            &[("count", &total.to_string())],
        ),
        Step::Results => fill(
            &gettext("Results · {count} photos · Source thumbnails"),
            &[("count", &total.to_string())],
        ),
        _ if !queue.has_explicit_selection() => gettext("Scope: all pending photos"),
        _ => {
            let mut s = fill(
                &gettext("{count} selected"),
                &[("count", &queue.selected_count().to_string())],
            );
            let hidden = queue.hidden_selected(base);
            if hidden > 0 {
                s.push_str(&fill(
                    &gettext(" · {count} selected items hidden"),
                    &[("count", &hidden.to_string())],
                ));
            }
            s
        }
    };
    c.selection_summary.set_label(&summary);
    c.selection_summary.set_tooltip_text(Some(&gettext(
        "Click a photo to inspect it. Use the check mark or Ctrl-click to include it. Filtering and preview do not change the processing scope",
    )));
    let reviews = matches!(step, Step::Preview | Step::Results);
    c.restore_scope
        .set_visible(!reviews && queue.has_explicit_selection());
    c.restore_scope.set_sensitive(!running);
    c.showing.set_label(&fill(
        &gettext("Showing {shown} / {total}"),
        &[("shown", &shown.to_string()), ("total", &total.to_string())],
    ));
    drop(queue);
    c.preview_export.set_visible(!reviews);
    let facts = app.facts();
    c.preview_export.set_sensitive(facts.can_review());
    c.preview_export.set_tooltip_text(Some(&gettext(if facts.can_review() {
        "Inspect ready, manual, and unsupported photos"
    } else if facts.items == 0 {
        "Add photos to preview the export"
    } else {
        "Preview becomes available after recognition finishes and at least one photo is selected"
    })));

    let enable = |name: &str, on: bool| {
        if let Some(action) = app
            .w
            .window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(on);
        }
    };
    let has_output = last.is_some() || app.output.borrow().is_some();
    enable("open-output", has_output && !running);
    enable(
        "open-report",
        last.as_ref().is_some_and(|l| l.summary.report.is_some()) && !running,
    );
    enable("review", facts.can_review());
    enable("restore-scope", !running);
}

fn install_actions(app: &Shared) {
    let window = &app.w.window;
    let add = |name: &str, f: Box<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| f());
        window.add_action(&action);
    };
    let a = app.clone();
    add(
        "add-folder",
        Box::new(move || super::sources::choose_folder(&a)),
    );
    let add_path = gio::SimpleAction::new("add-folder-path", Some(glib::VariantTy::STRING));
    let a = app.clone();
    add_path.connect_activate(move |_, arg| {
        if let Some(path) = arg.and_then(|v| v.get::<String>()) {
            super::sources::add_folder(&a, path.into());
        }
    });
    window.add_action(&add_path);
    let a = app.clone();
    add(
        "choose-output",
        Box::new(move || super::run::choose_output(&a, false)),
    );
    let a = app.clone();
    add("start", Box::new(move || super::flow::primary(&a)));
    let a = app.clone();
    add("stop", Box::new(move || super::run::stop(&a)));
    let a = app.clone();
    add(
        "preferences",
        Box::new(move || {
            let f = a.facts();
            a.update_flow(|flow| flow.open_settings(&f));
            super::sync(&a);
        }),
    );
    let a = app.clone();
    add(
        "review",
        Box::new(move || {
            let f = a.facts();
            a.update_flow(|flow| flow.open_review(&f));
            super::sync(&a);
        }),
    );
    let a = app.clone();
    add(
        "restore-scope",
        Box::new(move || {
            a.queue.borrow_mut().clear_selection();
            super::sync(&a);
        }),
    );
    let a = app.clone();
    add("open-output", Box::new(move || super::run::open_output(&a)));
    let a = app.clone();
    add("open-report", Box::new(move || super::run::open_report(&a)));
    let a = app.clone();
    add(
        "shortcuts",
        Box::new(move || shortcuts_window(&a.w.window).present()),
    );
    let a = app.clone();
    add("search", Box::new(move || super::queue::toggle_search(&a)));
    let a = app.clone();
    add(
        "inspector",
        Box::new(move || {
            let t = &a.w.queue.inspector_toggle;
            t.set_active(!t.is_active());
        }),
    );
    let a = app.clone();
    add(
        "select-shown",
        Box::new(move || {
            let base = a.base();
            a.queue.borrow_mut().select_visible(base);
            super::sync(&a);
        }),
    );
    let a = app.clone();
    add(
        "deselect-all",
        Box::new(move || {
            a.queue.borrow_mut().deselect_all();
            super::sync(&a);
        }),
    );
    let a = app.clone();
    add(
        "clear-filters",
        Box::new(move || super::queue::clear_filters(&a)),
    );
    let a = app.clone();
    add("rescan", Box::new(move || super::queue::scan(&a)));
    let a = app.clone();
    add("clear-sources", Box::new(move || super::sources::clear(&a)));
    let a = app.clone();
    add("burst-review", Box::new(move || super::burst::present(&a)));
    if let Some(gtk_app) = window.application() {
        gtk_app.set_accels_for_action("win.add-folder", &["<Control>o"]);
        gtk_app.set_accels_for_action("win.search", &["<Control>f"]);
        gtk_app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        gtk_app.set_accels_for_action("win.start", &["<Control>Return"]);
        gtk_app.set_accels_for_action("win.inspector", &["<Control><Alt>p"]);
        gtk_app.set_accels_for_action("win.shortcuts", &["<Control>question"]);
        gtk_app.set_accels_for_action("app.quit", &["<Control>q"]);
    }
}

/// Debug builds only: `MOMENTSTAYS_UI_DEMO=<step>[,<width>x<height>]` opens
/// the window at a step with the Apple route chosen, for UI screenshots
/// (the macOS app's `UI_DIAGNOSTICS` launch arguments).
fn demo(app: &Shared) {
    if !cfg!(debug_assertions) {
        return;
    }
    let Ok(spec) = std::env::var("MOMENTSTAYS_UI_DEMO") else {
        return;
    };
    app.target_confirmed.set(true);
    if let Some((w, h)) = spec.split(',').nth(1).and_then(|s| s.split_once('x')) {
        if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
            app.w.window.set_default_size(w, h);
        }
    }
    let step = match spec.split(',').next().unwrap_or("") {
        "1" => Step::Direction,
        "3" => Step::Features,
        "4" => Step::Preview,
        "5" => Step::Results,
        _ => Step::Import,
    };
    let a = app.clone();
    // Wait for the command-line folders to be scanned.
    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if a.scanning.get() {
            return glib::ControlFlow::Continue;
        }
        a.update_flow(|f| f.set_step(step));
        a.w.flow.live_expanded.set(step == Step::Features);
        if let Some(i) = a.queue.borrow().visible(Base::All).first() {
            a.w.queue.selection.set_selected(*i as u32);
        }
        super::sync(&a);
        glib::ControlFlow::Break
    });
}

fn shortcuts_window(parent: &adw::ApplicationWindow) -> gtk::ShortcutsWindow {
    let group = gtk::ShortcutsGroup::builder()
        .title(gettext("General"))
        .build();
    for (accel, title) in [
        ("<Control>o", gettext("Add Source Folder")),
        ("<Control>f", gettext("Search Photos")),
        ("<Control>Return", gettext("Next Step")),
        (
            "<Control><Alt>p",
            gettext("Show or Hide the Photo Inspector"),
        ),
        ("<Control>comma", gettext("Export Settings")),
        ("<Control>question", gettext("Keyboard Shortcuts")),
        ("<Control>q", gettext("Quit")),
    ] {
        group.add_shortcut(
            &gtk::ShortcutsShortcut::builder()
                .accelerator(accel)
                .title(title)
                .build(),
        );
    }
    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .build();
    section.add_group(&group);
    let window = gtk::ShortcutsWindow::builder()
        .transient_for(parent)
        .modal(true)
        .build();
    window.add_section(&section);
    window
}

/// The About dialog (`app.about`).
pub fn about(parent: &impl IsA<gtk::Widget>) {
    let dialog = adw::AboutDialog::builder()
        .application_name(gettext("MomentStays"))
        .application_icon(APP_ID)
        .developer_name("Lino")
        .version(VERSION)
        .website("https://momentstays.linofish.com")
        .issue_url("https://github.com/LinoFish/MomentStays/issues")
        .license_type(gtk::License::Gpl30Only)
        .copyright("© 2026 Lino")
        .comments(gettext(
            "Convert Android Motion Photos and Apple Live Photos, keeping the moment.",
        ))
        .translator_credits(gettext("translator-credits"))
        .build();
    dialog.present(Some(parent));
}

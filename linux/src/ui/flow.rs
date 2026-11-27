//! The workflow column (macOS `workflowSidebar`): "Workflow" header with
//! "Step n of 5", the five-step list, Back + primary action, the hint, and
//! the current step's panel in an `AdwNavigationView` (one page per step).

use super::direction::{preset_title, vendor_title};
use super::Shared;
use crate::i18n::{fill, gettext};
use crate::model::flow::{Advance, Step};
use crate::model::settings::{
    CandidateDelivery, Container, Direction, ExistingOutput, ExportMode, Preset, Unconverted,
    Vendor, VideoTrack,
};
use adw::prelude::*;
use gtk::glib;
use std::cell::Cell;
use std::rc::Rc;

pub struct FlowColumn {
    pub root: adw::ToolbarView,
    step_label: gtk::Label,
    steps: gtk::ListBox,
    rows: Vec<(Step, gtk::ListBoxRow, gtk::Label, gtk::Label, gtk::Label)>,
    back: gtk::Button,
    primary: gtk::Button,
    hint: gtk::Label,
    nav: adw::NavigationView,
    pages: Vec<(Step, adw::NavigationPage, gtk::Box)>,
    shown: Cell<Step>,
    pub live_expanded: Rc<Cell<bool>>,
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    for c in classes {
        l.add_css_class(c);
    }
    l
}

impl FlowColumn {
    pub fn new() -> FlowColumn {
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        let title = gtk::Label::new(Some(&gettext("Workflow")));
        title.add_css_class("heading");
        header.pack_start(&title);
        let step_label = gtk::Label::new(None);
        step_label.add_css_class("caption");
        step_label.add_css_class("dim-label");
        header.pack_end(&step_label);
        header.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));

        let steps = gtk::ListBox::new();
        steps.add_css_class("navigation-sidebar");
        steps.set_selection_mode(gtk::SelectionMode::Single);
        steps.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Complete workflow in five steps",
        ))]);
        let mut rows = Vec::new();
        for step in Step::ALL {
            let number = gtk::Label::new(Some(&step.number().to_string()));
            number.add_css_class("numeric");
            number.add_css_class("caption-heading");
            number.set_width_chars(2);
            let title = gtk::Label::builder()
                .label(gettext(step.title()))
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            let status = gtk::Label::new(None);
            status.add_css_class("caption");
            status.add_css_class("dim-label");
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 9);
            b.append(&number);
            b.append(&title);
            b.append(&status);
            let row = gtk::ListBoxRow::builder().child(&b).build();
            steps.append(&row);
            rows.push((step, row, number, title, status));
        }

        let back = gtk::Button::with_label(&gettext("Back"));
        back.set_tooltip_text(Some(&gettext(
            "Go back one step, keeping the current selection and settings",
        )));
        // The column is 264–300 pt: long localized titles ellipsize
        // instead of widening it (macOS `minimumScaleFactor`).
        let primary_label = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let primary = gtk::Button::builder().child(&primary_label).build();
        primary.set_hexpand(true);
        let commands = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        commands.append(&back);
        commands.append(&primary);
        let hint = label("", &["caption", "dim-label"]);
        hint.set_height_request(28);
        hint.set_valign(gtk::Align::Start);

        let top = gtk::Box::new(gtk::Orientation::Vertical, 10);
        top.set_margin_start(12);
        top.set_margin_end(12);
        top.set_margin_top(6);
        top.set_margin_bottom(12);
        top.append(&steps);
        top.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        top.append(&commands);
        top.append(&hint);

        let nav = adw::NavigationView::new();
        nav.set_pop_on_escape(false);
        nav.set_vexpand(true);
        let mut pages = Vec::new();
        for step in Step::ALL {
            let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let scroll = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&container)
                .build();
            let page = adw::NavigationPage::builder()
                .title(gettext(step.title()))
                .tag(format!("step-{}", step.number()))
                .child(&scroll)
                .can_pop(false)
                .build();
            pages.push((step, page, container));
        }
        nav.add(&pages[0].1);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&top);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&nav);
        let root = adw::ToolbarView::new();
        root.add_top_bar(&header);
        root.set_content(Some(&content));
        FlowColumn {
            root,
            step_label,
            steps,
            rows,
            back,
            primary,
            hint,
            nav,
            pages,
            shown: Cell::new(Step::Direction),
            live_expanded: Rc::default(),
        }
    }
}

pub fn connect(app: &Shared) {
    let a = app.clone();
    app.w.flow.steps.connect_row_activated(move |_, row| {
        if a.binding.get() {
            return;
        }
        let Some(step) = a.w.flow.rows.iter().find(|r| r.1 == *row).map(|r| r.0) else {
            return;
        };
        let f = a.facts();
        a.update_flow(|flow| {
            flow.visit(step, &f);
        });
        super::sync(&a);
    });
    let a = app.clone();
    app.w.flow.back.connect_clicked(move |_| {
        let f = a.facts();
        a.update_flow(|flow| {
            flow.back(&f);
        });
        super::sync(&a);
    });
    let a = app.clone();
    app.w.flow.primary.connect_clicked(move |_| primary(&a));
}

/// The primary button (macOS `advance`), or Stop while busy.
pub fn primary(app: &Shared) {
    let f = app.facts();
    if f.running {
        super::run::stop(app);
        return;
    }
    if f.scanning {
        super::queue::stop_scan(app);
        app.scanning.set(false);
        super::sync(app);
        return;
    }
    let mut flow = app.flow.get();
    let advance = flow.advance(&f);
    app.flow.set(flow);
    match advance {
        Some(Advance::ChooseOutput) => super::run::choose_output(app, false),
        Some(Advance::Start) => super::run::start(app),
        Some(Advance::Go(_)) | None => {}
    }
    super::sync(app);
}

/// Update the column for the current state.
pub fn sync(app: &Shared) {
    let w = &app.w.flow;
    let flow = app.flow.get();
    let f = app.facts();
    let step = flow.step();
    w.step_label.set_label(&fill(
        &gettext("Step {step} of {count}"),
        &[
            ("step", &step.number().to_string()),
            ("count", &Step::ALL.len().to_string()),
        ],
    ));
    app.binding.set(true);
    for (s, row, number, title, status) in &w.rows {
        let st = flow.status(*s, &f);
        status.set_label(&gettext(st.title()));
        row.set_sensitive(flow.can_visit(*s, &f));
        let current = *s == step;
        if current {
            w.steps.select_row(Some(row));
            number.add_css_class("accent");
            title.add_css_class("heading");
        } else {
            number.remove_css_class("accent");
            title.remove_css_class("heading");
        }
        row.set_tooltip_text(Some(&if flow.can_visit(*s, &f) {
            fill(
                &gettext("View {step}, keeping the current selection and settings"),
                &[("step", &gettext(s.title()))],
            )
        } else {
            fill(
                &gettext("Finish the earlier steps to open {step}"),
                &[("step", &gettext(s.title()))],
            )
        }));
    }
    app.binding.set(false);

    w.back.set_sensitive(flow.can_go_back());
    let p = flow.primary(&f);
    let title = gettext(p.title);
    let title = match p.count {
        Some(n) => fill(&title, &[("count", &n.to_string())]),
        None => title,
    };
    if let Some(l) = w.primary.child().and_downcast::<gtk::Label>() {
        l.set_label(&title);
    }
    w.primary.set_tooltip_text(Some(&title));
    w.primary.set_sensitive(p.enabled);
    if p.stop {
        w.primary.remove_css_class("suggested-action");
        w.primary.add_css_class("destructive-action");
    } else {
        w.primary.remove_css_class("destructive-action");
        w.primary.add_css_class("suggested-action");
    }
    let (hint, count) = flow.hint(&f, app.scanning.get());
    let hint = gettext(hint);
    w.hint.set_label(&match count {
        Some(n) => fill(&hint, &[("count", &n.to_string())]),
        None => hint,
    });

    navigate(app, step);
    let key = panel_key(app, step);
    if *app.panel_key.borrow() != key {
        *app.panel_key.borrow_mut() = key;
        rebuild(app, step);
    }
    if let Some((_, _, container)) = w.pages.iter().find(|p| p.0 == step) {
        container.set_sensitive(!f.running);
    }
    super::direction::sync(app);
}

/// Push or pop pages so the navigation stack is steps 1…`step`.
fn navigate(app: &Shared, step: Step) {
    let w = &app.w.flow;
    let shown = w.shown.get();
    if shown == step {
        return;
    }
    let page = |s: Step| w.pages.iter().find(|p| p.0 == s).map(|p| p.1.clone());
    if step > shown {
        for s in Step::ALL.iter().filter(|s| **s > shown && **s <= step) {
            if let Some(p) = page(*s) {
                w.nav.push(&p);
            }
        }
    } else if let Some(p) = page(step) {
        w.nav.pop_to_page(&p);
    }
    w.shown.set(step);
}

fn panel_key(app: &Shared, step: Step) -> String {
    let s = app.settings.borrow();
    let running = app.busy();
    match step {
        Step::Direction => format!(
            "1|{}|{:?}|{}",
            app.target_confirmed.get(),
            s.preset(),
            app.sources.borrow().paths().is_empty()
        ),
        Step::Import => format!(
            "2|{:?}|{}|{}|{:?}|{}|{}",
            app.sources.borrow().folders(),
            app.scanning.get(),
            app.queue.borrow().len(),
            app.queue.borrow().source(),
            running,
            app.target_confirmed.get()
        ),
        Step::Features => {
            let avail = app.with_plan(|p| {
                (
                    p.available_live(),
                    p.available_burst(),
                    p.available_hdr(),
                    p.available_time(),
                    p.available_metadata_time(),
                )
            });
            format!(
                "3|{:?}|{:?}|{:?}|{:?}|{}|{}|{:?}|{:?}|{:?}|{}|{:?}|{}|{}|{}|{}|{}|{}|{}",
                avail,
                s.direction,
                s.vendor,
                s.effective_container(),
                s.live_conversion,
                s.burst_conversion,
                s.export_mode,
                s.candidate_delivery,
                s.track,
                s.cover_selections.is_empty(),
                app.unconverted.get(),
                s.time_options_enabled,
                s.photo_time_repair,
                s.photo_time_synthetic,
                s.exif_file_times,
                s.legacy_hdr,
                s.preserve_jpeg,
                running
            )
        }
        Step::Preview => format!(
            "4|{:?}|{:?}|{:?}|{:?}|{}|{}",
            app.output.borrow(),
            app.unconverted.get(),
            app.unconverted_output.borrow(),
            s.existing_output,
            s.export_data_files,
            running
        ),
        Step::Results => format!(
            "5|{}|{}|{:?}|{:?}|{}",
            running,
            app.stopping.get(),
            app.last
                .borrow()
                .as_ref()
                .map(|l| (&l.summary, l.originals)),
            app.progress.borrow(),
            app.output.borrow().is_some()
        ),
    }
}

fn rebuild(app: &Shared, step: Step) {
    let Some((_, _, container)) = app.w.flow.pages.iter().find(|p| p.0 == step) else {
        return;
    };
    while let Some(c) = container.first_child() {
        container.remove(&c);
    }
    let panel = match step {
        Step::Direction => direction_panel(app),
        Step::Import => sources_panel(app),
        Step::Features => features_panel(app),
        Step::Preview => delivery_panel(app),
        Step::Results => results_panel(app),
    };
    container.append(&panel);
}

/// Re-sync after a panel control changed, outside its signal handler
/// (the panel may be rebuilt).
fn later(app: &Shared) {
    let a = app.clone();
    glib::idle_add_local_once(move || super::sync(&a));
}

fn panel_box() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 12);
    b.set_margin_start(12);
    b.set_margin_end(12);
    b.set_margin_top(12);
    b.set_margin_bottom(12);
    b
}

// ---- Step 1 -------------------------------------------------------------

fn direction_panel(app: &Shared) -> gtk::Widget {
    let b = panel_box();
    let confirmed = app.target_confirmed.get();
    b.append(&label(
        &gettext(if confirmed {
            "Selected Direction"
        } else {
            "Choose a conversion direction first"
        }),
        &["heading"],
    ));
    if confirmed {
        let preset = app.settings.borrow().preset();
        b.append(&label(&preset_title(preset), &["heading"]));
        b.append(&label(
            &import_scope(app.settings.borrow().direction),
            &["dim-label"],
        ));
    } else {
        b.append(&label(
            &gettext("Choose a source and target on the left. Import options depend on this direction; choose processing features after importing."),
            &["dim-label"],
        ));
    }
    if !app.sources.borrow().paths().is_empty() {
        b.append(&label(
            &gettext("Changing direction reuses valid inspection data and rechecks eligibility. Sources and selections are retained; incompatible items show a reason."),
            &["caption", "dim-label"],
        ));
    }
    b.upcast()
}

/// macOS `importScopeDescription`, without Photos library sources.
fn import_scope(d: Direction) -> String {
    gettext(match d {
        Direction::ToApple => "Local JPEG/HEIC motion photos, supported HDR, and bursts. Ordinary stills can be processed for dates only or exported unchanged in the next step.",
        Direction::ToAndroid => "Import complete Apple Live Photos (image and adjacent MOV). Static HDR follows the supported processing path.",
        Direction::AndroidToAndroid => "Verified Google Motion Photo JPEG → HONOR only, from local files. HEIC is unsupported.",
        Direction::HonorToGoogle => "Recognized HONOR Motion Photo JPEG → Google only, from local files. HEIC is unsupported.",
    })
}

// ---- Step 2 -------------------------------------------------------------

fn sources_panel(app: &Shared) -> gtk::Widget {
    let b = panel_box();
    b.set_spacing(10);
    let folders = app.sources.borrow().folders().to_vec();
    let busy = app.busy();

    let head = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let title = label(&gettext("Sources"), &["heading"]);
    title.set_hexpand(true);
    head.append(&title);
    let menu = gtk::gio::Menu::new();
    menu.append(Some(&gettext("Rescan Sources")), Some("win.rescan"));
    menu.append(
        Some(&gettext("Clear Sources and List")),
        Some("win.clear-sources"),
    );
    let more = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&menu)
        .tooltip_text(gettext("Rescan or clear sources"))
        .css_classes(["flat"])
        .sensitive(!folders.is_empty() && !busy)
        .build();
    more.update_property(&[gtk::accessible::Property::Label(&gettext("Source Actions"))]);
    head.append(&more);
    b.append(&head);

    let add = gtk::Button::with_label(&gettext(if folders.is_empty() {
        "Add Photos"
    } else {
        "Add Sources…"
    }));
    add.set_action_name(Some("win.add-folder"));
    if folders.is_empty() {
        add.add_css_class("suggested-action");
    }
    add.set_sensitive(app.target_confirmed.get() && !busy);
    add.set_tooltip_text(Some(
        &folders
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n"),
    ));
    b.append(&add);
    if folders.is_empty() {
        b.append(&label(
            &gettext("Add folders of photos; subfolders are included."),
            &["caption", "dim-label"],
        ));
        return b.upcast();
    }
    let clear = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("user-trash-symbolic")
                .label(gettext("Clear All Sources"))
                .build(),
        )
        .halign(gtk::Align::Start)
        .tooltip_text(gettext(
            "Clear this run's list only; never delete source files",
        ))
        .sensitive(!busy)
        .build();
    clear.add_css_class("flat");
    clear.add_css_class("destructive-action");
    clear.set_action_name(Some("win.clear-sources"));
    b.append(&clear);
    b.append(&label(
        &fill(
            &gettext("{sources} sources · {photos} photos"),
            &[
                ("sources", &folders.len().to_string()),
                ("photos", &app.queue.borrow().len().to_string()),
            ],
        ),
        &["caption", "dim-label"],
    ));

    let list = gtk::ListBox::new();
    list.add_css_class("navigation-sidebar");
    let all = adw::ActionRow::builder()
        .title(gettext("All Sources"))
        .activatable(true)
        .build();
    all.add_prefix(&gtk::Image::from_icon_name("view-grid-symbolic"));
    list.append(&all);
    let current = app.queue.borrow().source().map(|p| p.to_path_buf());
    if current.is_none() {
        list.select_row(Some(&all));
    }
    let mut paths = vec![None];
    for folder in &folders {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&super::sources::display_name(
                &folder.path,
            )))
            .activatable(true)
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
        monitor
            .connect_active_notify(move |s| super::sources::set_monitoring(&a, &p, s.is_active()));
        row.add_suffix(&monitor);
        let remove = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Remove Folder"))
            .css_classes(["flat", "circular"])
            .sensitive(!busy)
            .build();
        let a = app.clone();
        let p = folder.path.clone();
        remove.connect_clicked(move |_| {
            let a = a.clone();
            let p = p.clone();
            glib::idle_add_local_once(move || super::sources::remove_folder(&a, &p));
        });
        row.add_suffix(&remove);
        list.append(&row);
        if current.as_deref() == Some(folder.path.as_path()) {
            list.select_row(Some(&row));
        }
        paths.push(Some(folder.path.clone()));
    }
    let a = app.clone();
    list.connect_row_activated(move |_, row| {
        let source = paths.get(row.index() as usize).cloned().flatten();
        a.queue.borrow_mut().set_source(source);
        later(&a);
    });
    b.append(&list);
    b.upcast()
}

// ---- Step 3 -------------------------------------------------------------

fn note_row(text: &str) -> adw::ActionRow {
    adw::ActionRow::builder()
        .subtitle(glib::markup_escape_text(text))
        .subtitle_lines(0)
        .build()
}

fn switch_row(
    app: &Shared,
    title: &str,
    subtitle: Option<&str>,
    active: bool,
    set: impl Fn(&Shared, bool) + 'static,
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .active(active)
        .title_lines(0)
        .build();
    if let Some(s) = subtitle {
        row.set_subtitle(&glib::markup_escape_text(s));
        row.set_subtitle_lines(0);
    }
    let a = app.clone();
    row.connect_active_notify(move |r| {
        set(&a, r.is_active());
        later(&a);
    });
    row
}

fn combo_row<T: Copy + PartialEq + 'static>(
    app: &Shared,
    title: &str,
    items: &[(T, String)],
    current: T,
    set: impl Fn(&Shared, T) + 'static,
) -> adw::ComboRow {
    let names: Vec<&str> = items.iter().map(|(_, n)| n.as_str()).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&names))
        .build();
    row.set_selected(items.iter().position(|(v, _)| *v == current).unwrap_or(0) as u32);
    let values: Vec<T> = items.iter().map(|(v, _)| *v).collect();
    let a = app.clone();
    row.connect_selected_notify(move |r| {
        if let Some(v) = values.get(r.selected() as usize) {
            set(&a, *v);
            later(&a);
        }
    });
    row
}

fn group(title: Option<&str>) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    if let Some(t) = title {
        g.set_title(&glib::markup_escape_text(t));
    }
    g
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn features_panel(app: &Shared) -> gtk::Widget {
    let b = panel_box();
    b.set_spacing(18);
    let (live, burst, hdr, time, metadata_time) = app.with_plan(|p| {
        (
            p.available_live(),
            p.available_burst(),
            p.available_hdr(),
            p.available_time(),
            p.available_metadata_time(),
        )
    });
    let s = app.settings.borrow().clone();

    if live {
        let g = group(None);
        g.add(&switch_row(
            app,
            &gettext("Live Photo"),
            None,
            s.live_conversion,
            |a, v| {
                a.settings.borrow_mut().live_conversion = v;
            },
        ));
        if s.live_conversion {
            let exp = adw::ExpanderRow::builder()
                .title(gettext("Format, Video, and Cover Options"))
                .expanded(app.w.flow.live_expanded.get())
                .build();
            let keep = app.w.flow.live_expanded.clone();
            exp.connect_expanded_notify(move |e| keep.set(e.is_expanded()));
            match s.direction {
                Direction::ToApple => apple_options(app, &s, &exp),
                Direction::ToAndroid => android_options(app, &s, &exp),
                Direction::AndroidToAndroid | Direction::HonorToGoogle => {
                    exp.add_row(&note_row(&gettext(match s.direction {
                        Direction::AndroidToAndroid => "Google → HONOR supports the verified Google Motion Photo JPEG route only, not HEIC. The only Android transfer routes are this one and HONOR → Google.",
                        _ => "HONOR → Google supports recognized HONOR Motion Photo JPEG only. Static HDR is handled as an independent image, subject to inspection. HEIC is unsupported.",
                    })));
                }
            }
            g.add(&exp);
        }
        b.append(&g);
    }
    if burst {
        let g = group(None);
        g.add(&switch_row(
            app,
            &gettext("Burst"),
            None,
            s.burst_conversion,
            |a, v| {
                a.settings.borrow_mut().burst_conversion = v;
            },
        ));
        if s.burst_conversion {
            let review = adw::ButtonRow::builder()
                .title(gettext("Choose Burst Members and Representative…"))
                .action_name("win.burst-review")
                .build();
            g.add(&review);
        }
        b.append(&g);
    }
    {
        let g = group(None);
        let on = app.unconverted.get() != Unconverted::None;
        g.add(&switch_row(
            app,
            &gettext("Export Unchanged Photos"),
            None,
            on,
            |a, v| {
                a.unconverted.set(if v {
                    a.unconverted_location.get()
                } else {
                    Unconverted::None
                });
            },
        ));
        if on {
            g.add(&note_row(&gettext("Export complete, unchanged source resources. Choose the location in Export Preview.")));
        }
        b.append(&g);
    }
    if time {
        let g = group(None);
        let exp = adw::ExpanderRow::builder()
            .title(gettext("Photo Dates"))
            .subtitle(glib::markup_escape_text(&gettext("Enable only when photo dates appear out of order in Photos. Source files are not modified.")))
            .show_enable_switch(true)
            .enable_expansion(s.time_options_enabled)
            .expanded(s.time_options_enabled)
            .build();
        let a = app.clone();
        exp.connect_enable_expansion_notify(move |e| {
            let metadata = a.with_plan(|p| p.available_metadata_time());
            a.settings
                .borrow_mut()
                .set_time_options(e.enables_expansion(), metadata, now());
            later(&a);
        });
        if metadata_time {
            let repair = switch_row(
                app,
                &gettext("Fill missing capture dates (PhotoTime)"),
                Some(&gettext("Recognize a date or Unix timestamp from the original filename. Fill missing dates only in output copies; existing nonempty dates stay unchanged.")),
                s.photo_time_repair,
                |a, v| a.settings.borrow_mut().set_photo_time_repair(v, now()),
            );
            repair.set_sensitive(
                !(live
                    && s.live_conversion
                    && s.direction == Direction::ToApple
                    && s.export_mode == ExportMode::AuxiliaryOnly),
            );
            exp.add_row(&repair);
        }
        if s.photo_time_repair {
            exp.add_row(&switch_row(
                app,
                &gettext("Generate synthetic ordering dates when recognition fails"),
                Some(&format!(
                    "{}{}",
                    gettext("Synthetic dates are not actual capture dates. They follow natural filename order at one-second intervals. Time zone: "),
                    s.photo_time_zone
                )),
                s.photo_time_synthetic,
                |a, v| a.settings.borrow_mut().photo_time_synthetic = v,
            ));
            exp.add_row(&note_row(&gettext("If a motion container or HDR resource cannot be fully verified, it is not published or split into a still; see Export Preview for the reason.")));
        }
        exp.add_row(&switch_row(
            app,
            &gettext("Set output file dates from EXIF"),
            Some(&gettext("Independently set creation and modification dates; off by default. After repair, read actual EXIF dates. Without usable EXIF, keep source file dates.")),
            s.exif_file_times,
            |a, v| a.settings.borrow_mut().exif_file_times = v,
        ));
        g.add(&exp);
        b.append(&g);
    }
    if hdr {
        let g = group(None);
        g.add(&switch_row(
            app,
            &gettext("HDR (experimental, off by default)"),
            s.legacy_hdr.then(|| gettext("Only supported HDR profiles are attempted. Unsupported items are listed in preview; the original device's HDR appearance is not guaranteed.")).as_deref(),
            s.legacy_hdr,
            |a, v| a.settings.borrow_mut().legacy_hdr = v,
        ));
        b.append(&g);
    }
    b.upcast()
}

fn mode_title(m: ExportMode) -> String {
    gettext(match m {
        ExportMode::LivePhoto => "Live Photo",
        ExportMode::WithAuxiliary => "Live Photo + Auxiliary Video",
        ExportMode::AuxiliaryOnly => "Auxiliary Video Only",
        ExportMode::WithCandidates => "Live Photo + High-resolution Covers",
    })
}

fn apple_options(app: &Shared, s: &crate::model::settings::ExportSettings, exp: &adw::ExpanderRow) {
    let modes: Vec<_> = ExportMode::ALL
        .iter()
        .map(|m| (*m, mode_title(*m)))
        .collect();
    exp.add_row(&combo_row(
        app,
        &gettext("Output content"),
        &modes,
        s.export_mode,
        |a, v| {
            a.settings.borrow_mut().export_mode = v;
        },
    ));
    if s.export_mode == ExportMode::WithCandidates {
        let items = [
            (
                CandidateDelivery::StaticPhotos,
                gettext("Main photo live, other covers still"),
            ),
            (
                CandidateDelivery::LivePhotos,
                gettext("Export Every Cover as a Live Photo"),
            ),
        ];
        exp.add_row(&combo_row(
            app,
            &gettext("High-resolution Cover Output"),
            &items,
            s.candidate_delivery,
            |a, v| {
                a.settings.borrow_mut().candidate_delivery = v;
            },
        ));
    }
    let tracks: Vec<_> = VideoTrack::ALL
        .iter()
        .map(|t| {
            (
                *t,
                gettext(match t {
                    VideoTrack::Primary => "Motion Video",
                    VideoTrack::Secondary => "Auxiliary Video",
                }),
            )
        })
        .collect();
    let track = combo_row(
        app,
        &gettext("Video Source"),
        &tracks,
        s.effective_track(),
        |a, v| {
            a.settings.borrow_mut().track = v;
        },
    );
    track.set_sensitive(
        s.export_mode != ExportMode::AuxiliaryOnly
            && s.export_mode != ExportMode::WithCandidates
            && s.cover_selections.is_empty(),
    );
    exp.add_row(&track);
    if !s.cover_selections.is_empty() {
        exp.add_row(&note_row(&gettext(
            "A high-resolution cover is selected; the motion video is required.",
        )));
    }
    let note = match s.export_mode {
        ExportMode::AuxiliaryOnly => Some("Export only the silent auxiliary video, without creating a Live Photo."),
        _ if s.track == VideoTrack::Secondary => Some("The auxiliary video may contain only one frame and cannot replace the motion video."),
        ExportMode::WithAuxiliary => Some("The auxiliary video is saved separately as .auxiliary.mov and is not part of the Live Photo pair."),
        ExportMode::WithCandidates => Some(if s.candidate_delivery == CandidateDelivery::LivePhotos {
            "Each high-resolution cover becomes a separate Live Photo, using more space."
        } else {
            "The main photo stays live; other high-resolution covers are saved as still photos."
        }),
        ExportMode::LivePhoto => None,
    };
    if let Some(n) = note {
        exp.add_row(&note_row(&gettext(n)));
    }
    if s.export_mode != ExportMode::AuxiliaryOnly {
        exp.add_row(&note_row(&gettext("Use Choose Cover beside a photo to select a high-resolution candidate. New covers are JPEG and do not preserve the original cover's HDR.")));
    }
}

fn android_options(
    app: &Shared,
    s: &crate::model::settings::ExportSettings,
    exp: &adw::ExpanderRow,
) {
    let target = adw::ActionRow::builder()
        .title(gettext("Target Format"))
        .subtitle(glib::markup_escape_text(&preset_title(Preset::Android(
            s.vendor,
        ))))
        .build();
    target.add_css_class("property");
    exp.add_row(&target);
    exp.add_row(&note_row(&gettext(
        "Return to step 1, Conversion Direction, to change the route or target brand.",
    )));
    if matches!(s.vendor, Vendor::Google | Vendor::Honor) {
        exp.add_row(&note_row(&gettext(
            "Google / General is recommended for newer HONOR devices. HONOR format is a fallback.",
        )));
        exp.add_row(&note_row(&gettext("Newer HONOR devices have been verified with the Google format exported by MomentStays. This does not imply support for Google Camera originals.")));
    }
    let mut formats = vec![(Container::Jpeg, "JPEG".to_string())];
    if s.vendor.supports_heic() {
        formats.push((Container::Heic, "HEIC".to_string()));
    }
    exp.add_row(&combo_row(
        app,
        &gettext("Photo Format"),
        &formats,
        s.effective_container(),
        |a, v| {
            a.settings.borrow_mut().container = v;
        },
    ));
    if !s.vendor.supports_heic() {
        exp.add_row(&note_row(&gettext("This vendor supports JPEG only.")));
    }
    let _ = vendor_title(s.vendor);
    if s.effective_container() == Container::Jpeg {
        exp.add_row(&switch_row(
            app,
            &gettext("Preserve original JPEG encoding"),
            None,
            s.preserve_jpeg,
            |a, v| {
                a.settings.borrow_mut().preserve_jpeg = v;
            },
        ));
        for (title, value, gain) in [
            (gettext("Main image quality"), s.image_quality, false),
            (gettext("Gain-map quality"), s.gain_quality, true),
        ] {
            let row = adw::SpinRow::with_range(1.0, 100.0, 1.0);
            row.set_title(&title);
            row.set_value((value * 100.0).round());
            row.set_subtitle("%");
            let a = app.clone();
            row.connect_value_notify(move |r| {
                let v = r.value() / 100.0;
                let mut s = a.settings.borrow_mut();
                if gain {
                    s.gain_quality = v;
                } else {
                    s.image_quality = v;
                }
            });
            exp.add_row(&row);
        }
        exp.add_row(&note_row(&gettext("JPEG preservation avoids recompression. HEIC-to-JPEG uses main image quality; gain-map quality applies only when re-encoding.")));
        exp.add_row(&note_row(&gettext(
            "HDR uses Ultra HDR. SDR stays unchanged.",
        )));
    } else {
        let row = adw::ActionRow::builder()
            .title(gettext("Preserve original HEIC encoding"))
            .build();
        row.add_prefix(&gtk::Image::from_icon_name("security-high-symbolic"));
        exp.add_row(&row);
        exp.add_row(&note_row(&gettext("No quality adjustment is needed. Check compatibility and HDR display on the target phone.")));
    }
}

// ---- Step 4 -------------------------------------------------------------

/// A folder and, below it, its Choose Folder… row (fits the 264 pt column).
fn folder_rows(
    g: &adw::PreferencesGroup,
    title: &str,
    path: Option<&std::path::Path>,
    empty: &str,
    action: &str,
) {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(glib::markup_escape_text(
            &path
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| empty.to_string()),
        ))
        .subtitle_selectable(true)
        .subtitle_lines(0)
        .build();
    row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
    g.add(&row);
    g.add(
        &adw::ButtonRow::builder()
            .title(gettext("Choose Folder…"))
            .action_name(action)
            .build(),
    );
}

fn delivery_panel(app: &Shared) -> gtk::Widget {
    let b = panel_box();
    b.set_spacing(18);
    let g = group(Some(&gettext("Output Location")));
    let output = app.output.borrow().clone();
    folder_rows(
        &g,
        &gettext("Folder"),
        output.as_deref(),
        &gettext("Not selected"),
        "win.choose-output",
    );
    let unconverted = app.unconverted.get();
    if unconverted != Unconverted::None {
        let items = [
            (
                Unconverted::SameLocation,
                gettext("Export to the Same Location"),
            ),
            (
                Unconverted::SeparateLocation,
                gettext("Export to a Different Location"),
            ),
        ];
        g.add(&combo_row(
            app,
            &gettext("Unchanged-export location"),
            &items,
            unconverted,
            |a, v| {
                a.unconverted_location.set(v);
                a.unconverted.set(v);
            },
        ));
        if unconverted == Unconverted::SeparateLocation {
            let separate = app.unconverted_output.borrow().clone();
            folder_rows(
                &g,
                &gettext("Export Unchanged"),
                separate.as_deref(),
                &gettext("Choose a different folder for unchanged resources"),
                "win.choose-unconverted-output",
            );
        } else {
            g.add(&note_row(&gettext("Use this run's output location, with a separate folder for original resources. Sources are not overwritten, and unchanged exports do not count as conversions.")));
        }
    }
    b.append(&g);

    let g = group(Some(&gettext("When a Result Exists")));
    let existing: Vec<_> = ExistingOutput::ALL
        .iter()
        .map(|e| {
            (
                *e,
                gettext(match e {
                    ExistingOutput::Resume => "Verify and Reuse",
                    ExistingOutput::Skip => "Skip",
                    ExistingOutput::Overwrite => "Replace",
                }),
            )
        })
        .collect();
    let s = app.settings.borrow().clone();
    let row = combo_row(
        app,
        &gettext("Existing Output"),
        &existing,
        s.existing_output,
        |a, v| {
            a.settings.borrow_mut().existing_output = v;
        },
    );
    row.set_subtitle(&glib::markup_escape_text(&gettext("Verify and Reuse checks sources, scope, settings, and output files, and continues without reprocessing when they match.")));
    row.set_subtitle_lines(0);
    g.add(&row);
    g.add(&switch_row(
        app,
        &gettext("Write Report Files"),
        Some(&gettext(
            "Save the manifest and JSON report next to the results.",
        )),
        s.export_data_files,
        |a, v| a.settings.borrow_mut().export_data_files = v,
    ));
    b.append(&g);
    b.upcast()
}

// ---- Step 5 -------------------------------------------------------------

fn results_panel(app: &Shared) -> gtk::Widget {
    let b = panel_box();
    let running = app.busy();
    b.append(&label(
        &gettext(if running {
            "Running"
        } else {
            "This Run's Results"
        }),
        &["heading"],
    ));
    let last = app.last.borrow().clone();
    let status = if running {
        let p = app.progress.borrow().clone().unwrap_or_default();
        fill(
            &gettext("Processed {done} / {total}"),
            &[
                ("done", &p.done.to_string()),
                ("total", &p.total.to_string()),
            ],
        )
    } else if let Some(l) = &last {
        gettext(if l.summary.cancelled {
            "Stopped"
        } else if l.summary.ok() {
            "Conversion Complete"
        } else {
            "Task incomplete"
        })
    } else {
        String::new()
    };
    b.append(&label(&status, &[]));
    if let Some(l) = &last {
        let s = &l.summary;
        b.append(&label(
            &fill(
                &gettext("Converted {converted} · Unchanged {copied} · Existing {resumed} · Skipped {skipped} · Failed {failed}"),
                &[
                    ("converted", &s.converted.to_string()),
                    ("copied", &(s.copied + l.originals.map_or(0, |o| o[0])).to_string()),
                    ("resumed", &(s.resumed + l.originals.map_or(0, |o| o[1])).to_string()),
                    ("skipped", &(s.skipped + l.originals.map_or(0, |o| o[2])).to_string()),
                    ("failed", &(s.failed + l.originals.map_or(0, |o| o[3])).to_string()),
                ],
            ),
            &["caption", "dim-label"],
        ));
        if let Some(o) = l.originals {
            b.append(&label(
                &fill(
                    &gettext("Unchanged export: new {copied} · reused {resumed} · skipped {skipped} · failed {failed}"),
                    &[
                        ("copied", &o[0].to_string()),
                        ("resumed", &o[1].to_string()),
                        ("skipped", &o[2].to_string()),
                        ("failed", &o[3].to_string()),
                    ],
                ),
                &["caption", "dim-label"],
            ));
        }
    }
    if !running && (last.is_some() || app.output.borrow().is_some()) {
        let open = gtk::Button::with_label(&gettext("Open Output Folder"));
        open.set_halign(gtk::Align::Start);
        open.set_action_name(Some("win.open-output"));
        b.append(&open);
    }
    if !running && last.as_ref().is_some_and(|l| l.summary.report.is_some()) {
        let report = gtk::Button::with_label(&gettext("View Full Report"));
        report.set_halign(gtk::Align::Start);
        report.set_action_name(Some("win.open-report"));
        b.append(&report);
    }
    b.append(&label(
        &gettext("File generation and unchanged exports are recorded separately; the report in the output folder lists every photo."),
        &["caption", "dim-label"],
    ));
    b.upcast()
}

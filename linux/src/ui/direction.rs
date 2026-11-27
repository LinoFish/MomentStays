//! Step 1 canvas (macOS `directionCanvas`): verified routes as cards.

use super::Shared;
use crate::i18n::gettext;
use crate::model::claims;
use crate::model::flow::Step;
use crate::model::settings::{Preset, Vendor};
use adw::prelude::*;

pub struct DirectionCanvas {
    pub root: gtk::ScrolledWindow,
    cards: Vec<(Preset, gtk::Button, gtk::Image, gtk::Image)>,
}

pub fn card_title(p: Preset) -> String {
    match p {
        Preset::Apple => gettext("Apple photos"),
        Preset::Android(Vendor::Google) => gettext("Google / General"),
        Preset::Android(v) => vendor_title(v),
        Preset::GoogleToHonor => gettext("Google → HONOR"),
        Preset::HonorToGoogle => gettext("HONOR → Google"),
    }
}

pub fn vendor_title(v: Vendor) -> String {
    match v {
        Vendor::Google => gettext("Google / General"),
        Vendor::Xiaomi => gettext("Xiaomi"),
        Vendor::Samsung => gettext("Samsung"),
        Vendor::Huawei => gettext("Huawei"),
        Vendor::Honor => gettext("HONOR (Fallback)"),
        Vendor::Oppo => "OPPO".into(),
        Vendor::Vivo => "vivo V2".into(),
    }
}

/// macOS `MigrationTargetPreset.title`.
pub fn preset_title(p: Preset) -> String {
    match p {
        Preset::Apple => gettext("Supported photos → Apple (Live / HDR / Burst)"),
        Preset::Android(v) => format!("{}{}", gettext("Apple Live Photos → "), vendor_title(v)),
        Preset::GoogleToHonor => gettext("Google → HONOR (JPEG)"),
        Preset::HonorToGoogle => gettext("HONOR → Google (JPEG)"),
    }
}

fn support_summary(p: Preset) -> String {
    match p {
        Preset::Apple => gettext(claims::APPLE_ROUTE),
        Preset::GoogleToHonor => gettext("Verified Google Motion Photo JPEG → HONOR only. HEIC, Photos library sources, and bursts are unsupported."),
        Preset::HonorToGoogle => gettext("Recognized HONOR Motion Photo JPEG → Google only. HEIC, Photos library sources, and bursts are unsupported."),
        Preset::Android(v) => format!(
            "{}{}{}",
            gettext("Apple Live Photos · "),
            if v.supports_heic() { "JPEG / HEIC" } else { "JPEG" },
            gettext(claims::ANDROID_ROUTE_TAIL)
        ),
    }
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .build();
    for c in classes {
        l.add_css_class(c);
    }
    l
}

impl DirectionCanvas {
    pub fn new() -> DirectionCanvas {
        let content = gtk::Box::new(gtk::Orientation::Vertical, 22);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        let notes = gtk::Box::new(gtk::Orientation::Vertical, 12);
        notes.append(&label(&gettext(claims::READ_ONLY_NOTE), &["dim-label"]));
        notes.append(&label(
            &gettext(claims::ROUTES_NOTE),
            &["caption", "dim-label"],
        ));
        content.append(&notes);
        let mut cards = Vec::new();
        for (title, presets) in [
            (gettext("Android to Apple"), &Preset::ALL[..1]),
            (gettext("Apple to Android"), &Preset::ALL[1..8]),
            (gettext("Android to Android"), &Preset::ALL[8..]),
        ] {
            let group = gtk::Box::new(gtk::Orientation::Vertical, 10);
            group.append(&label(&title, &["heading"]));
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .halign(gtk::Align::Start)
                .homogeneous(true)
                .min_children_per_line(1)
                .max_children_per_line(4)
                .column_spacing(12)
                .row_spacing(12)
                .build();
            for preset in presets {
                let icon = gtk::Image::from_icon_name(if *preset == Preset::Apple {
                    "image-x-generic-symbolic"
                } else {
                    "phone-symbolic"
                });
                icon.add_css_class("accent");
                icon.set_valign(gtk::Align::Start);
                let text = gtk::Box::new(gtk::Orientation::Vertical, 5);
                text.set_hexpand(true);
                text.append(&label(&card_title(*preset), &["heading"]));
                let summary = label(&support_summary(*preset), &["caption", "dim-label"]);
                summary.set_max_width_chars(28);
                summary.set_width_chars(22);
                text.append(&summary);
                let mark = gtk::Image::from_icon_name("go-next-symbolic");
                mark.set_valign(gtk::Align::Start);
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
                row.set_margin_start(14);
                row.set_margin_end(14);
                row.set_margin_top(14);
                row.set_margin_bottom(14);
                row.append(&icon);
                row.append(&text);
                row.append(&mark);
                let button = gtk::Button::builder()
                    .child(&row)
                    .width_request(210)
                    .tooltip_text(preset_title(*preset))
                    .build();
                button.add_css_class("card");
                button.update_property(&[gtk::accessible::Property::Label(&preset_title(*preset))]);
                flow.append(&button);
                cards.push((*preset, button, mark, icon));
            }
            group.append(&flow);
            content.append(&group);
        }
        let root = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&content)
            .build();
        root.add_css_class("view");
        DirectionCanvas { root, cards }
    }
}

pub fn connect(app: &Shared) {
    for (preset, button, _, _) in &app.w.direction.cards {
        let a = app.clone();
        let preset = *preset;
        button.connect_clicked(move |_| select(&a, preset));
    }
}

/// macOS `selectMigrationTarget` + `workspaceStep = .importPhotos`.
fn select(app: &Shared, preset: Preset) {
    if app.busy() {
        return;
    }
    let before = app.settings.borrow().direction;
    let was_confirmed = app.target_confirmed.get();
    app.target_confirmed.set(true);
    app.settings.borrow_mut().apply_preset(preset);
    let direction = app.settings.borrow().direction;
    if direction != before || !was_confirmed {
        let f = app.facts();
        app.update_flow(|flow| flow.direction_changed(&f));
    }
    app.update_flow(|flow| flow.set_step(Step::Import));
    if direction != before {
        super::queue::scan(app);
    } else {
        super::sync(app);
    }
}

pub fn sync(app: &Shared) {
    let current = app.settings.borrow().preset();
    let confirmed = app.target_confirmed.get();
    for (preset, button, mark, _) in &app.w.direction.cards {
        let selected = confirmed && *preset == current;
        mark.set_icon_name(Some(if selected {
            "object-select-symbolic"
        } else {
            "go-next-symbolic"
        }));
        if selected {
            mark.add_css_class("accent");
        } else {
            mark.remove_css_class("accent");
        }
        button.set_sensitive(!app.busy());
        button.update_state(&[gtk::accessible::State::Checked(if selected {
            gtk::AccessibleTristate::True
        } else {
            gtk::AccessibleTristate::False
        })]);
    }
}

//! Photo inspector beside the browser (macOS `focusedPreview` /
//! `inspector`): media, status, this run's plan or review, and source.

use super::thumbs;
use super::Shared;
use crate::i18n::{fill, gettext};
use crate::model::flow::Step;
use crate::model::queue::PhotoItem;
use crate::model::settings::{Direction, ExportMode};
use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub struct Inspector {
    /// Back row (narrow queues) + stack.
    pub root: gtk::Box,
    stack: gtk::Stack,
    name: gtk::Label,
    picture: gtk::Picture,
    status: gtk::Label,
    plan_title: gtk::Label,
    plan_main: gtk::Label,
    plan_detail: gtk::Label,
    extra: gtk::Label,
    cover: gtk::Button,
    toggle: gtk::Button,
    relative: gtk::Label,
    detail: gtk::Label,
    showing: Rc<RefCell<PathBuf>>,
    index: std::cell::Cell<Option<usize>>,
}

fn text(class: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .build();
    for c in class {
        l.add_css_class(c);
    }
    l
}

impl Inspector {
    pub fn new() -> Inspector {
        let heading = text(&["heading"]);
        heading.set_label(&gettext("Photo Inspector"));
        let name = text(&["heading"]);
        name.set_selectable(true);
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .height_request(200)
            .build();
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("card");
        frame.set_overflow(gtk::Overflow::Hidden);
        frame.append(&picture);
        let status = text(&["dim-label"]);
        let plan_title = text(&["heading"]);
        let plan_main = text(&[]);
        let plan_detail = text(&["dim-label"]);
        let extra = text(&["caption", "dim-label"]);
        let cover = gtk::Button::with_label(&gettext("Choose Cover…"));
        cover.set_halign(gtk::Align::Start);
        let toggle = gtk::Button::new();
        toggle.set_halign(gtk::Align::Start);
        let source_title = text(&["heading"]);
        source_title.set_label(&gettext("Source and Resources"));
        let relative = text(&["caption"]);
        relative.set_selectable(true);
        let detail = text(&["caption", "dim-label"]);
        detail.set_selectable(true);
        let note = text(&["caption", "dim-label"]);
        note.set_label(&gettext(
            "Previewing does not change global selection. Conversion and unchanged export create separate copies; sources stay unchanged.",
        ));

        let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        content.set_margin_start(16);
        content.set_margin_end(16);
        content.set_margin_top(16);
        content.set_margin_bottom(16);
        for w in [
            heading.upcast_ref::<gtk::Widget>(),
            name.upcast_ref(),
            frame.upcast_ref(),
            status.upcast_ref(),
        ] {
            content.append(w);
        }
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        for w in [
            plan_title.upcast_ref::<gtk::Widget>(),
            plan_main.upcast_ref(),
            plan_detail.upcast_ref(),
            extra.upcast_ref(),
            cover.upcast_ref(),
            toggle.upcast_ref(),
        ] {
            content.append(w);
        }
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        for w in [
            source_title.upcast_ref::<gtk::Widget>(),
            relative.upcast_ref(),
            detail.upcast_ref(),
            note.upcast_ref(),
        ] {
            content.append(w);
        }
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&content)
            .build();

        let empty_title = text(&["heading"]);
        empty_title.set_label(&gettext("Photo Inspector"));
        let empty_text = text(&["dim-label"]);
        empty_text.set_label(&gettext(
            "Add sources, then open a photo preview to inspect features and the plan.",
        ));
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 12);
        empty.set_margin_start(16);
        empty.set_margin_end(16);
        empty.set_margin_top(16);
        empty.append(&empty_title);
        empty.append(&empty_text);

        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroll, Some("photo"));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        root.append(&stack);
        root.add_css_class("view");
        Inspector {
            root,
            stack,
            name,
            picture,
            status,
            plan_title,
            plan_main,
            plan_detail,
            extra,
            cover,
            toggle,
            relative,
            detail,
            showing: Rc::default(),
            index: std::cell::Cell::new(None),
        }
    }
}

pub fn connect(app: &Shared) {
    let a = app.clone();
    app.w.inspector.cover.connect_clicked(move |_| {
        if let Some(i) = a.w.inspector.index.get() {
            super::cover::present(&a, i);
        }
    });
    let a = app.clone();
    app.w.inspector.toggle.connect_clicked(move |_| {
        if let Some(i) = a.w.inspector.index.get() {
            a.queue.borrow_mut().toggle(i);
            super::sync(&a);
        }
    });
}

/// The photo to inspect: the selection if it is shown, else the first
/// shown photo (macOS `previewItem`).
fn preview_index(app: &Shared) -> Option<usize> {
    let selected = app
        .w
        .queue
        .selection
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .map(|o| *o.borrow::<usize>());
    let visible = app.queue.borrow().visible(app.base());
    selected
        .filter(|s| visible.contains(s))
        .or_else(|| visible.first().copied())
}

pub fn show(app: &Shared) {
    let ins = &app.w.inspector;
    let index = preview_index(app);
    ins.index.set(index);
    let item: Option<PhotoItem> = index.and_then(|i| app.queue.borrow().get(i).cloned());
    let Some(item) = item else {
        ins.stack.set_visible_child_name("empty");
        *ins.showing.borrow_mut() = PathBuf::new();
        return;
    };
    ins.stack.set_visible_child_name("photo");
    ins.name.set_label(&item.name);
    let step = app.flow.get().step();
    let reviews = matches!(step, Step::Preview | Step::Results);
    if reviews {
        let (status, _, _) = super::queue::review_text(app, &item);
        ins.status.set_label(&status);
    } else {
        ins.status.set_label(&gettext(if item.is_motion {
            "Motion Photo"
        } else if item.burst.is_some() {
            "Burst"
        } else if item.is_static_hdr {
            "Static HDR"
        } else {
            "Still Photo"
        }));
    }
    let summary: Vec<String> =
        app.with_plan(|p| p.summary(&item).iter().map(|s| gettext(s)).collect());
    let summary = summary.join("; ");
    if step == Step::Results {
        ins.plan_title.set_label(&gettext("Result"));
        let queue = app.queue.borrow();
        match queue.result(&item) {
            Some((state, detail)) => {
                ins.plan_main.set_label(&super::queue::result_title(*state));
                ins.plan_detail.set_label(detail);
            }
            None => {
                ins.plan_main
                    .set_label(&gettext("Not converted in this run"));
                ins.plan_detail.set_label(&summary);
            }
        }
    } else if step == Step::Preview {
        ins.plan_title.set_label(&gettext("Planned Output"));
        ins.plan_main
            .set_label(&gettext(app.with_plan(|p| p.expected_resources(&item))));
        let (status, reason) = app.with_plan(|p| p.anticipated_status(&item));
        let mut line = gettext(status);
        if let Some(r) = reason {
            line.push_str(&gettext(&r));
        }
        ins.plan_detail.set_label(&format!("{line}\n{summary}"));
    } else {
        ins.plan_title
            .set_label(&gettext("Features and This Run's Plan"));
        ins.plan_main.set_label(&summary);
        ins.plan_detail.set_label("");
    }
    let extra = if let Some(b) = &item.burst {
        b.index
            .map(|i| {
                fill(
                    &gettext(
                        "Original burst index: {index}; the camera's original total is unknown.",
                    ),
                    &[("index", &i.to_string())],
                )
            })
            .unwrap_or_default()
    } else if !item.is_motion {
        gettext("Still image: no video, motion cover, or Live Photo pair is generated.")
    } else if let Some(i) = app
        .settings
        .borrow()
        .cover_selections
        .get(item.source.to_string_lossy().as_ref())
    {
        fill(
            &gettext("High-resolution cover selected: candidate {index}"),
            &[("index", &i.to_string())],
        )
    } else {
        String::new()
    };
    ins.extra.set_label(&extra);
    ins.extra.set_visible(!extra.is_empty() && !reviews);
    ins.plan_detail
        .set_visible(!ins.plan_detail.label().is_empty());
    let s = app.settings.borrow();
    ins.cover.set_visible(
        !reviews
            && item.is_motion
            && s.direction == Direction::ToApple
            && s.export_mode != ExportMode::AuxiliaryOnly,
    );
    drop(s);
    ins.cover.set_sensitive(!app.busy());
    ins.toggle.set_visible(!reviews);
    ins.toggle.set_sensitive(!app.busy());
    let checked = index.is_some_and(|i| app.queue.borrow().is_checked(i));
    ins.toggle.set_label(&gettext(if checked {
        "Deselect this photo"
    } else {
        "Select this photo"
    }));
    ins.relative.set_label(&item.relative);
    ins.detail.set_label(&item.detail);

    // Preview: the cached thumbnail at once, then a larger decode.
    let path = item.source.clone();
    if *ins.showing.borrow() == path {
        return;
    }
    *ins.showing.borrow_mut() = path.clone();
    ins.picture.set_paintable(app.thumbs.cached(&path).as_ref());
    let picture = ins.picture.clone();
    let showing = ins.showing.clone();
    glib::spawn_future_local(async move {
        if let Some(tex) = thumbs::preview(path.clone()).await {
            if *showing.borrow() == path {
                picture.set_paintable(Some(&tex));
            }
        }
    });
}

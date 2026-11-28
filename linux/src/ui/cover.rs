//! Choose Cover… (macOS cover editor): high-resolution cover candidates
//! decoded from the motion video by the core (`cover-candidates`).

use super::Shared;
use crate::engine;
use crate::i18n::{fill, gettext};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use momentstays_core::engine::StopToken;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;

/// Most candidates decoded for one photo.
const MAX: usize = 12;

struct Candidate {
    index: i64,
    seconds: f64,
    image: Option<PathBuf>,
}

fn load(source: PathBuf, folder: PathBuf) -> Result<Vec<Candidate>, String> {
    let token = Arc::new(StopToken::new());
    let list = engine::run(
        &json!({"mode": "cover-candidates", "inputs": [source], "output": folder}),
        Arc::default(),
        token.clone(),
    )?;
    let mut out = Vec::new();
    for c in list["candidates"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MAX)
    {
        let Some(index) = c["index"].as_i64() else {
            continue;
        };
        let dir = folder.join(index.to_string());
        let image = engine::run(
            &json!({"mode": "cover-candidates", "inputs": [source], "output": dir, "index": index}),
            Arc::default(),
            token.clone(),
        )
        .ok()
        .map(|_| dir.join("candidate.jpg"))
        .filter(|p| p.exists());
        out.push(Candidate {
            index,
            seconds: c["timestamp_us"].as_f64().unwrap_or(0.0) / 1_000_000.0,
            image,
        });
    }
    Ok(out)
}

pub fn present(app: &Shared, index: usize) {
    if app.busy() {
        return;
    }
    let Some(item) = app.queue.borrow().get(index).cloned() else {
        return;
    };
    let key = item.source.to_string_lossy().into_owned();
    let dialog = adw::Dialog::builder()
        .title(gettext("Choose Cover"))
        .content_width(620)
        .content_height(520)
        .build();
    let header = adw::HeaderBar::new();
    let done = gtk::Button::with_label(&gettext("Done"));
    done.add_css_class("suggested-action");
    header.pack_end(&done);
    let d = dialog.clone();
    done.connect_clicked(move |_| {
        d.close();
    });
    let name = gtk::Label::builder()
        .label(&item.name)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .xalign(0.0)
        .build();
    name.add_css_class("dim-label");
    let loading = adw::StatusPage::builder()
        .title(gettext("Reading high-resolution cover…"))
        .paintable(&adw::SpinnerPaintable::new(None::<&gtk::Widget>))
        .vexpand(true)
        .build();
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.set_margin_start(20);
    content.set_margin_end(20);
    content.set_margin_top(12);
    content.set_margin_bottom(20);
    content.append(&name);
    content.append(&loading);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&content)
            .build(),
    ));
    dialog.set_child(Some(&view));
    dialog.present(Some(&app.w.window));

    let folder = glib::tmp_dir().join(format!(
        "momentstays-covers-{}-{}",
        std::process::id(),
        glib::random_int()
    ));
    let source = item.source.clone();
    let a = app.clone();
    glib::spawn_future_local(async move {
        let f = folder.clone();
        let result = gio::spawn_blocking(move || load(source, f))
            .await
            .unwrap_or_else(|_| Err(gettext("No other high-resolution covers found")));
        content.remove(&loading);
        let candidates = match result {
            Ok(c) if !c.is_empty() => c,
            Ok(_) | Err(_) => {
                let empty = adw::StatusPage::builder()
                    .icon_name("image-missing-symbolic")
                    .title(gettext("No other high-resolution covers found"))
                    .vexpand(true)
                    .build();
                content.append(&empty);
                return;
            }
        };
        let current = a.settings.borrow().cover_selections.get(&key).copied();
        let flow = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .max_children_per_line(4)
            .column_spacing(12)
            .row_spacing(12)
            .build();
        let mut first: Option<gtk::ToggleButton> = None;
        let mut options: Vec<(Option<i64>, gtk::Widget, String)> = vec![(
            None,
            gtk::Image::from_icon_name("image-x-generic-symbolic").upcast(),
            gettext("Source cover"),
        )];
        for c in &candidates {
            let picture = gtk::Picture::builder()
                .content_fit(gtk::ContentFit::Cover)
                .height_request(96)
                .build();
            if let Some(p) = &c.image {
                if let Ok(t) = gdk::Texture::from_filename(p) {
                    picture.set_paintable(Some(&t));
                }
            }
            options.push((
                Some(c.index),
                picture.upcast(),
                fill(
                    &gettext("Cover {seconds} s"),
                    &[("seconds", &format!("{:.2}", c.seconds))],
                ),
            ));
        }
        for (index, media, caption) in options {
            let label = gtk::Label::new(Some(&caption));
            label.add_css_class("caption");
            let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
            media.set_size_request(120, 96);
            b.append(&media);
            b.append(&label);
            let toggle = gtk::ToggleButton::builder().child(&b).build();
            toggle.add_css_class("flat");
            if let Some(f) = &first {
                toggle.set_group(Some(f));
            } else {
                first = Some(toggle.clone());
            }
            toggle.set_active(index == current);
            let a = a.clone();
            let key = key.clone();
            toggle.connect_toggled(move |t| {
                if !t.is_active() {
                    return;
                }
                {
                    let mut s = a.settings.borrow_mut();
                    match index {
                        Some(i) => {
                            s.cover_selections.insert(key.clone(), i);
                        }
                        None => {
                            s.cover_selections.remove(&key);
                        }
                    }
                }
                super::sync(&a);
            });
            flow.append(&toggle);
        }
        content.append(&flow);
        let note = gtk::Label::builder()
            .label(gettext("Changing the cover does not affect the motion video. New covers are JPEG and do not preserve the original cover's HDR."))
            .wrap(true)
            .xalign(0.0)
            .build();
        note.add_css_class("caption");
        note.add_css_class("dim-label");
        content.append(&note);
        let _ = std::fs::remove_dir_all(&folder);
    });
}

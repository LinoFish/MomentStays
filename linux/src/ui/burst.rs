//! Burst review (macOS `BurstReviewSheet`): members, the representative
//! frame and independent export for each burst group.

use super::Shared;
use crate::i18n::{fill, gettext};
use crate::model::plan::{burst_choice, BurstChoice, BurstGroup, Plan};
use adw::prelude::*;
use gtk::glib;
use std::path::Path;

fn set_choice(app: &Shared, group: &BurstGroup, f: impl FnOnce(&mut BurstChoice)) {
    let mut choice = burst_choice(&app.settings.borrow(), group);
    f(&mut choice);
    app.settings
        .borrow_mut()
        .burst_choices
        .insert(group.id.clone(), choice.json());
}

fn file_name(source: &str) -> String {
    Path::new(source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string())
}

fn page(app: &Shared) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::new();
    let intro = adw::PreferencesGroup::builder()
        .description(glib::markup_escape_text(&gettext("Trusted HONOR JPEG → Apple only. Missing frames are allowed; existing frame indices are preserved. No frames or video are synthesized. Filenames cannot establish the camera's original total; only currently available members are checked.")))
        .build();
    page.add(&intro);
    let groups = app.groups.borrow().clone();
    for group in &groups {
        let (selected, reason) = {
            let queue = app.queue.borrow();
            let settings = app.settings.borrow();
            let plan = Plan {
                queue: &queue,
                settings: &settings,
                groups: &groups,
                unconverted: app.unconverted.get(),
            };
            (
                plan.burst_selected_count(group),
                plan.burst_blocking_reason(group),
            )
        };
        let choice = burst_choice(&app.settings.borrow(), group);
        let g = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&fill(
                &gettext("{label} · {frames} frames · {selected} selected"),
                &[
                    ("label", &group.label),
                    ("frames", &group.members.len().to_string()),
                    ("selected", &selected.to_string()),
                ],
            )))
            .build();
        if let Some(r) = &reason {
            g.set_description(Some(&glib::markup_escape_text(&gettext(r))));
        }
        let all = selected == group.members.len();
        let toggle_all = gtk::Button::builder()
            .label(gettext(if all {
                "Deselect Group"
            } else {
                "Select Group"
            }))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let a = app.clone();
        let members = group.members.clone();
        toggle_all.connect_clicked(move |_| {
            {
                let mut q = a.queue.borrow_mut();
                for m in &members {
                    if let Some(i) = q.position(Path::new(m)) {
                        q.set_checked(i, !all);
                    }
                }
            }
            refresh(&a);
        });
        g.set_header_suffix(Some(&toggle_all));
        let mut radio_group: Option<gtk::CheckButton> = None;
        for (n, member) in group.members.iter().enumerate() {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&file_name(member)))
                .subtitle(glib::markup_escape_text(
                    &if group.representative.as_deref() == Some(member.as_str()) {
                        format!(
                            "{} · {}",
                            fill(
                                &gettext("Frame {index}"),
                                &[("index", &(n + 1).to_string())]
                            ),
                            gettext("Original COVER")
                        )
                    } else {
                        fill(
                            &gettext("Frame {index}"),
                            &[("index", &(n + 1).to_string())],
                        )
                    },
                ))
                .build();
            let index = app.queue.borrow().position(Path::new(member));
            let include = gtk::CheckButton::builder()
                .active(index.is_some_and(|i| app.queue.borrow().is_checked(i)))
                .valign(gtk::Align::Center)
                .tooltip_text(gettext("Select this photo"))
                .build();
            let a = app.clone();
            include.connect_toggled(move |c| {
                if let Some(i) = index {
                    a.queue.borrow_mut().set_checked(i, c.is_active());
                    refresh(&a);
                }
            });
            row.add_prefix(&include);
            let rep = gtk::CheckButton::builder()
                .active(choice.representative.as_deref() == Some(member.as_str()))
                .valign(gtk::Align::Center)
                .tooltip_text(fill(
                    &gettext("Set frame {index} as representative"),
                    &[("index", &(n + 1).to_string())],
                ))
                .build();
            if let Some(r) = &radio_group {
                rep.set_group(Some(r));
            } else {
                radio_group = Some(rep.clone());
            }
            let a = app.clone();
            let gr = group.clone();
            let m = member.clone();
            rep.connect_toggled(move |c| {
                if c.is_active() {
                    set_choice(&a, &gr, |ch| ch.representative = Some(m.clone()));
                    refresh(&a);
                }
            });
            row.add_suffix(&rep);
            g.add(&row);
        }
        let independent = adw::SwitchRow::builder()
            .title(gettext(
                "Export selected photos independently (no burst markers)",
            ))
            .active(choice.independent)
            .title_lines(0)
            .build();
        let a = app.clone();
        let gr = group.clone();
        independent.connect_active_notify(move |r| {
            let on = r.is_active();
            set_choice(&a, &gr, |ch| ch.independent = on);
            refresh(&a);
        });
        g.add(&independent);
        page.add(&g);
    }
    page
}

thread_local! {
    static OPEN: std::cell::RefCell<Option<(adw::Dialog, adw::ToolbarView)>> = const { std::cell::RefCell::new(None) };
}

/// Rebuild the open dialog and the window after a change.
fn refresh(app: &Shared) {
    let a = app.clone();
    glib::idle_add_local_once(move || {
        OPEN.with(|o| {
            if let Some((_, view)) = o.borrow().as_ref() {
                view.set_content(Some(&page(&a)));
            }
        });
        super::sync(&a);
    });
}

pub fn present(app: &Shared) {
    if app.busy() || app.groups.borrow().is_empty() {
        return;
    }
    let dialog = adw::Dialog::builder()
        .title(gettext("Burst Conversion Plan"))
        .content_width(560)
        .content_height(600)
        .build();
    let header = adw::HeaderBar::new();
    let done = gtk::Button::with_label(&gettext("Done"));
    done.add_css_class("suggested-action");
    done.set_tooltip_text(Some(&gettext("Save selection and return to workspace")));
    let d = dialog.clone();
    done.connect_clicked(move |_| {
        d.close();
    });
    header.pack_end(&done);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&header);
    view.set_content(Some(&page(app)));
    dialog.set_child(Some(&view));
    dialog.connect_closed(|_| {
        OPEN.with(|o| o.borrow_mut().take());
    });
    OPEN.with(|o| *o.borrow_mut() = Some((dialog.clone(), view)));
    dialog.present(Some(&app.w.window));
}

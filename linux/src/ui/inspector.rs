//! Inspector pane: the core's scan facts for the selected photo.

use super::thumbs::{self, Thumbnails};
use crate::i18n::gettext;
use crate::model::queue::{BurstFormat, Category, PhotoItem};
use adw::prelude::*;
use gtk::glib;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub struct Inspector {
    pub root: gtk::Stack,
    picture: gtk::Picture,
    rows: Vec<(Fact, adw::ActionRow)>,
    burst_group: adw::PreferencesGroup,
    burst_rows: Vec<(BurstFact, adw::ActionRow)>,
    showing: Rc<RefCell<PathBuf>>,
}

#[derive(Clone, Copy)]
enum Fact {
    File,
    Folder,
    Kind,
    Included,
    Manufacturer,
    Hdr,
    Cover,
    Detail,
}

#[derive(Clone, Copy)]
enum BurstFact {
    Format,
    Group,
    Index,
    Cover,
    Support,
}

fn row(title: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle_selectable(true)
        .build();
    row.add_css_class("property");
    row
}

fn kind_text(item: &PhotoItem) -> String {
    match item.category {
        Category::Live if item.row["motion"] == true => gettext("Live / Motion Photo"),
        Category::Live => gettext("Live"),
        Category::Hdr => gettext("Static HDR photo"),
        Category::Burst => gettext("Burst member (experimental)"),
        Category::Attention => gettext("Not convertible in this direction"),
    }
}

fn yes_no(v: bool) -> String {
    if v {
        gettext("Yes")
    } else {
        gettext("No")
    }
}

impl Inspector {
    pub fn new() -> Inspector {
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .height_request(220)
            .build();
        picture.add_css_class("inspector-preview");

        let facts = adw::PreferencesGroup::builder()
            .title(gettext("Photo"))
            .build();
        let scan = adw::PreferencesGroup::builder()
            .title(gettext("Scan"))
            .description(gettext("Reported by the conversion engine."))
            .build();
        let mut rows = Vec::new();
        for (fact, title, group) in [
            (Fact::File, gettext("File"), &facts),
            (Fact::Folder, gettext("Location"), &facts),
            (Fact::Kind, gettext("Type"), &facts),
            (Fact::Included, gettext("Included"), &facts),
            (Fact::Manufacturer, gettext("Device"), &scan),
            (Fact::Hdr, gettext("HDR"), &scan),
            (Fact::Cover, gettext("Cover"), &scan),
            (Fact::Detail, gettext("Engine Note"), &scan),
        ] {
            let r = row(&title);
            group.add(&r);
            rows.push((fact, r));
        }
        let burst_group = adw::PreferencesGroup::builder()
            .title(gettext("Burst"))
            .build();
        let mut burst_rows = Vec::new();
        for (fact, title) in [
            (BurstFact::Format, gettext("Format")),
            (BurstFact::Group, gettext("Group")),
            (BurstFact::Index, gettext("Frame")),
            (BurstFact::Cover, gettext("Original Cover Frame")),
            (BurstFact::Support, gettext("Support")),
        ] {
            let r = row(&title);
            burst_group.add(&r);
            burst_rows.push((fact, r));
        }

        let content = gtk::Box::new(gtk::Orientation::Vertical, 18);
        content.set_margin_start(12);
        content.set_margin_end(12);
        content.set_margin_top(12);
        content.set_margin_bottom(18);
        content.append(&picture);
        content.append(&facts);
        content.append(&scan);
        content.append(&burst_group);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&content)
            .build();

        let empty = adw::StatusPage::builder()
            .icon_name("image-x-generic-symbolic")
            .title(gettext("No Photo Selected"))
            .description(gettext("Select a photo to see what the scan found."))
            .build();
        empty.add_css_class("compact");
        let root = gtk::Stack::new();
        root.add_named(&empty, Some("empty"));
        root.add_named(&scroll, Some("facts"));
        Inspector {
            root,
            picture,
            rows,
            burst_group,
            burst_rows,
            showing: Rc::default(),
        }
    }

    pub fn show(&self, item: Option<&PhotoItem>, thumbs: &Rc<Thumbnails>) {
        let Some(item) = item else {
            self.root.set_visible_child_name("empty");
            *self.showing.borrow_mut() = PathBuf::new();
            return;
        };
        self.root.set_visible_child_name("facts");
        let dash = || "—".to_string();
        let or_dash = |s: &str| if s.is_empty() { dash() } else { s.to_string() };
        for (fact, row) in &self.rows {
            let value = match fact {
                Fact::File => item.name.clone(),
                Fact::Folder => item
                    .source
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(dash),
                Fact::Kind => kind_text(item),
                Fact::Included => yes_no(item.included),
                Fact::Manufacturer => {
                    if item.manufacturer_detail.is_empty() {
                        or_dash(&item.manufacturer)
                    } else {
                        format!("{} · {}", item.manufacturer, item.manufacturer_detail)
                    }
                }
                Fact::Hdr => or_dash(&item.hdr),
                Fact::Cover => or_dash(&item.cover),
                Fact::Detail => or_dash(&item.detail),
            };
            row.set_subtitle(&glib::markup_escape_text(&value));
        }
        match &item.burst {
            Some(b) => {
                self.burst_group.set_visible(true);
                for (fact, row) in &self.burst_rows {
                    let value = match fact {
                        BurstFact::Format => match b.format {
                            BurstFormat::Google => "Google".to_string(),
                            BurstFormat::Samsung => gettext("Samsung"),
                            BurstFormat::Huawei => gettext("Huawei"),
                            BurstFormat::Honor => gettext("Honor"),
                        },
                        BurstFact::Group => or_dash(&b.group),
                        BurstFact::Index => b.index.map(|i| i.to_string()).unwrap_or_else(dash),
                        BurstFact::Cover => yes_no(b.source_cover),
                        BurstFact::Support => {
                            if b.supported {
                                gettext("Supported (experimental)")
                            } else {
                                or_dash(&b.reason)
                            }
                        }
                    };
                    row.set_subtitle(&glib::markup_escape_text(&value));
                }
            }
            None => self.burst_group.set_visible(false),
        }

        // Preview: the cached thumbnail at once, then a larger decode.
        let path = item.source.clone();
        *self.showing.borrow_mut() = path.clone();
        self.picture.set_paintable(thumbs.cached(&path).as_ref());
        let picture = self.picture.clone();
        let showing = self.showing.clone();
        glib::spawn_future_local(async move {
            if let Some(tex) = thumbs::preview(path.clone()).await {
                if *showing.borrow() == path {
                    picture.set_paintable(Some(&tex));
                }
            }
        });
    }
}

//! Photo queue grid: scanning, list model, cells with include checkboxes.

use super::{thumbs::ThumbCallback, window::update_status, Shared};
use crate::engine;
use crate::i18n::gettext;
use crate::model::queue::{BurstFormat, Category, PhotoItem};
use crate::model::settings::scan_request;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use momentstays_core::engine::StopToken;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

/// Scan all source folders for the current direction (off the main thread).
pub fn scan(app: &Shared) {
    let generation = app.scan_generation.get() + 1;
    app.scan_generation.set(generation);
    let folders = app.sources.borrow().paths();
    if folders.is_empty() {
        app.scanning.set(false);
        app.queue.borrow_mut().clear();
        refresh(app);
        return;
    }
    app.scanning.set(true);
    refresh(app);
    let direction = app.settings.borrow().direction;
    let req = scan_request(&folders, direction);
    let a = app.clone();
    glib::spawn_future_local(async move {
        let reply = gio::spawn_blocking(move || {
            engine::run(&req, Arc::default(), Arc::new(StopToken::new()))
        })
        .await
        .unwrap_or_else(|_| Err(gettext("The scan stopped unexpectedly.")));
        if a.scan_generation.get() != generation {
            return; // A newer scan replaced this one.
        }
        a.scanning.set(false);
        match reply {
            Ok(reply) => a.queue.borrow_mut().replace(engine::rows(reply), direction),
            Err(message) => {
                a.queue.borrow_mut().clear();
                a.toast(&message);
            }
        }
        refresh(&a);
    });
}

/// Refill the grid from the queue's visible items and update counts.
pub fn refresh(app: &Shared) {
    let selected = app
        .w
        .selection
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .and_then(|o| {
            app.queue
                .borrow()
                .get(*o.borrow::<usize>())
                .map(|i| i.source.clone())
        });
    let visible = app.queue.borrow().visible();
    let objects: Vec<glib::Object> = visible
        .iter()
        .map(|i| glib::BoxedAnyObject::new(*i).upcast())
        .collect();
    app.binding.set(true);
    app.w.store.splice(0, app.w.store.n_items(), &objects);
    app.binding.set(false);
    // Keep the selected photo selected when it is still shown.
    let reselect = selected.and_then(|path| {
        let queue = app.queue.borrow();
        let index = queue.position(&path)?;
        visible.iter().position(|v| *v == index)
    });
    app.w
        .selection
        .set_selected(reselect.map_or(gtk::INVALID_LIST_POSITION, |p| p as u32));

    let queue_empty = app.queue.borrow().is_empty();
    let page = if app.scanning.get() {
        "scanning"
    } else if queue_empty {
        "empty"
    } else if visible.is_empty() {
        "no-match"
    } else {
        "grid"
    };
    app.w.content_stack.set_visible_child_name(page);
    update_status(app);
}

pub fn category_label(item: &PhotoItem) -> String {
    match item.category {
        Category::Live => gettext("Live"),
        Category::Hdr => gettext("HDR"),
        Category::Burst => match item.burst.as_ref().map(|b| b.format) {
            Some(BurstFormat::Google) => gettext("Burst · Google"),
            Some(BurstFormat::Samsung) => gettext("Burst · Samsung"),
            Some(BurstFormat::Huawei) => gettext("Burst · Huawei"),
            _ => gettext("Burst · Honor"),
        },
        Category::Attention => gettext("Not Convertible"),
    }
}

struct Cell_ {
    root: gtk::Box,
    picture: gtk::Picture,
    check: gtk::CheckButton,
    badge: gtk::Label,
    name: gtk::Label,
    caption: gtk::Label,
    index: Cell<Option<usize>>,
    path: RefCell<PathBuf>,
    updating: Cell<bool>,
}

impl Cell_ {
    fn new() -> Rc<Cell_> {
        let placeholder = gtk::Image::from_icon_name("image-x-generic-symbolic");
        placeholder.set_pixel_size(48);
        placeholder.add_css_class("dim-label");
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        let frame = gtk::Overlay::builder()
            .child(&placeholder)
            .width_request(156)
            .height_request(156)
            .overflow(gtk::Overflow::Hidden)
            .build();
        frame.add_css_class("photo-thumb");
        frame.add_overlay(&picture);
        let check = gtk::CheckButton::builder()
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_start(6)
            .margin_top(6)
            .tooltip_text(gettext("Include in This Conversion"))
            .build();
        check.add_css_class("selection-mode");
        frame.add_overlay(&check);
        let badge = gtk::Label::builder()
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_end(6)
            .margin_top(6)
            .build();
        badge.add_css_class("photo-badge");
        frame.add_overlay(&badge);
        let name = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .max_width_chars(18)
            .build();
        let caption = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .build();
        caption.add_css_class("caption");
        caption.add_css_class("dim-label");
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("photo-cell");
        root.append(&frame);
        root.append(&name);
        root.append(&caption);
        Rc::new(Cell_ {
            root,
            picture,
            check,
            badge,
            name,
            caption,
            index: Cell::new(None),
            path: RefCell::default(),
            updating: Cell::new(false),
        })
    }

    fn show(&self, item: Option<(usize, &PhotoItem)>, thumb: Option<gdk::Texture>) {
        self.updating.set(true);
        match item {
            Some((index, item)) => {
                self.index.set(Some(index));
                *self.path.borrow_mut() = item.source.clone();
                self.name.set_label(&item.name);
                self.root.set_tooltip_text(Some(&item.relative));
                let caption = if item.manufacturer.is_empty() || item.manufacturer == "未知" {
                    category_label(item)
                } else {
                    format!("{} · {}", category_label(item), item.manufacturer)
                };
                self.caption.set_label(&caption);
                self.badge.set_label(&category_label(item));
                self.badge.set_visible(item.category != Category::Attention);
                self.check.set_active(item.included);
                self.check.set_sensitive(item.category.convertible());
                self.picture.set_paintable(thumb.as_ref());
            }
            None => {
                self.index.set(None);
                self.picture.set_paintable(None::<&gdk::Paintable>);
            }
        }
        self.updating.set(false);
    }
}

pub fn factory(app: &Shared) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let a = app.clone();
    factory.connect_setup(move |_, object| {
        let Some(list_item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let cell = Cell_::new();
        list_item.set_child(Some(&cell.root));

        // Include checkbox writes back to the queue.
        let app = a.clone();
        let c = Rc::downgrade(&cell);
        cell.check.connect_toggled(move |check| {
            let Some(cell) = c.upgrade() else { return };
            if cell.updating.get() || app.binding.get() {
                return;
            }
            if let Some(index) = cell.index.get() {
                app.queue
                    .borrow_mut()
                    .set_included(index, check.is_active());
                update_status(&app);
                let queue = app.queue.borrow();
                if let Some(item) = queue.get(index) {
                    if item.included != check.is_active() {
                        cell.updating.set(true);
                        check.set_active(item.included);
                        cell.updating.set(false);
                    }
                }
            }
        });

        // Finished thumbnails land on whichever cell shows that path now.
        let c = Rc::downgrade(&cell);
        let listener: Rc<ThumbCallback> = Rc::new(move |path, tex| {
            if let Some(cell) = c.upgrade() {
                if cell.index.get().is_some() && *cell.path.borrow() == path {
                    cell.picture.set_paintable(Some(tex));
                }
            }
        });
        a.thumbs.listen(&listener);

        // Rebinding: the list item's `item` changes as the grid recycles cells.
        let app = a.clone();
        list_item.connect_item_notify(move |li| {
            let _keep = &listener;
            let index = li
                .item()
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|o| *o.borrow::<usize>());
            let queue = app.queue.borrow();
            let item = index.and_then(|i| queue.get(i).map(|item| (i, item)));
            let thumb = item.and_then(|(_, it)| app.thumbs.cached(&it.source));
            if let (Some((_, it)), None) = (item, &thumb) {
                app.thumbs.request(&it.source);
            }
            cell.show(item, thumb);
        });
    });
    factory
}

//! Thumbnails: decoded off the main thread with GdkPixbuf (runtime
//! library), at most a few at a time, cached by path for this session.

use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf, gio, glib};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

const SIZE: i32 = 256;
const PARALLEL: usize = 3;

/// Called with each finished thumbnail.
pub type ThumbCallback = dyn Fn(&Path, &gdk::Texture);
type Listener = Weak<ThumbCallback>;

#[derive(Default)]
pub struct Thumbnails {
    cache: RefCell<HashMap<PathBuf, gdk::Texture>>,
    failed: RefCell<HashSet<PathBuf>>,
    queued: RefCell<VecDeque<PathBuf>>,
    pending: RefCell<HashSet<PathBuf>>,
    running: Cell<usize>,
    listeners: RefCell<Vec<Listener>>,
}

struct Decoded {
    bytes: glib::Bytes,
    width: i32,
    height: i32,
    stride: usize,
    alpha: bool,
}

fn decode(path: &Path, size: i32) -> Option<Decoded> {
    let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(path, size, size, true).ok()?;
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    Some(Decoded {
        bytes: pixbuf.read_pixel_bytes(),
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride: pixbuf.rowstride() as usize,
        alpha: pixbuf.has_alpha(),
    })
}

fn texture(d: Decoded) -> gdk::Texture {
    let format = if d.alpha {
        gdk::MemoryFormat::R8g8b8a8
    } else {
        gdk::MemoryFormat::R8g8b8
    };
    gdk::MemoryTexture::new(d.width, d.height, format, &d.bytes, d.stride).upcast()
}

impl Thumbnails {
    pub fn cached(&self, path: &Path) -> Option<gdk::Texture> {
        self.cache.borrow().get(path).cloned()
    }

    /// Called with each finished thumbnail while `listener` is alive.
    pub fn listen(&self, listener: &Rc<ThumbCallback>) {
        let mut listeners = self.listeners.borrow_mut();
        listeners.retain(|l| l.strong_count() > 0);
        listeners.push(Rc::downgrade(listener));
    }

    /// Ask for a thumbnail; listeners hear about it when it is ready.
    pub fn request(self: &Rc<Self>, path: &Path) {
        if self.cache.borrow().contains_key(path)
            || self.failed.borrow().contains(path)
            || !self.pending.borrow_mut().insert(path.to_path_buf())
        {
            return;
        }
        self.queued.borrow_mut().push_back(path.to_path_buf());
        self.pump();
    }

    fn pump(self: &Rc<Self>) {
        while self.running.get() < PARALLEL {
            let Some(path) = self.queued.borrow_mut().pop_front() else {
                return;
            };
            self.running.set(self.running.get() + 1);
            let this = self.clone();
            glib::spawn_future_local(async move {
                let p = path.clone();
                let decoded = gio::spawn_blocking(move || decode(&p, SIZE))
                    .await
                    .ok()
                    .flatten();
                this.running.set(this.running.get() - 1);
                this.pending.borrow_mut().remove(&path);
                match decoded {
                    Some(d) => {
                        let tex = texture(d);
                        this.cache.borrow_mut().insert(path.clone(), tex.clone());
                        let listeners: Vec<_> = this
                            .listeners
                            .borrow()
                            .iter()
                            .filter_map(Weak::upgrade)
                            .collect();
                        for l in listeners {
                            l(&path, &tex);
                        }
                    }
                    None => {
                        this.failed.borrow_mut().insert(path);
                    }
                }
                this.pump();
            });
        }
    }
}

/// A larger preview for the inspector (decoded off the main thread).
pub async fn preview(path: PathBuf) -> Option<gdk::Texture> {
    gio::spawn_blocking(move || decode(&path, 720))
        .await
        .ok()
        .flatten()
        .map(texture)
}

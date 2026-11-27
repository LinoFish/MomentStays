//! The photo queue (macOS `PhotoQueueView`): scanning, the feature tab
//! row and browser tools, grid and list browsers, and click semantics
//! (click = inspect; check mark or Ctrl-click = include).

use super::{thumbs::ThumbCallback, Shared};
use crate::engine;
use crate::i18n::{fill, gettext};
use crate::model::flow::Step;
use crate::model::plan::{Bucket, BurstGroup};
use crate::model::queue::{Feature, PhotoItem, ResultState, Tab};
use crate::model::settings::{scan_request, Direction, ExportMode};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use momentstays_core::engine::StopToken;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub struct QueueView {
    /// Tabs and tools; vertical (two rows) on narrow queues.
    pub heading: gtk::Box,
    tabs: adw::ToggleGroup,
    tab_key: RefCell<String>,
    filter_menu: gtk::MenuButton,
    filter_checks: Vec<(Feature, gtk::CheckButton)>,
    burst_button: gtk::Button,
    search_toggle: gtk::Button,
    pub search_inline: gtk::Revealer,
    pub search_below: gtk::Revealer,
    search_entry: gtk::SearchEntry,
    search_entry_below: gtk::SearchEntry,
    pub inspector_toggle: gtk::ToggleButton,
    view_toggle: gtk::Button,
    more_menu: gtk::MenuButton,
    pub inspector_split: adw::OverlaySplitView,
    pub inspector_back: gtk::Box,
    browser: gtk::Stack,
    grid: gtk::GridView,
    list: gtk::ListView,
    back: gtk::Button,
    empty_page: adw::StatusPage,
    store: gio::ListStore,
    pub selection: gtk::SingleSelection,
}

fn tool(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.add_css_class("flat");
    b.set_tooltip_text(Some(tooltip));
    b
}

fn feature_title(f: Feature) -> String {
    gettext(match f {
        Feature::Live => "Live Photo",
        Feature::Hdr => "HDR",
        Feature::Burst => "Burst",
        Feature::MissingTime => "Missing Date",
    })
}

pub fn result_title(s: ResultState) -> String {
    gettext(match s {
        ResultState::Converted => "Converted",
        ResultState::Resumed => "Existing Result",
        ResultState::Copied => "Unchanged export",
        ResultState::Failed => "Failed",
        ResultState::Skipped => "Skipped",
    })
}

impl QueueView {
    pub fn new(window: &adw::ApplicationWindow) -> QueueView {
        let tabs = adw::ToggleGroup::new();
        let tab_scroll = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::External)
            .hexpand(true)
            .child(&tabs)
            .build();
        tabs.set_halign(gtk::Align::Start);

        // Filter menu (results step): features combine (AND).
        let filter_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
        filter_box.set_margin_start(6);
        filter_box.set_margin_end(6);
        filter_box.set_margin_top(6);
        filter_box.set_margin_bottom(6);
        let mut filter_checks = Vec::new();
        for f in Feature::ALL {
            let c = gtk::CheckButton::with_label(&feature_title(f));
            filter_box.append(&c);
            filter_checks.push((f, c));
        }
        filter_box.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        let clear = gtk::Button::with_label(&gettext("Clear Feature Filters"));
        clear.add_css_class("flat");
        clear.set_action_name(Some("win.clear-feature-filters"));
        filter_box.append(&clear);
        let popover = gtk::Popover::builder().child(&filter_box).build();
        let filter_menu = gtk::MenuButton::builder()
            .label(gettext("Filter"))
            .popover(&popover)
            .always_show_arrow(true)
            .tooltip_text(gettext(
                "Filter by photo features. Multiple choices show photos matching all selected features without changing task scope",
            ))
            .css_classes(["flat"])
            .build();

        let burst_button = tool("view-paged-symbolic", &gettext("Inspect Burst Groups"));
        burst_button.set_action_name(Some("win.burst-review"));
        let search_toggle = tool("system-search-symbolic", &gettext("Search Photos"));
        search_toggle.set_action_name(Some("win.search"));

        let search_entry = gtk::SearchEntry::builder()
            .placeholder_text(gettext("Search filenames, sources, or devices"))
            .width_request(200)
            .build();
        let search_inline = gtk::Revealer::builder()
            .child(&search_entry)
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .build();
        let search_entry_below = gtk::SearchEntry::builder()
            .placeholder_text(gettext("Search filenames, sources, or devices"))
            .hexpand(true)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        search_entry
            .bind_property("text", &search_entry_below, "text")
            .bidirectional()
            .build();
        let search_below = gtk::Revealer::builder()
            .child(&search_entry_below)
            .visible(false)
            .build();
        search_entry.set_key_capture_widget(Some(window));

        let inspector_toggle = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-right-symbolic")
            .tooltip_text(gettext("Show or hide the photo inspector (Ctrl+Alt+P)"))
            .css_classes(["flat"])
            .build();
        inspector_toggle.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Photo Inspector",
        ))]);
        let view_toggle = tool("view-list-bullet-symbolic", &gettext("Switch to List"));
        let more = gio::Menu::new();
        let selection = gio::Menu::new();
        selection.append(
            Some(&gettext("Select Filtered Photos")),
            Some("win.select-shown"),
        );
        selection.append(Some(&gettext("Deselect All")), Some("win.deselect-all"));
        selection.append(
            Some(&gettext("Restore Default Processing Scope")),
            Some("win.restore-scope"),
        );
        more.append_section(None, &selection);
        let filters = gio::Menu::new();
        filters.append(
            Some(&gettext("Clear Filters and View All")),
            Some("win.clear-filters"),
        );
        more.append_section(None, &filters);
        let more_menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&more)
            .tooltip_text(gettext("Selection and filtering actions"))
            .css_classes(["flat"])
            .build();

        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        tools.set_halign(gtk::Align::End);
        tools.append(&filter_menu);
        tools.append(&burst_button);
        tools.append(&search_inline);
        tools.append(&search_toggle);
        tools.append(&inspector_toggle);
        tools.append(&view_toggle);
        tools.append(&more_menu);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        heading.set_margin_start(12);
        heading.set_margin_end(12);
        heading.set_margin_top(6);
        heading.set_margin_bottom(6);
        heading.append(&tab_scroll);
        heading.append(&tools);

        // Browser: grid or list over one model and selection.
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let selection = gtk::SingleSelection::builder()
            .model(&store)
            .autoselect(false)
            .can_unselect(true)
            .build();
        let grid = gtk::GridView::builder()
            .model(&selection)
            .max_columns(12)
            .min_columns(1)
            .build();
        let list = gtk::ListView::builder().model(&selection).build();
        list.add_css_class("rich-list");
        let scroll = |child: &gtk::Widget| {
            gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(child)
                .build()
        };
        let empty_page = adw::StatusPage::builder()
            .icon_name("folder-pictures-symbolic")
            .title(gettext("Add Photos"))
            .description(gettext(
                "Add files, folders, or Photos library items, or drop them into the photo area",
            ))
            .vexpand(true)
            .build();
        let scanning = adw::StatusPage::builder()
            .title(gettext("Recognizing photos"))
            .paintable(&adw::SpinnerPaintable::new(None::<&gtk::Widget>))
            .build();
        let no_match = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title(gettext("No Matching Photos"))
            .description(gettext("Try another filter or search."))
            .build();
        let browser = gtk::Stack::new();
        browser.set_transition_type(gtk::StackTransitionType::Crossfade);
        browser.add_named(&empty_page, Some("empty"));
        browser.add_named(&scanning, Some("scanning"));
        browser.add_named(&no_match, Some("no-match"));
        browser.add_named(&scroll(grid.upcast_ref()), Some("grid"));
        browser.add_named(&scroll(list.upcast_ref()), Some("list"));

        let back = gtk::Button::with_label(&gettext("Back to Photo List"));
        back.add_css_class("flat");
        let back_title = gtk::Label::new(Some(&gettext("Photo Inspector")));
        back_title.add_css_class("heading");
        back_title.set_hexpand(true);
        back_title.set_xalign(1.0);
        let inspector_back = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        inspector_back.set_margin_start(6);
        inspector_back.set_margin_end(10);
        inspector_back.set_margin_top(6);
        inspector_back.set_visible(false);
        inspector_back.append(&back);
        inspector_back.append(&back_title);

        // Inspector beside the browser: min(330, 42 %) of the queue.
        let inspector_split = adw::OverlaySplitView::builder()
            .content(&browser)
            .sidebar_position(gtk::PackType::End)
            .sidebar_width_fraction(0.42)
            .min_sidebar_width(220.0)
            .max_sidebar_width(330.0)
            .show_sidebar(false)
            .build();

        QueueView {
            heading,
            tabs,
            tab_key: RefCell::default(),
            filter_menu,
            filter_checks,
            burst_button,
            search_toggle,
            search_inline,
            search_below,
            search_entry,
            search_entry_below,
            inspector_toggle,
            view_toggle,
            more_menu,
            inspector_split,
            inspector_back,
            browser,
            grid,
            list,
            back,
            empty_page,
            store,
            selection,
        }
    }
}

pub fn connect(app: &Shared) {
    let q = &app.w.queue;
    q.grid.set_factory(Some(&factory(app, true)));
    q.list.set_factory(Some(&factory(app, false)));

    let a = app.clone();
    q.tabs.connect_active_name_notify(move |g| {
        if a.binding.get() {
            return;
        }
        let name = g.active_name().map(|n| n.to_string()).unwrap_or_default();
        {
            let mut queue = a.queue.borrow_mut();
            let results = a.flow.get().step() == Step::Results;
            let mut tab = queue.tab.clone();
            if results {
                tab.result = ResultState::ALL
                    .iter()
                    .find(|s| format!("{s:?}") == name)
                    .copied();
            } else {
                tab.features = Feature::ALL
                    .iter()
                    .find(|f| format!("{f:?}") == name)
                    .map(|f| [*f].into())
                    .unwrap_or_default();
            }
            queue.tab = tab;
        }
        super::sync(&a);
    });
    for (feature, check) in &q.filter_checks {
        let a = app.clone();
        let feature = *feature;
        check.connect_toggled(move |c| {
            if a.binding.get() {
                return;
            }
            {
                let mut queue = a.queue.borrow_mut();
                if c.is_active() {
                    queue.tab.features.insert(feature);
                } else {
                    queue.tab.features.remove(&feature);
                }
            }
            super::sync(&a);
        });
    }
    let clear = gio::SimpleAction::new("clear-feature-filters", None);
    let a = app.clone();
    clear.connect_activate(move |_, _| {
        a.queue.borrow_mut().tab.features.clear();
        super::sync(&a);
    });
    app.w.window.add_action(&clear);

    let a = app.clone();
    q.search_entry.connect_search_changed(move |e| {
        a.queue.borrow_mut().set_search(&e.text());
        if !e.text().is_empty() && !a.search_expanded.get() {
            a.search_expanded.set(true);
        }
        super::sync(&a);
    });
    for entry in [&q.search_entry, &q.search_entry_below] {
        let a = app.clone();
        entry.connect_stop_search(move |_| close_search(&a));
    }

    let a = app.clone();
    q.inspector_toggle.connect_toggled(move |t| {
        a.w.queue.inspector_split.set_show_sidebar(t.is_active());
    });
    let a = app.clone();
    q.inspector_split.connect_show_sidebar_notify(move |s| {
        if a.w.queue.inspector_toggle.is_active() != s.shows_sidebar() {
            a.w.queue.inspector_toggle.set_active(s.shows_sidebar());
        }
    });
    let a = app.clone();
    q.back
        .connect_clicked(move |_| a.w.queue.inspector_split.set_show_sidebar(false));
    let a = app.clone();
    q.view_toggle.connect_clicked(move |_| {
        a.prefers_grid.set(!a.prefers_grid.get());
        super::sync(&a);
    });
    let a = app.clone();
    q.selection.connect_selected_item_notify(move |s| {
        if a.binding.get() {
            return;
        }
        let index = s
            .selected_item()
            .and_downcast::<glib::BoxedAnyObject>()
            .map(|o| *o.borrow::<usize>());
        // macOS `focus`: inspecting opens the inspector on wide queues.
        if index.is_some() && !a.w.queue.inspector_split.is_collapsed() {
            a.w.queue.inspector_split.set_show_sidebar(true);
        }
        super::inspector::show(&a);
        super::sync(&a);
    });
}

pub fn toggle_search(app: &Shared) {
    if app.search_expanded.get() {
        close_search(app);
    } else {
        app.search_expanded.set(true);
        super::sync(app);
        let q = &app.w.queue;
        if q.search_inline.is_visible() {
            q.search_entry.grab_focus();
        } else {
            q.search_entry_below.grab_focus();
        }
    }
}

fn close_search(app: &Shared) {
    app.search_expanded.set(false);
    app.w.queue.search_entry.set_text("");
    super::sync(app);
}

pub fn clear_filters(app: &Shared) {
    {
        let mut q = app.queue.borrow_mut();
        q.tab = Tab::all();
        q.set_source(None);
        q.set_search("");
    }
    app.w.queue.search_entry.set_text("");
    super::sync(app);
}

/// Scan all source folders for the current direction (off the main thread).
pub fn scan(app: &Shared) {
    let generation = app.scan_generation.get() + 1;
    app.scan_generation.set(generation);
    if let Some(t) = app.scan_token.borrow_mut().take() {
        t.cancel();
    }
    let folders = app.sources.borrow().paths();
    if folders.is_empty() {
        app.scanning.set(false);
        app.queue.borrow_mut().clear();
        app.groups.borrow_mut().clear();
        let f = app.facts();
        app.update_flow(|flow| flow.sources_emptied(&f));
        super::sync(app);
        return;
    }
    app.scanning.set(true);
    super::sync(app);
    let direction = app.settings.borrow().direction;
    let req = scan_request(&folders, direction);
    let token = Arc::new(StopToken::new());
    *app.scan_token.borrow_mut() = Some(token.clone());
    let a = app.clone();
    glib::spawn_future_local(async move {
        let reply = gio::spawn_blocking(move || -> Result<(Vec<Value>, Vec<Value>), String> {
            let rows = engine::rows(engine::run(&req, Arc::default(), token.clone())?);
            let rows: Vec<Value> = rows.into_iter().map(engine::annotate).collect();
            let groups =
                if direction.supports_burst() && rows.iter().any(|r| r["burst"].is_object()) {
                    engine::run(
                        &json!({"mode": "burst-plan", "scan_rows": rows}),
                        Arc::default(),
                        token,
                    )
                    .map(|v| v["groups"].as_array().cloned().unwrap_or_default())
                    .unwrap_or_default()
                } else {
                    Vec::new()
                };
            Ok((rows, groups))
        })
        .await
        .unwrap_or_else(|_| Err(gettext("The scan stopped unexpectedly.")));
        if a.scan_generation.get() != generation {
            return; // A newer scan replaced this one.
        }
        a.scanning.set(false);
        a.scan_token.borrow_mut().take();
        match reply {
            Ok((rows, groups)) => {
                a.queue.borrow_mut().replace(rows, direction);
                *a.groups.borrow_mut() = groups.iter().filter_map(BurstGroup::from_json).collect();
            }
            Err(message) => {
                a.queue.borrow_mut().clear();
                a.groups.borrow_mut().clear();
                a.toast(&message);
            }
        }
        super::sync(&a);
    });
}

/// Stop a running scan (the primary button shows Stop while scanning).
pub fn stop_scan(app: &Shared) {
    if let Some(t) = app.scan_token.borrow_mut().take() {
        t.cancel();
    }
}

/// Visible indices in display order (preview groups by bucket).
fn display_order(app: &Shared) -> Vec<usize> {
    let base = app.base();
    let mut visible = app.queue.borrow().visible(base);
    if app.flow.get().step() == Step::Preview {
        app.with_plan(|p| {
            let queue = p.queue;
            visible.sort_by_key(|i| {
                queue
                    .get(*i)
                    .map(|it| p.bucket(it))
                    .unwrap_or(Bucket::NotConverted)
            });
        });
    }
    visible
}

/// Refill the browser, tabs and tools from the queue.
pub fn refresh(app: &Shared) {
    let q = &app.w.queue;
    let step = app.flow.get().step();
    let reviews = matches!(step, Step::Preview | Step::Results);
    sync_tabs(app);

    // Tools.
    q.filter_menu.set_visible(step == Step::Results);
    {
        let queue = app.queue.borrow();
        let n = queue.tab.features.len();
        q.filter_menu.set_label(&if n == 0 {
            gettext("Filter")
        } else {
            fill(&gettext("Filter {count}"), &[("count", &n.to_string())])
        });
        app.binding.set(true);
        for (f, c) in &q.filter_checks {
            c.set_active(queue.tab.features.contains(f));
        }
        app.binding.set(false);
    }
    let groups = app.groups.borrow().clone();
    q.burst_button.set_visible(!reviews && !groups.is_empty());
    q.burst_button.set_sensitive(!app.busy());
    q.burst_button.set_tooltip_text(Some(&fill(
        &gettext("Inspect burst groups: {groups} groups · {frames} frames"),
        &[
            ("groups", &groups.len().to_string()),
            (
                "frames",
                &groups
                    .iter()
                    .map(|g| g.members.len())
                    .sum::<usize>()
                    .to_string(),
            ),
        ],
    )));
    let expanded = app.search_expanded.get();
    q.search_inline.set_reveal_child(expanded);
    q.search_below.set_reveal_child(expanded);
    q.search_toggle
        .set_visible(!expanded || !q.search_inline.is_visible());
    q.inspector_toggle
        .set_sensitive(!app.queue.borrow().is_empty());
    let grid = app.prefers_grid.get();
    q.view_toggle.set_icon_name(if grid {
        "view-list-bullet-symbolic"
    } else {
        "view-grid-symbolic"
    });
    q.view_toggle.set_tooltip_text(Some(&gettext(if grid {
        "Switch to List"
    } else {
        "Switch to Grid"
    })));
    q.more_menu.set_visible(!reviews);
    q.empty_page
        .set_description(Some(&gettext(if app.target_confirmed.get() {
            "Add files, folders, or Photos library items, or drop them into the photo area"
        } else {
            "Choose a conversion direction first"
        })));

    // Browser contents.
    let selected = q
        .selection
        .selected_item()
        .and_downcast::<glib::BoxedAnyObject>()
        .and_then(|o| {
            app.queue
                .borrow()
                .get(*o.borrow::<usize>())
                .map(|i| i.source.clone())
        });
    let visible = display_order(app);
    let current: Vec<usize> = (0..q.store.n_items())
        .filter_map(|i| q.store.item(i).and_downcast::<glib::BoxedAnyObject>())
        .map(|o| *o.borrow::<usize>())
        .collect();
    app.binding.set(true);
    if current != visible {
        let objects: Vec<glib::Object> = visible
            .iter()
            .map(|i| glib::BoxedAnyObject::new(*i).upcast())
            .collect();
        q.store.splice(0, q.store.n_items(), &objects);
    } else {
        // Same photos: rebind so check marks and status lines update.
        let n = q.store.n_items();
        q.store.items_changed(0, n, n);
    }
    let reselect = selected.and_then(|path| {
        let queue = app.queue.borrow();
        let index = queue.position(&path)?;
        visible.iter().position(|v| *v == index)
    });
    q.selection
        .set_selected(reselect.map_or(gtk::INVALID_LIST_POSITION, |p| p as u32));
    app.binding.set(false);

    let queue_empty = app.queue.borrow().is_empty();
    let page = if app.scanning.get() && queue_empty {
        "scanning"
    } else if queue_empty {
        "empty"
    } else if visible.is_empty() {
        "no-match"
    } else if grid {
        "grid"
    } else {
        "list"
    };
    q.browser.set_visible_child_name(page);
    if queue_empty {
        q.inspector_split.set_show_sidebar(false);
    }
}

fn sync_tabs(app: &Shared) {
    let q = &app.w.queue;
    let step = app.flow.get().step();
    let base = app.base();
    let queue = app.queue.borrow();
    let mut entries: Vec<(String, String, usize)> = Vec::new();
    entries.push((
        "All".into(),
        gettext("All"),
        queue.tab_count(base, &Tab::all()),
    ));
    let active = if step == Step::Results {
        for s in ResultState::ALL {
            let n = queue.tab_count(base, &Tab::result(s));
            if n > 0 {
                entries.push((format!("{s:?}"), result_title(s), n));
            }
        }
        queue
            .tab
            .result
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|| "All".into())
    } else {
        for f in Feature::ALL {
            let n = queue.tab_count(base, &Tab::feature(f));
            if n > 0 || f == Feature::MissingTime {
                entries.push((format!("{f:?}"), feature_title(f), n));
            }
        }
        if queue.tab.features.len() == 1 {
            queue
                .tab
                .features
                .iter()
                .next()
                .map(|f| format!("{f:?}"))
                .unwrap_or_else(|| "All".into())
        } else {
            "All".into()
        }
    };
    let key = format!("{entries:?}|{active}");
    if *q.tab_key.borrow() == key {
        return;
    }
    *q.tab_key.borrow_mut() = key;
    app.binding.set(true);
    q.tabs.remove_all();
    for (name, title, count) in &entries {
        let label = gtk::Label::new(Some(title));
        let n = gtk::Label::new(Some(&count.to_string()));
        n.add_css_class("dim-label");
        n.add_css_class("numeric");
        let child = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        child.set_margin_start(6);
        child.set_margin_end(6);
        child.append(&label);
        child.append(&n);
        let toggle = adw::Toggle::builder()
            .name(name.as_str())
            .child(&child)
            .tooltip(gettext(
                "Filter the view only, without changing this run's scope",
            ))
            .build();
        q.tabs.add(toggle);
    }
    if entries.iter().any(|(n, _, _)| *n == active) {
        q.tabs.set_active_name(Some(&active));
    } else {
        q.tabs.set_active_name(Some("All"));
    }
    app.binding.set(false);
}

// ---- Cells ---------------------------------------------------------------

pub fn status_icon(state: Option<ResultState>, bucket: Option<Bucket>) -> &'static str {
    match (state, bucket) {
        (Some(ResultState::Converted), _) => "object-select-symbolic",
        (Some(ResultState::Resumed), _) => "checkbox-checked-symbolic",
        (Some(ResultState::Copied), _) => "edit-copy-symbolic",
        (Some(ResultState::Failed), _) => "dialog-error-symbolic",
        (Some(ResultState::Skipped), _) => "action-unavailable-symbolic",
        (None, Some(Bucket::Ready)) => "view-reveal-symbolic",
        (None, Some(Bucket::Risk)) => "dialog-warning-symbolic",
        (None, _) => "action-unavailable-symbolic",
    }
}

/// The footer line of a photo in steps 4–5 (macOS `reviewLabel`).
pub fn review_text(app: &Shared, item: &PhotoItem) -> (String, &'static str, bool) {
    let step = app.flow.get().step();
    if step == Step::Results {
        let queue = app.queue.borrow();
        if let Some((state, _)) = queue.result(item) {
            return (
                result_title(*state),
                status_icon(Some(*state), None),
                *state == ResultState::Failed,
            );
        }
        return (
            gettext("Not converted in this run"),
            status_icon(None, None),
            false,
        );
    }
    app.with_plan(|p| {
        let (title, _) = p.anticipated_status(item);
        let bucket = p.bucket(item);
        let mut text = gettext(title);
        if title == "Will not convert: " {
            text = gettext("Will Not Convert");
        }
        (text, status_icon(None, Some(bucket)), false)
    })
}

struct Cell_ {
    root: gtk::Box,
    picture: gtk::Picture,
    check: gtk::CheckButton,
    badges: gtk::Box,
    cover: gtk::Button,
    name: gtk::Label,
    caption: gtk::Label,
    status_icon: gtk::Image,
    index: Cell<Option<usize>>,
    path: RefCell<PathBuf>,
    updating: Cell<bool>,
}

fn badge(text: Option<&str>, icon: Option<&str>, tooltip: &str) -> gtk::Widget {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    b.add_css_class("osd");
    if let Some(icon) = icon {
        let i = gtk::Image::from_icon_name(icon);
        i.set_margin_start(6);
        i.set_margin_end(if text.is_some() { 0 } else { 6 });
        i.set_margin_top(3);
        i.set_margin_bottom(3);
        b.append(&i);
    }
    if let Some(text) = text {
        let l = gtk::Label::new(Some(text));
        l.add_css_class("caption-heading");
        l.set_margin_start(if icon.is_some() { 0 } else { 6 });
        l.set_margin_end(6);
        l.set_margin_top(3);
        l.set_margin_bottom(3);
        b.append(&l);
    }
    b.set_tooltip_text(Some(tooltip));
    b.upcast()
}

impl Cell_ {
    fn new(grid: bool) -> Rc<Cell_> {
        let placeholder = gtk::Image::from_icon_name("image-x-generic-symbolic");
        placeholder.set_pixel_size(if grid { 40 } else { 20 });
        placeholder.add_css_class("dim-label");
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_shrink(true)
            .build();
        let frame = gtk::Overlay::builder()
            .child(&placeholder)
            .overflow(gtk::Overflow::Hidden)
            .build();
        frame.add_overlay(&picture);
        let badges = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        badges.set_halign(gtk::Align::Start);
        badges.set_valign(gtk::Align::Start);
        badges.set_margin_start(8);
        badges.set_margin_top(8);
        let cover = gtk::Button::with_label(&gettext("Choose Cover…"));
        cover.add_css_class("osd");
        cover.add_css_class("caption");
        cover.set_halign(gtk::Align::Start);
        cover.set_valign(gtk::Align::End);
        cover.set_margin_start(8);
        cover.set_margin_bottom(8);
        cover.set_visible(false);
        let check = gtk::CheckButton::builder()
            .valign(gtk::Align::Center)
            .tooltip_text(gettext("Select this photo"))
            .build();
        let status_icon = gtk::Image::from_icon_name("view-reveal-symbolic");
        status_icon.set_visible(false);
        let name = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .xalign(0.0)
            .build();
        name.add_css_class("heading");
        let caption = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .xalign(0.0)
            .build();
        caption.add_css_class("caption");
        caption.add_css_class("dim-label");
        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(&name);
        text.append(&caption);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        footer.append(&check);
        footer.append(&status_icon);
        footer.append(&text);
        let root = if grid {
            frame.set_height_request(120);
            frame.set_width_request(168);
            frame.add_overlay(&badges);
            frame.add_overlay(&cover);
            footer.set_margin_start(8);
            footer.set_margin_end(8);
            footer.set_margin_top(6);
            footer.set_margin_bottom(6);
            let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
            card.add_css_class("card");
            card.set_overflow(gtk::Overflow::Hidden);
            card.append(&frame);
            card.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            card.append(&footer);
            card
        } else {
            frame.set_size_request(40, 40);
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            row.append(&frame);
            footer.set_hexpand(true);
            row.append(&footer);
            badges.set_valign(gtk::Align::Center);
            badges.set_margin_start(0);
            badges.set_margin_top(0);
            row.append(&badges);
            row.append(&cover);
            cover.set_valign(gtk::Align::Center);
            cover.set_margin_bottom(0);
            row
        };
        Rc::new(Cell_ {
            root,
            picture,
            check,
            badges,
            cover,
            name,
            caption,
            status_icon,
            index: Cell::new(None),
            path: RefCell::default(),
            updating: Cell::new(false),
        })
    }

    fn show(&self, app: &Shared, item: Option<(usize, &PhotoItem)>, thumb: Option<gdk::Texture>) {
        self.updating.set(true);
        match item {
            Some((index, item)) => {
                self.index.set(Some(index));
                *self.path.borrow_mut() = item.source.clone();
                self.name.set_label(&item.name);
                self.root.set_tooltip_text(Some(&format!(
                    "{}\n{}{}\n{}",
                    item.relative,
                    gettext("Cover: "),
                    item.cover,
                    item.detail
                )));
                while let Some(c) = self.badges.first_child() {
                    self.badges.remove(&c);
                }
                if item.burst.is_some() {
                    self.badges.append(&badge(
                        Some(&gettext("Burst")),
                        Some("view-paged-symbolic"),
                        &gettext("Burst"),
                    ));
                }
                if item.is_motion {
                    self.badges.append(&badge(
                        None,
                        Some("camera-video-symbolic"),
                        &gettext("Live Photo"),
                    ));
                }
                if item.hdr.to_lowercase().contains("hdr") {
                    self.badges.append(&badge(Some("HDR"), None, &item.hdr));
                }
                if item.capture_time_missing == Some(true) {
                    self.badges.append(&badge(
                        Some(&gettext("Missing Date")),
                        Some("alarm-symbolic"),
                        &gettext("Missing Date"),
                    ));
                }
                let step = app.flow.get().step();
                let reviews = matches!(step, Step::Preview | Step::Results);
                self.check.set_visible(!reviews);
                self.status_icon.set_visible(reviews);
                if reviews {
                    let (text, icon, error) = review_text(app, item);
                    self.caption.set_label(&text);
                    self.status_icon.set_icon_name(Some(icon));
                    if error {
                        self.caption.add_css_class("error");
                    } else {
                        self.caption.remove_css_class("error");
                    }
                } else {
                    self.caption.remove_css_class("error");
                    let selected = app
                        .w
                        .queue
                        .selection
                        .selected_item()
                        .and_downcast::<glib::BoxedAnyObject>()
                        .map(|o| *o.borrow::<usize>())
                        == Some(index);
                    self.caption.set_label(&if selected {
                        gettext("Current preview")
                    } else if item.manufacturer.is_empty() || item.manufacturer == "未知" {
                        String::new()
                    } else {
                        item.manufacturer.clone()
                    });
                    self.check.set_active(app.queue.borrow().is_checked(index));
                    self.check.set_sensitive(!app.busy());
                }
                let s = app.settings.borrow();
                self.cover.set_visible(
                    !reviews
                        && item.is_motion
                        && s.direction == Direction::ToApple
                        && s.export_mode != ExportMode::AuxiliaryOnly,
                );
                self.cover.set_sensitive(!app.busy());
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

fn factory(app: &Shared, grid: bool) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let a = app.clone();
    factory.connect_setup(move |_, object| {
        let Some(list_item) = object.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let cell = Cell_::new(grid);
        list_item.set_child(Some(&cell.root));

        // The check mark includes or excludes; it never inspects.
        let app = a.clone();
        let c = Rc::downgrade(&cell);
        cell.check.connect_toggled(move |check| {
            let Some(cell) = c.upgrade() else { return };
            if cell.updating.get() || app.binding.get() || app.busy() {
                return;
            }
            if let Some(index) = cell.index.get() {
                app.queue.borrow_mut().set_checked(index, check.is_active());
                super::sync(&app);
            }
        });
        // Ctrl-click toggles inclusion instead of inspecting (macOS ⌘-click).
        let gesture = gtk::GestureClick::new();
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        let app = a.clone();
        let c = Rc::downgrade(&cell);
        gesture.connect_pressed(move |g, _, _, _| {
            let ctrl = g
                .current_event_state()
                .contains(gdk::ModifierType::CONTROL_MASK);
            let step = app.flow.get().step();
            if !ctrl || matches!(step, Step::Preview | Step::Results) || app.busy() {
                return;
            }
            let Some(cell) = c.upgrade() else { return };
            if let Some(index) = cell.index.get() {
                g.set_state(gtk::EventSequenceState::Claimed);
                app.queue.borrow_mut().toggle(index);
                super::sync(&app);
            }
        });
        cell.root.add_controller(gesture);
        let app = a.clone();
        let c = Rc::downgrade(&cell);
        cell.cover.connect_clicked(move |_| {
            let Some(cell) = c.upgrade() else { return };
            if let Some(index) = cell.index.get() {
                super::cover::present(&app, index);
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

        let app = a.clone();
        list_item.connect_item_notify(move |li| {
            let _keep = &listener;
            let index = li
                .item()
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|o| *o.borrow::<usize>());
            let item = {
                let queue = app.queue.borrow();
                index.and_then(|i| queue.get(i).cloned().map(|item| (i, item)))
            };
            let thumb = item
                .as_ref()
                .and_then(|(_, it)| app.thumbs.cached(&it.source));
            if let (Some((_, it)), None) = (&item, &thumb) {
                app.thumbs.request(&it.source);
            }
            cell.show(&app, item.as_ref().map(|(i, it)| (*i, it)), thumb);
        });
    });
    factory
}

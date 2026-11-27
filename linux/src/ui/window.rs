//! Window layout, header bars, actions and breakpoints.

use super::{inspector, sidebar, App, Shared, Widgets};
use crate::config::{APP_ID, NATIVE_CODECS, VERSION};
use crate::i18n::{fill, gettext, ngettext};
use crate::model::claims;
use crate::model::queue::{Category, Filter};
use crate::model::settings::Direction;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b
}

fn primary_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let section = gio::Menu::new();
    section.append(Some(&gettext("_Export Settings")), Some("win.preferences"));
    section.append(Some(&gettext("_Keyboard Shortcuts")), Some("win.shortcuts"));
    section.append(Some(&gettext("_About MomentStays")), Some("app.about"));
    menu.append_section(None, &section);
    menu
}

fn filter_title(filter: Filter) -> String {
    match filter {
        Filter::All => gettext("All"),
        Filter::Only(Category::Live) => gettext("Live"),
        Filter::Only(Category::Hdr) => gettext("HDR"),
        Filter::Only(Category::Burst) => gettext("Burst"),
        Filter::Only(Category::Attention) => gettext("Needs Attention"),
    }
}

pub fn build(application: &adw::Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title(gettext("MomentStays"))
        .default_width(1180)
        .default_height(760)
        .width_request(360)
        .height_request(294)
        .build();

    // ---- Sidebar pane: source folders --------------------------------
    let add_folder = icon_button("list-add-symbolic", &gettext("Add Source Folder"));
    add_folder.set_action_name(Some("win.add-folder"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&primary_menu())
        .tooltip_text(gettext("Main Menu"))
        .primary(true)
        .build();
    let sidebar_header = adw::HeaderBar::new();
    sidebar_header.pack_start(&add_folder);
    sidebar_header.pack_end(&menu_button);
    sidebar_header.set_title_widget(Some(&adw::WindowTitle::new(&gettext("Sources"), "")));

    let folder_list = gtk::ListBox::new();
    folder_list.add_css_class("navigation-sidebar");
    folder_list.set_selection_mode(gtk::SelectionMode::None);
    let sidebar_empty = adw::StatusPage::builder()
        .icon_name("folder-symbolic")
        .title(gettext("No Source Folders"))
        .description(gettext(
            "Add the folders that hold your photos. Subfolders are included.",
        ))
        .build();
    sidebar_empty.add_css_class("compact");
    let add_first = gtk::Button::with_mnemonic(&gettext("_Add Folder…"));
    add_first.add_css_class("pill");
    add_first.add_css_class("suggested-action");
    add_first.set_halign(gtk::Align::Center);
    add_first.set_action_name(Some("win.add-folder"));
    sidebar_empty.set_child(Some(&add_first));
    let sidebar_stack = gtk::Stack::new();
    sidebar_stack.add_named(&sidebar_empty, Some("empty"));
    sidebar_stack.add_named(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&folder_list)
            .build(),
        Some("list"),
    );
    let read_only = gtk::Label::builder()
        .label(gettext(claims::SOURCES_ARE_READ_ONLY))
        .wrap(true)
        .xalign(0.0)
        .margin_start(12)
        .margin_end(12)
        .margin_top(6)
        .margin_bottom(12)
        .build();
    read_only.add_css_class("caption");
    read_only.add_css_class("dim-label");
    let sidebar_view = adw::ToolbarView::new();
    sidebar_view.add_top_bar(&sidebar_header);
    sidebar_view.set_content(Some(&sidebar_stack));
    sidebar_view.add_bottom_bar(&read_only);

    // ---- Content pane: header, filters, grid --------------------------
    let to_apple = gtk::ToggleButton::with_label(&gettext("Android → Apple"));
    let to_android = gtk::ToggleButton::with_label(&gettext("Apple → Android"));
    to_android.set_group(Some(&to_apple));
    to_apple.set_active(true);
    to_apple.set_tooltip_text(Some(&gettext(
        "Convert Android Motion Photos to Apple Live Photos",
    )));
    to_android.set_tooltip_text(Some(&gettext(
        "Convert Apple Live Photos to Android Motion Photos",
    )));
    let direction = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    direction.add_css_class("linked");
    direction.append(&to_apple);
    direction.append(&to_android);

    let sidebar_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-symbolic")
        .tooltip_text(gettext("Toggle Sources"))
        .build();
    let search_toggle = gtk::ToggleButton::builder()
        .icon_name("system-search-symbolic")
        .tooltip_text(gettext("Search Photos"))
        .build();
    let start_button = gtk::Button::with_mnemonic(&gettext("_Start"));
    start_button.add_css_class("suggested-action");
    start_button.set_action_name(Some("win.start"));
    start_button.set_tooltip_text(Some(&gettext("Convert the included photos")));
    let output_label = gtk::Label::builder()
        .label(gettext("Output Folder…"))
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .max_width_chars(16)
        .build();
    let output_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    output_content.append(&gtk::Image::from_icon_name("folder-symbolic"));
    output_content.append(&output_label);
    let output_button = gtk::Button::builder()
        .child(&output_content)
        .action_name("win.choose-output")
        .tooltip_text(gettext("Choose Output Folder"))
        .build();
    let settings_button = icon_button("emblem-system-symbolic", &gettext("Export Settings"));
    settings_button.set_action_name(Some("win.preferences"));
    let inspector_toggle = gtk::ToggleButton::builder()
        .icon_name("sidebar-show-right-symbolic")
        .tooltip_text(gettext("Toggle Details"))
        .build();

    let content_header = adw::HeaderBar::new();
    content_header.pack_start(&sidebar_toggle);
    content_header.pack_start(&search_toggle);
    content_header.set_title_widget(Some(&direction));
    content_header.pack_end(&start_button);
    content_header.pack_end(&output_button);
    content_header.pack_end(&settings_button);

    let search_entry = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search file names, folders or devices"))
        .hexpand(true)
        .build();
    let search_clamp = adw::Clamp::builder()
        .maximum_size(480)
        .child(&search_entry)
        .build();
    let search_bar = gtk::SearchBar::builder().child(&search_clamp).build();
    search_bar.connect_entry(&search_entry);
    search_bar.set_key_capture_widget(Some(&window));
    search_toggle
        .bind_property("active", &search_bar, "search-mode-enabled")
        .bidirectional()
        .build();

    let filter_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    filter_box.set_margin_start(12);
    filter_box.set_margin_end(12);
    filter_box.set_margin_top(6);
    filter_box.set_margin_bottom(6);
    let mut filters = Vec::new();
    let mut first: Option<gtk::ToggleButton> = None;
    for filter in [
        Filter::All,
        Filter::Only(Category::Live),
        Filter::Only(Category::Hdr),
        Filter::Only(Category::Burst),
        Filter::Only(Category::Attention),
    ] {
        let b = gtk::ToggleButton::with_label(&filter_title(filter));
        b.add_css_class("flat");
        b.add_css_class("filter-chip");
        if let Some(f) = &first {
            b.set_group(Some(f));
        } else {
            b.set_active(true);
            first = Some(b.clone());
        }
        filter_box.append(&b);
        filters.push((filter, b));
    }
    let include_menu = gio::Menu::new();
    include_menu.append(
        Some(&gettext("Include All Shown")),
        Some("win.include-shown"),
    );
    include_menu.append(
        Some(&gettext("Exclude All Shown")),
        Some("win.exclude-shown"),
    );
    let include_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&include_menu)
        .tooltip_text(gettext("Include or Exclude"))
        .css_classes(["flat"])
        .hexpand(true)
        .halign(gtk::Align::End)
        .build();
    filter_box.append(&include_button);
    let filter_scroll = gtk::ScrolledWindow::builder()
        .vscrollbar_policy(gtk::PolicyType::Never)
        .child(&filter_box)
        .build();

    let banner = adw::Banner::new(&gettext(claims::NO_NATIVE_CODECS));
    banner.set_revealed(!NATIVE_CODECS);

    let empty_page = adw::StatusPage::builder()
        .icon_name(APP_ID)
        .title(gettext("Add Photos to Convert"))
        .vexpand(true)
        .build();
    let scanning = adw::StatusPage::builder()
        .title(gettext("Scanning Photos…"))
        .paintable(&adw::SpinnerPaintable::new(None::<&gtk::Widget>))
        .build();
    let no_match = adw::StatusPage::builder()
        .icon_name("edit-find-symbolic")
        .title(gettext("No Matching Photos"))
        .description(gettext("Try another filter or search."))
        .build();

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
    grid.add_css_class("photo-grid");
    let grid_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&grid)
        .build();

    let content_stack = gtk::Stack::new();
    content_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    content_stack.add_named(&empty_page, Some("empty"));
    content_stack.add_named(&scanning, Some("scanning"));
    content_stack.add_named(&no_match, Some("no-match"));
    content_stack.add_named(&grid_scroll, Some("grid"));

    let status_label = gtk::Label::builder().xalign(0.0).hexpand(true).build();
    status_label.add_css_class("caption");
    let progress_bar = gtk::ProgressBar::builder()
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let stop_button = gtk::Button::with_mnemonic(&gettext("Sto_p"));
    stop_button.add_css_class("destructive-action");
    stop_button.set_action_name(Some("win.stop"));
    let progress_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    progress_box.append(&progress_bar);
    progress_box.append(&stop_button);
    progress_box.set_visible(false);
    let status_bar = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    status_bar.set_margin_start(12);
    status_bar.set_margin_end(12);
    status_bar.set_margin_top(6);
    status_bar.set_margin_bottom(6);
    status_bar.append(&status_label);
    status_bar.append(&progress_box);

    let content_view = adw::ToolbarView::new();
    content_view.add_top_bar(&content_header);
    content_view.add_top_bar(&search_bar);
    content_view.add_top_bar(&filter_scroll);
    content_view.add_top_bar(&banner);
    content_view.set_content(Some(&content_stack));
    content_view.add_bottom_bar(&status_bar);
    content_view.set_top_bar_style(adw::ToolbarStyle::Raised);

    // ---- Inspector pane ------------------------------------------------
    let inspector = inspector::Inspector::new();
    let inspector_header = adw::HeaderBar::new();
    inspector_header.set_title_widget(Some(&adw::WindowTitle::new(&gettext("Details"), "")));
    let inspector_view = adw::ToolbarView::new();
    inspector_view.add_top_bar(&inspector_header);
    inspector_view.set_content(Some(&inspector.root));
    content_header.pack_end(&inspector_toggle);

    // ---- Split views and breakpoints -----------------------------------
    let inner = adw::OverlaySplitView::builder()
        .content(&content_view)
        .sidebar(&inspector_view)
        .sidebar_position(gtk::PackType::End)
        .min_sidebar_width(280.0)
        .max_sidebar_width(360.0)
        .show_sidebar(false)
        .build();
    let outer = adw::OverlaySplitView::builder()
        .content(&inner)
        .sidebar(&sidebar_view)
        .min_sidebar_width(220.0)
        .max_sidebar_width(300.0)
        .build();
    sidebar_toggle
        .bind_property("active", &outer, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();
    outer.set_show_sidebar(true);
    inspector_toggle
        .bind_property("active", &inner, "show-sidebar")
        .bidirectional()
        .sync_create()
        .build();

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&outer));
    window.set_content(Some(&toasts));

    // Medium width: the inspector overlays the grid.
    if let Ok(medium) = adw::BreakpointCondition::parse("max-width: 980sp") {
        let bp = adw::Breakpoint::new(medium);
        bp.add_setters(&[(&inner, "collapsed", true.to_value())]);
        window.add_breakpoint(bp);
    }
    // Narrow (phone-sized): both panes overlay and the output folder shows
    // as an icon only.
    if let Ok(narrow) = adw::BreakpointCondition::parse("max-width: 640sp") {
        let bp = adw::Breakpoint::new(narrow);
        bp.add_setters(&[
            (
                &inner.clone().upcast::<glib::Object>(),
                "collapsed",
                true.to_value(),
            ),
            (
                &outer.clone().upcast::<glib::Object>(),
                "collapsed",
                true.to_value(),
            ),
            (
                &output_label.clone().upcast::<glib::Object>(),
                "visible",
                false.to_value(),
            ),
            (
                &search_toggle.clone().upcast::<glib::Object>(),
                "visible",
                false.to_value(),
            ),
        ]);
        window.add_breakpoint(bp);
    }

    let app: Shared = Rc::new(App {
        w: Widgets {
            window: window.clone(),
            toasts,
            inner,
            folder_list,
            sidebar_stack,
            to_apple,
            to_android,
            output_button,
            output_label,
            start_button,
            search_bar,
            search_entry,
            filters,
            content_stack,
            empty_page,
            store,
            selection,
            status_label,
            progress_bar,
            progress_box,
            stop_button,
            inspector,
        },
        sources: RefCell::default(),
        queue: RefCell::default(),
        settings: RefCell::default(),
        output: RefCell::default(),
        scan_generation: Cell::new(0),
        scanning: Cell::new(false),
        running: RefCell::default(),
        monitors: RefCell::default(),
        rescan_timer: RefCell::default(),
        binding: Cell::new(false),
        thumbs: Rc::default(),
    });

    grid.set_factory(Some(&super::grid::factory(&app)));
    connect(&app);
    install_actions(&app);
    sidebar::refresh(&app);
    super::grid::refresh(&app);
    window
}

fn connect(app: &Shared) {
    for (filter, button) in &app.w.filters {
        let filter = *filter;
        let a = app.clone();
        button.connect_toggled(move |b| {
            if b.is_active() {
                a.queue.borrow_mut().set_filter(filter);
                super::grid::refresh(&a);
            }
        });
    }
    let a = app.clone();
    app.w.search_entry.connect_search_changed(move |e| {
        a.queue.borrow_mut().set_search(&e.text());
        super::grid::refresh(&a);
    });
    let a = app.clone();
    app.w.to_apple.connect_toggled(move |b| {
        let direction = if b.is_active() {
            Direction::ToApple
        } else {
            Direction::ToAndroid
        };
        if a.settings.borrow().direction != direction {
            a.settings.borrow_mut().direction = direction;
            super::grid::scan(&a);
        }
    });
    let a = app.clone();
    app.w.selection.connect_selected_item_notify(move |s| {
        let index = s
            .selected_item()
            .and_downcast::<glib::BoxedAnyObject>()
            .map(|o| *o.borrow::<usize>());
        let queue = a.queue.borrow();
        a.w.inspector
            .show(index.and_then(|i| queue.get(i)), &a.thumbs);
        if index.is_some() && !a.w.inner.is_collapsed() {
            a.w.inner.set_show_sidebar(true);
        }
    });
}

fn install_actions(app: &Shared) {
    let window = &app.w.window;
    let add = |name: &str, f: Box<dyn Fn()>| {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(move |_, _| f());
        window.add_action(&action);
    };
    let a = app.clone();
    add("add-folder", Box::new(move || sidebar::choose_folder(&a)));
    let a = app.clone();
    add(
        "choose-output",
        Box::new(move || super::run::choose_output(&a, false)),
    );
    let a = app.clone();
    add("start", Box::new(move || super::run::start(&a)));
    let a = app.clone();
    add("stop", Box::new(move || super::run::stop(&a)));
    let a = app.clone();
    add("preferences", Box::new(move || super::prefs::present(&a)));
    let a = app.clone();
    add(
        "shortcuts",
        Box::new(move || shortcuts_window(&a.w.window).present()),
    );
    let a = app.clone();
    add(
        "search",
        Box::new(move || {
            let bar = &a.w.search_bar;
            bar.set_search_mode(!bar.is_search_mode());
        }),
    );
    let a = app.clone();
    add(
        "include-shown",
        Box::new(move || {
            a.queue.borrow_mut().set_visible_included(true);
            super::grid::refresh(&a);
        }),
    );
    let a = app.clone();
    add(
        "exclude-shown",
        Box::new(move || {
            a.queue.borrow_mut().set_visible_included(false);
            super::grid::refresh(&a);
        }),
    );
    if let Some(gtk_app) = window.application() {
        gtk_app.set_accels_for_action("win.add-folder", &["<Control>o"]);
        gtk_app.set_accels_for_action("win.search", &["<Control>f"]);
        gtk_app.set_accels_for_action("win.preferences", &["<Control>comma"]);
        gtk_app.set_accels_for_action("win.start", &["<Control>Return"]);
        gtk_app.set_accels_for_action("win.shortcuts", &["<Control>question"]);
        gtk_app.set_accels_for_action("app.quit", &["<Control>q"]);
    }
}

/// Status line under the grid: how many photos are included.
pub(crate) fn update_status(app: &App) {
    let counts = app.queue.borrow().counts();
    for (filter, button) in &app.w.filters {
        button.set_label(&format!(
            "{}  {}",
            filter_title(*filter),
            counts.of(*filter)
        ));
    }
    let text = if counts.all == 0 {
        String::new()
    } else {
        fill(
            &ngettext(
                "{included} of {all} photo included",
                "{included} of {all} photos included",
                counts.all as u32,
            ),
            &[
                ("included", &counts.included.to_string()),
                ("all", &counts.all.to_string()),
            ],
        )
    };
    if !app.busy() {
        app.w.status_label.set_label(&text);
    }
    let ready = counts.included > 0 && !app.scanning.get() && !app.busy();
    if let Some(action) = app
        .w
        .window
        .lookup_action("start")
        .and_downcast::<gio::SimpleAction>()
    {
        action.set_enabled(ready);
    }
    let direction = app.settings.borrow().direction;
    let summary = match direction {
        Direction::ToApple => format!(
            "{}\n\n{}",
            gettext(claims::TO_APPLE_SUMMARY),
            gettext(claims::BURST_CLAIM)
        ),
        Direction::ToAndroid => gettext(claims::TO_ANDROID_SUMMARY),
    };
    app.w.empty_page.set_description(Some(&summary));
}

fn shortcuts_window(parent: &adw::ApplicationWindow) -> gtk::ShortcutsWindow {
    let group = gtk::ShortcutsGroup::builder()
        .title(gettext("General"))
        .build();
    for (accel, title) in [
        ("<Control>o", gettext("Add Source Folder")),
        ("<Control>f", gettext("Search Photos")),
        ("<Control>Return", gettext("Start Conversion")),
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

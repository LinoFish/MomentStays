//! MomentStays (留刻) for GNOME.

mod config;
mod engine;
mod i18n;
mod model;
mod ui;

use adw::prelude::*;
use gtk::{gio, glib};

fn window(app: &adw::Application) -> gtk::Window {
    app.active_window()
        .unwrap_or_else(|| ui::build(app).upcast())
}

fn main() -> glib::ExitCode {
    i18n::init();
    gio::resources_register_include!("momentstays.gresource").expect("bundled GResource registers");
    glib::set_application_name(&i18n::gettext("MomentStays"));

    let app = adw::Application::builder()
        .application_id(config::APP_ID)
        .resource_base_path(config::RESOURCE_BASE)
        .flags(gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    // The brand accent replaces the system accent (style.css / style-dark.css).
    app.connect_startup(|app| {
        let quit = gio::SimpleAction::new("quit", None);
        let a = app.clone();
        quit.connect_activate(move |_, _| a.quit());
        app.add_action(&quit);
        let about = gio::SimpleAction::new("about", None);
        let a = app.clone();
        about.connect_activate(move |_, _| {
            if let Some(window) = a.active_window() {
                ui::about(&window);
            }
        });
        app.add_action(&about);
    });
    app.connect_activate(|app| {
        window(app).present();
    });
    // `momentstays FOLDER…` (or a file manager "Open With") adds source folders.
    app.connect_open(|app, files, _| {
        let window = window(app);
        for file in files {
            if let Some(path) = file.path().filter(|p| p.is_dir()) {
                let arg = path.to_string_lossy().to_variant();
                WidgetExt::activate_action(&window, "win.add-folder-path", Some(&arg)).ok();
            }
        }
        window.present();
    });
    app.run()
}

//! Export settings (AdwPreferencesDialog). Changes apply immediately.

use super::Shared;
use crate::i18n::gettext;
use crate::model::claims;
use crate::model::settings::{Container, Direction, ExistingOutput, Vendor};
use adw::prelude::*;

fn vendor_title(v: Vendor) -> String {
    match v {
        Vendor::Google => gettext("Google / Generic (recommended for newer Honor)"),
        Vendor::Xiaomi => gettext("Xiaomi"),
        Vendor::Samsung => gettext("Samsung"),
        Vendor::Huawei => gettext("Huawei"),
        Vendor::Honor => gettext("Honor (fallback)"),
        Vendor::Oppo => "OPPO".to_string(),
        Vendor::Vivo => "vivo V2".to_string(),
    }
}

const EXISTING: [ExistingOutput; 3] = [
    ExistingOutput::Resume,
    ExistingOutput::Skip,
    ExistingOutput::Overwrite,
];

fn existing_title(e: ExistingOutput) -> String {
    match e {
        ExistingOutput::Resume => gettext("Verify and Resume"),
        ExistingOutput::Skip => gettext("Skip"),
        ExistingOutput::Overwrite => gettext("Overwrite"),
    }
}

fn combo(title: &str, items: &[String], selected: usize) -> adw::ComboRow {
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&refs))
        .build();
    row.set_selected(selected as u32);
    row
}

pub fn present(app: &Shared) {
    let s = app.settings.borrow().clone();
    let page = adw::PreferencesPage::builder()
        .title(gettext("Export"))
        .icon_name("document-save-symbolic")
        .build();

    // Apple → Android target.
    let target = adw::PreferencesGroup::builder()
        .title(gettext("Android Target"))
        .description(gettext(
            "Used for Apple → Android. Change the direction in the header bar.",
        ))
        .build();
    let vendors: Vec<String> = Vendor::ALL.iter().map(|v| vendor_title(*v)).collect();
    let vendor_row = combo(
        &gettext("Brand"),
        &vendors,
        Vendor::ALL.iter().position(|v| *v == s.vendor).unwrap_or(0),
    );
    let format_row = combo(
        &gettext("Photo Format"),
        &["JPEG".to_string(), "HEIC".to_string()],
        usize::from(s.effective_container() == Container::Heic),
    );
    let heic_note = gettext("HEIC is available for Samsung only.");
    format_row.set_sensitive(s.vendor.supports_heic());
    if !s.vendor.supports_heic() {
        format_row.set_subtitle(&heic_note);
    }
    let preserve = adw::SwitchRow::builder()
        .title(gettext("Keep Original JPEG Encoding"))
        .subtitle(gettext("The main image is not recompressed when kept."))
        .active(s.preserve_jpeg)
        .build();
    target.add(&vendor_row);
    target.add(&format_row);
    target.add(&preserve);
    target.set_sensitive(s.direction == Direction::ToAndroid);

    // Output.
    let output = adw::PreferencesGroup::builder()
        .title(gettext("Output"))
        .build();
    let existing: Vec<String> = EXISTING.iter().map(|e| existing_title(*e)).collect();
    let existing_row = combo(
        &gettext("When a Result Exists"),
        &existing,
        EXISTING
            .iter()
            .position(|e| *e == s.existing_output)
            .unwrap_or(0),
    );
    existing_row.set_subtitle(&gettext(
        "Verify and Resume keeps verified results and redoes incomplete ones.",
    ));
    let reports = adw::SwitchRow::builder()
        .title(gettext("Write Report Files"))
        .subtitle(gettext(
            "Save the manifest and JSON report next to the results.",
        ))
        .active(s.export_data_files)
        .build();
    let exif = adw::SwitchRow::builder()
        .title(gettext("Use EXIF Time for File Dates"))
        .subtitle(gettext(
            "Off by default. Otherwise output files keep the source file's modification time.",
        ))
        .active(s.exif_file_times)
        .build();
    output.add(&existing_row);
    output.add(&reports);
    output.add(&exif);

    // Bursts: what the code supports (no switch in this milestone).
    let bursts = adw::PreferencesGroup::builder()
        .title(gettext("Bursts"))
        .description(gettext(claims::BURST_CLAIM))
        .build();

    // Experimental.
    let experimental = adw::PreferencesGroup::builder()
        .title(gettext("Experimental"))
        .build();
    let private_hdr = adw::SwitchRow::builder()
        .title(gettext(claims::PRIVATE_HDR_TITLE))
        .subtitle(gettext(claims::PRIVATE_HDR_CLAIM))
        .active(s.private_hdr)
        .build();
    experimental.add(&private_hdr);

    page.add(&target);
    page.add(&output);
    page.add(&bursts);
    page.add(&experimental);
    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Export Settings"))
        .build();
    dialog.add(&page);

    let a = app.clone();
    let fr = format_row.clone();
    vendor_row.connect_selected_notify(move |row| {
        let vendor = Vendor::ALL
            .get(row.selected() as usize)
            .copied()
            .unwrap_or_default();
        a.settings.borrow_mut().set_vendor(vendor);
        fr.set_sensitive(vendor.supports_heic());
        fr.set_subtitle(if vendor.supports_heic() {
            ""
        } else {
            &heic_note
        });
        if !vendor.supports_heic() {
            fr.set_selected(0);
        }
    });
    let a = app.clone();
    format_row.connect_selected_notify(move |row| {
        a.settings.borrow_mut().container = if row.selected() == 1 {
            Container::Heic
        } else {
            Container::Jpeg
        };
    });
    let a = app.clone();
    preserve.connect_active_notify(move |r| a.settings.borrow_mut().preserve_jpeg = r.is_active());
    let a = app.clone();
    existing_row.connect_selected_notify(move |row| {
        a.settings.borrow_mut().existing_output = EXISTING
            .get(row.selected() as usize)
            .copied()
            .unwrap_or_default();
    });
    let a = app.clone();
    reports
        .connect_active_notify(move |r| a.settings.borrow_mut().export_data_files = r.is_active());
    let a = app.clone();
    exif.connect_active_notify(move |r| a.settings.borrow_mut().exif_file_times = r.is_active());
    let a = app.clone();
    private_hdr.connect_active_notify(move |r| a.settings.borrow_mut().private_hdr = r.is_active());

    dialog.set_sensitive(!app.busy());
    dialog.present(Some(&app.w.window));
}

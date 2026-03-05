use gtk4::prelude::*;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::Config;
use crate::history::{History, SearchQuery};

/// Widget handles needed for signal connection and refresh.
pub struct HistoryPageWidgets {
    pub search_entry: gtk4::SearchEntry,
    pub from_entry: gtk4::Entry,
    pub to_entry: gtk4::Entry,
    pub list_box: gtk4::ListBox,
    pub history: Rc<RefCell<History>>,
}

/// Build and return the history PreferencesPage.
pub fn build_history_page(config: &Config) -> (libadwaita::PreferencesPage, HistoryPageWidgets) {
    let history_dir = config.history_dir();
    let max_entries = config.history.max_entries;
    let history = History::load(&history_dir, max_entries).unwrap_or_else(|_| {
        History::load(&std::env::temp_dir().join("koe-history-fallback"), max_entries)
            .unwrap_or_else(|_| panic!("Failed to load history"))
    });
    let history = Rc::new(RefCell::new(history));

    let page = libadwaita::PreferencesPage::builder()
        .title("History")
        .icon_name("document-open-recent-symbolic")
        .build();

    // ── Search section ──────────────────────────────────────────────────────

    let search_group = libadwaita::PreferencesGroup::builder()
        .title("Search")
        .build();

    let search_entry = gtk4::SearchEntry::builder()
        .placeholder_text("Search transcriptions...")
        .hexpand(true)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();

    let search_row = libadwaita::ActionRow::builder().build();
    search_row.set_activatable_widget(Some(&search_entry));
    search_row.add_suffix(&search_entry);
    // Remove the title from the row since search entry is self-explanatory
    search_group.add(&search_row);

    // Date filter rows
    let from_entry = gtk4::Entry::builder()
        .placeholder_text("YYYY-MM-DD")
        .hexpand(true)
        .build();
    let from_row = libadwaita::ActionRow::builder()
        .title("From date")
        .build();
    from_row.add_suffix(&from_entry);
    search_group.add(&from_row);

    let to_entry = gtk4::Entry::builder()
        .placeholder_text("YYYY-MM-DD")
        .hexpand(true)
        .build();
    let to_row = libadwaita::ActionRow::builder()
        .title("To date")
        .build();
    to_row.add_suffix(&to_entry);
    search_group.add(&to_row);

    page.add(&search_group);

    // ── History list section ────────────────────────────────────────────────

    let list_group = libadwaita::PreferencesGroup::builder()
        .title("Transcription History")
        .build();

    let list_box = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .css_classes(vec!["boxed-list".to_string()])
        .build();

    list_group.add(&list_box);
    page.add(&list_group);

    // ── Export / Actions section ────────────────────────────────────────────

    let actions_group = libadwaita::PreferencesGroup::builder()
        .title("Actions")
        .build();

    // Export CSV button
    let export_csv_row = libadwaita::ActionRow::builder()
        .title("Export as CSV")
        .subtitle("Save transcription history to a CSV file")
        .activatable(true)
        .build();
    let csv_icon = gtk4::Image::from_icon_name("document-save-symbolic");
    export_csv_row.add_suffix(&csv_icon);

    // Export JSON button
    let export_json_row = libadwaita::ActionRow::builder()
        .title("Export as JSON")
        .subtitle("Save transcription history to a JSON file")
        .activatable(true)
        .build();
    let json_icon = gtk4::Image::from_icon_name("document-save-symbolic");
    export_json_row.add_suffix(&json_icon);

    // Clear all button
    let clear_row = libadwaita::ActionRow::builder()
        .title("Clear All History")
        .subtitle("Permanently delete all transcription history")
        .activatable(true)
        .build();
    let clear_icon = gtk4::Image::from_icon_name("edit-delete-symbolic");
    clear_row.add_suffix(&clear_icon);

    actions_group.add(&export_csv_row);
    actions_group.add(&export_json_row);
    actions_group.add(&clear_row);
    page.add(&actions_group);

    // ── Wire up signals ─────────────────────────────────────────────────────

    let widgets = HistoryPageWidgets {
        search_entry: search_entry.clone(),
        from_entry: from_entry.clone(),
        to_entry: to_entry.clone(),
        list_box: list_box.clone(),
        history: history.clone(),
    };

    // Initial population
    refresh_list(&list_box, &history, "", "", "");

    // Search entry changed
    {
        let history_ref = history.clone();
        let list_box_ref = list_box.clone();
        let from_ref = from_entry.clone();
        let to_ref = to_entry.clone();
        search_entry.connect_search_changed(move |entry| {
            let text = entry.text().to_string();
            let from = from_ref.text().to_string();
            let to = to_ref.text().to_string();
            refresh_list(&list_box_ref, &history_ref, &text, &from, &to);
        });
    }

    // From date changed
    {
        let history_ref = history.clone();
        let list_box_ref = list_box.clone();
        let search_ref = search_entry.clone();
        let to_ref = to_entry.clone();
        from_entry.connect_changed(move |entry| {
            let from = entry.text().to_string();
            let text = search_ref.text().to_string();
            let to = to_ref.text().to_string();
            refresh_list(&list_box_ref, &history_ref, &text, &from, &to);
        });
    }

    // To date changed
    {
        let history_ref = history.clone();
        let list_box_ref = list_box.clone();
        let search_ref = search_entry.clone();
        let from_ref = from_entry.clone();
        to_entry.connect_changed(move |entry| {
            let to = entry.text().to_string();
            let text = search_ref.text().to_string();
            let from = from_ref.text().to_string();
            refresh_list(&list_box_ref, &history_ref, &text, &from, &to);
        });
    }

    // Export CSV
    {
        let history_ref = history.clone();
        let page_ref = page.clone();
        export_csv_row.connect_activated(move |_| {
            export_history(&history_ref, &page_ref, ExportFormat::Csv);
        });
    }

    // Export JSON
    {
        let history_ref = history.clone();
        let page_ref = page.clone();
        export_json_row.connect_activated(move |_| {
            export_history(&history_ref, &page_ref, ExportFormat::Json);
        });
    }

    // Clear all
    {
        let history_ref = history.clone();
        let list_box_ref = list_box.clone();
        let page_ref = page.clone();
        clear_row.connect_activated(move |_| {
            show_clear_confirmation(&history_ref, &list_box_ref, &page_ref);
        });
    }

    (page, widgets)
}

/// Refresh the list box based on current search/filter state.
fn refresh_list(
    list_box: &gtk4::ListBox,
    history: &Rc<RefCell<History>>,
    search_text: &str,
    from_str: &str,
    to_str: &str,
) {
    // Remove all existing rows
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let text_filter = if search_text.trim().is_empty() {
        None
    } else {
        Some(search_text.trim().to_string())
    };

    let from_dt = parse_date_str(from_str, false);
    let to_dt = parse_date_str(to_str, true);

    let query = SearchQuery {
        text: text_filter,
        from: from_dt,
        to: to_dt,
    };

    let hist = history.borrow();
    let results = hist.search(&query);

    if results.is_empty() {
        let empty_row = libadwaita::ActionRow::builder()
            .title("No history entries")
            .subtitle("Transcriptions will appear here after recording")
            .build();
        list_box.append(&empty_row);
        return;
    }

    for entry in results {
        let id = entry.id.clone();
        let processed_text = entry.processed_text.clone();
        let timestamp_str = entry
            .timestamp
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        let preview = truncate_str(&processed_text, 80).to_string();

        let row = libadwaita::ActionRow::builder()
            .title(&timestamp_str)
            .subtitle(&preview)
            .build();

        // Copy button
        let copy_btn = gtk4::Button::builder()
            .icon_name("edit-copy-symbolic")
            .tooltip_text("Copy to clipboard")
            .valign(gtk4::Align::Center)
            .css_classes(vec!["flat".to_string()])
            .build();

        let text_for_copy = processed_text.clone();
        copy_btn.connect_clicked(move |btn| {
            let display = gtk4::prelude::WidgetExt::display(btn);
            display.clipboard().set_text(&text_for_copy);
        });

        // Delete button
        let delete_btn = gtk4::Button::builder()
            .icon_name("edit-delete-symbolic")
            .tooltip_text("Delete entry")
            .valign(gtk4::Align::Center)
            .css_classes(vec!["flat".to_string()])
            .build();

        let history_for_delete = history.clone();
        let list_box_for_delete = list_box.clone();
        let row_for_delete = row.clone();
        delete_btn.connect_clicked(move |_| {
            {
                let mut hist_mut = history_for_delete.borrow_mut();
                let _ = hist_mut.delete_entry(&id);
            }
            list_box_for_delete.remove(&row_for_delete);
        });

        row.add_suffix(&copy_btn);
        row.add_suffix(&delete_btn);
        list_box.append(&row);
    }
}

/// Parse a "YYYY-MM-DD" string into a UTC DateTime, returning None if empty or invalid.
/// When `end_of_day` is true, the time is set to 23:59:59 so that the entire day is included
/// (used for "to" date filters).
fn parse_date_str(s: &str, end_of_day: bool) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        .ok()
        .and_then(|d| {
            if end_of_day {
                d.and_hms_opt(23, 59, 59)
            } else {
                d.and_hms_opt(0, 0, 0)
            }
        })
        .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
}

/// Truncate a string to at most `max` bytes, respecting char boundaries.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

enum ExportFormat {
    Csv,
    Json,
}

/// Open a file-chooser dialog and export history to the chosen path.
fn export_history(
    history: &Rc<RefCell<History>>,
    page: &libadwaita::PreferencesPage,
    format: ExportFormat,
) {
    let (filter_name, extension, dialog_title) = match format {
        ExportFormat::Csv => ("CSV files", "csv", "Export history as CSV"),
        ExportFormat::Json => ("JSON files", "json", "Export history as JSON"),
    };

    let dialog = gtk4::FileChooserDialog::builder()
        .title(dialog_title)
        .action(gtk4::FileChooserAction::Save)
        .modal(true)
        .build();

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("Save", gtk4::ResponseType::Accept);
    dialog.set_current_name(&format!("koe-history.{}", extension));

    let file_filter = gtk4::FileFilter::new();
    file_filter.set_name(Some(filter_name));
    file_filter.add_pattern(&format!("*.{}", extension));
    dialog.add_filter(&file_filter);

    // Attach to the nearest window
    if let Some(root) = page.root() {
        if let Some(window) = root.downcast_ref::<gtk4::Window>() {
            dialog.set_transient_for(Some(window));
        }
    }

    let history_ref = history.clone();
    let page_ref = page.clone();
    let ext = extension.to_string();
    dialog.connect_response(move |dlg, response| {
        if response == gtk4::ResponseType::Accept {
            if let Some(file) = dlg.file() {
                if let Some(path) = file.path() {
                    let result = if ext == "csv" {
                        let hist = history_ref.borrow();
                        let mut buf = Vec::new();
                        hist.export_csv(&mut buf)
                            .map_err(|e| e.to_string())
                            .and_then(|_| {
                                std::fs::write(&path, &buf).map_err(|e| e.to_string())
                            })
                    } else {
                        let hist = history_ref.borrow();
                        hist.export_json()
                            .map_err(|e| e.to_string())
                            .and_then(|json| {
                                std::fs::write(&path, json.as_bytes()).map_err(|e| e.to_string())
                            })
                    };

                    match result {
                        Ok(()) => {
                            if let Some(root) = page_ref.root() {
                                if let Some(adw_win) =
                                    root.downcast_ref::<libadwaita::PreferencesWindow>()
                                {
                                    adw_win.add_toast(libadwaita::Toast::new("History exported"));
                                }
                            }
                        }
                        Err(e) => {
                            if let Some(root) = page_ref.root() {
                                if let Some(adw_win) =
                                    root.downcast_ref::<libadwaita::PreferencesWindow>()
                                {
                                    adw_win.add_toast(libadwaita::Toast::new(&format!(
                                        "Export failed: {}",
                                        truncate_str(&e, 60)
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
        dlg.close();
    });

    dialog.present();
}

/// Show a confirmation dialog before clearing all history.
fn show_clear_confirmation(
    history: &Rc<RefCell<History>>,
    list_box: &gtk4::ListBox,
    page: &libadwaita::PreferencesPage,
) {
    let dialog = libadwaita::MessageDialog::builder()
        .heading("Clear All History?")
        .body("This will permanently delete all transcription history. This action cannot be undone.")
        .build();

    dialog.add_response("cancel", "Cancel");
    dialog.add_response("clear", "Clear All");
    dialog.set_response_appearance("clear", libadwaita::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    if let Some(root) = page.root() {
        if let Some(window) = root.downcast_ref::<gtk4::Window>() {
            dialog.set_transient_for(Some(window));
        }
    }

    let history_ref = history.clone();
    let list_box_ref = list_box.clone();
    let page_ref = page.clone();
    dialog.connect_response(None, move |dlg, response| {
        if response == "clear" {
            {
                let mut hist = history_ref.borrow_mut();
                let _ = hist.clear();
            }
            // Remove all rows from list
            while let Some(child) = list_box_ref.first_child() {
                list_box_ref.remove(&child);
            }
            let empty_row = libadwaita::ActionRow::builder()
                .title("No history entries")
                .subtitle("Transcriptions will appear here after recording")
                .build();
            list_box_ref.append(&empty_row);

            if let Some(root) = page_ref.root() {
                if let Some(adw_win) = root.downcast_ref::<libadwaita::PreferencesWindow>() {
                    adw_win.add_toast(libadwaita::Toast::new("History cleared"));
                }
            }
        }
        dlg.close();
    });

    dialog.present();
}

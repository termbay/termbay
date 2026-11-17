//! The macOS menu bar (section 6.6 of `docs/FASE1-PLANO.md`, task B2).
//!
//! Tauri's default menu has "Close Window" on Cmd+W, which would take the
//! key before the page sees it. This menu keeps what macOS users expect
//! (app menu, Edit for Cmd+C/V, Window) and nothing on Cmd+W, Cmd+T or
//! Cmd+D, so the app's own shortcuts reach the page.
//!
//! The layout is plain data ([`LAYOUT`]) so it can be checked by tests
//! without creating native menus.

/// One entry of the menu, all predefined by the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    About,
    Separator,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Zoom,
    Fullscreen,
}

impl Item {
    /// Shortcut macOS gives this predefined item (`muda`, used by Tauri).
    #[cfg(test)]
    pub fn accelerator(self) -> Option<&'static str> {
        match self {
            Self::About | Self::Separator | Self::ShowAll | Self::Zoom => None,
            Self::Hide => Some("Cmd+H"),
            Self::HideOthers => Some("Cmd+Option+H"),
            Self::Quit => Some("Cmd+Q"),
            Self::Undo => Some("Cmd+Z"),
            Self::Redo => Some("Cmd+Shift+Z"),
            Self::Cut => Some("Cmd+X"),
            Self::Copy => Some("Cmd+C"),
            Self::Paste => Some("Cmd+V"),
            Self::SelectAll => Some("Cmd+A"),
            Self::Minimize => Some("Cmd+M"),
            Self::Fullscreen => Some("Ctrl+Cmd+F"),
        }
    }
}

/// Submenus and their items. `None` as the title means the app's name.
pub const LAYOUT: &[(Option<&str>, &[Item])] = &[
    (
        None,
        &[
            Item::About,
            Item::Separator,
            Item::Hide,
            Item::HideOthers,
            Item::ShowAll,
            Item::Separator,
            Item::Quit,
        ],
    ),
    (
        Some("Edit"),
        &[
            Item::Undo,
            Item::Redo,
            Item::Separator,
            Item::Cut,
            Item::Copy,
            Item::Paste,
            Item::SelectAll,
        ],
    ),
    (
        Some("Window"),
        &[
            Item::Minimize,
            Item::Zoom,
            Item::Separator,
            Item::Fullscreen,
        ],
    ),
];

/// Builds [`LAYOUT`] as a native menu (passed to `tauri::Builder::menu`).
#[cfg(target_os = "macos")]
pub fn build<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<tauri::menu::Menu<R>> {
    use tauri::menu::{IsMenuItem, Menu, PredefinedMenuItem, Submenu};

    let app_name = app.package_info().name.clone();
    let mut submenus = Vec::new();
    for (title, items) in LAYOUT {
        let items = items
            .iter()
            .map(|item| match item {
                Item::About => PredefinedMenuItem::about(app, None, None),
                Item::Separator => PredefinedMenuItem::separator(app),
                Item::Hide => PredefinedMenuItem::hide(app, None),
                Item::HideOthers => PredefinedMenuItem::hide_others(app, None),
                Item::ShowAll => PredefinedMenuItem::show_all(app, None),
                Item::Quit => PredefinedMenuItem::quit(app, None),
                Item::Undo => PredefinedMenuItem::undo(app, None),
                Item::Redo => PredefinedMenuItem::redo(app, None),
                Item::Cut => PredefinedMenuItem::cut(app, None),
                Item::Copy => PredefinedMenuItem::copy(app, None),
                Item::Paste => PredefinedMenuItem::paste(app, None),
                Item::SelectAll => PredefinedMenuItem::select_all(app, None),
                Item::Minimize => PredefinedMenuItem::minimize(app, None),
                Item::Zoom => PredefinedMenuItem::maximize(app, None),
                Item::Fullscreen => PredefinedMenuItem::fullscreen(app, None),
            })
            .collect::<tauri::Result<Vec<_>>>()?;
        let refs: Vec<&dyn IsMenuItem<R>> = items.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
        let title = title.unwrap_or(app_name.as_str());
        submenus.push(Submenu::with_items(app, title, true, &refs)?);
    }
    let refs: Vec<&dyn IsMenuItem<R>> = submenus.iter().map(|s| s as &dyn IsMenuItem<R>).collect();
    Menu::with_items(app, &refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accelerators() -> Vec<&'static str> {
        LAYOUT
            .iter()
            .flat_map(|(_, items)| items.iter())
            .filter_map(|item| item.accelerator())
            .collect()
    }

    #[test]
    fn no_item_takes_the_app_shortcuts() {
        let taken = accelerators();
        for key in [
            "Cmd+W",
            "Cmd+T",
            "Cmd+D",
            "Cmd+Shift+D",
            "Cmd+F",
            "Cmd+Shift+O",
        ] {
            assert!(!taken.contains(&key), "{key} is taken by the menu");
        }
        for digit in 1..=9 {
            let key = format!("Cmd+{digit}");
            assert!(!taken.contains(&key.as_str()), "{key} is taken by the menu");
        }
    }

    #[test]
    fn edit_and_quit_keep_their_keys() {
        let taken = accelerators();
        for key in [
            "Cmd+C", "Cmd+V", "Cmd+X", "Cmd+A", "Cmd+Z", "Cmd+Q", "Cmd+M",
        ] {
            assert!(taken.contains(&key), "{key} is missing");
        }
    }
}

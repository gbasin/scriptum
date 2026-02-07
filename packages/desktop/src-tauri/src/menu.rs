use serde::Serialize;
use tauri::{
    menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    AppHandle, Emitter, Manager, Runtime,
};
use tauri_plugin_dialog::{DialogExt, FilePath};

pub const MENU_ACTION_EVENT: &str = "scriptum://menu-action";
pub const IMPORT_DIALOG_SELECTED_EVENT: &str = "scriptum://dialog/import-selected";
pub const EXPORT_DIALOG_SELECTED_EVENT: &str = "scriptum://dialog/export-selected";
pub const MENU_ID_NEW_DOCUMENT: &str = "menu.new-document";
pub const MENU_ID_SAVE_DOCUMENT: &str = "menu.save-document";
pub const MENU_ID_IMPORT_MARKDOWN: &str = "menu.import-markdown";
pub const MENU_ID_EXPORT_MARKDOWN: &str = "menu.export-markdown";
pub const MENU_ID_CLOSE_WINDOW: &str = "menu.close-window";
pub const MENU_ID_QUIT_APP: &str = "menu.quit-app";
pub const MENU_ID_TOGGLE_FULLSCREEN: &str = "menu.toggle-fullscreen";
pub const MENU_ID_OPEN_HELP: &str = "menu.open-help";
pub const MENU_ID_OPEN_ABOUT: &str = "menu.open-about";

pub const MENU_SHORTCUT_NEW_DOCUMENT: &str = "CmdOrCtrl+N";
pub const MENU_SHORTCUT_SAVE_DOCUMENT: &str = "CmdOrCtrl+S";
pub const MENU_SHORTCUT_CLOSE_WINDOW: &str = "CmdOrCtrl+W";
pub const MENU_SHORTCUT_QUIT_APP: &str = "CmdOrCtrl+Q";
pub const MENU_SHORTCUT_TOGGLE_FULLSCREEN: &str = "F11";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewDocument,
    SaveDocument,
    ImportMarkdown,
    ExportMarkdown,
    CloseWindow,
    QuitApp,
    ToggleFullscreen,
    OpenHelp,
    OpenAbout,
}

impl MenuAction {
    fn id(self) -> &'static str {
        match self {
            Self::NewDocument => MENU_ID_NEW_DOCUMENT,
            Self::SaveDocument => MENU_ID_SAVE_DOCUMENT,
            Self::ImportMarkdown => MENU_ID_IMPORT_MARKDOWN,
            Self::ExportMarkdown => MENU_ID_EXPORT_MARKDOWN,
            Self::CloseWindow => MENU_ID_CLOSE_WINDOW,
            Self::QuitApp => MENU_ID_QUIT_APP,
            Self::ToggleFullscreen => MENU_ID_TOGGLE_FULLSCREEN,
            Self::OpenHelp => MENU_ID_OPEN_HELP,
            Self::OpenAbout => MENU_ID_OPEN_ABOUT,
        }
    }

    fn frontend_action(self) -> &'static str {
        match self {
            Self::NewDocument => "new-document",
            Self::SaveDocument => "save-document",
            Self::ImportMarkdown => "import-markdown",
            Self::ExportMarkdown => "export-markdown",
            Self::CloseWindow => "close-window",
            Self::QuitApp => "quit-app",
            Self::ToggleFullscreen => "toggle-fullscreen",
            Self::OpenHelp => "open-help",
            Self::OpenAbout => "open-about",
        }
    }

    fn from_menu_id(menu_id: &str) -> Option<Self> {
        match menu_id {
            MENU_ID_NEW_DOCUMENT => Some(Self::NewDocument),
            MENU_ID_SAVE_DOCUMENT => Some(Self::SaveDocument),
            MENU_ID_IMPORT_MARKDOWN => Some(Self::ImportMarkdown),
            MENU_ID_EXPORT_MARKDOWN => Some(Self::ExportMarkdown),
            MENU_ID_CLOSE_WINDOW => Some(Self::CloseWindow),
            MENU_ID_QUIT_APP => Some(Self::QuitApp),
            MENU_ID_TOGGLE_FULLSCREEN => Some(Self::ToggleFullscreen),
            MENU_ID_OPEN_HELP => Some(Self::OpenHelp),
            MENU_ID_OPEN_ABOUT => Some(Self::OpenAbout),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MenuActionPayload {
    action: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileDialogSelectionPayload {
    path: String,
}

pub fn build_menu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
    let new_document = MenuItemBuilder::with_id(MenuAction::NewDocument.id(), "New")
        .accelerator(MENU_SHORTCUT_NEW_DOCUMENT)
        .build(manager)?;
    let save_document = MenuItemBuilder::with_id(MenuAction::SaveDocument.id(), "Save")
        .accelerator(MENU_SHORTCUT_SAVE_DOCUMENT)
        .build(manager)?;
    let import_markdown =
        MenuItemBuilder::with_id(MenuAction::ImportMarkdown.id(), "Import Markdown...")
            .build(manager)?;
    let export_markdown =
        MenuItemBuilder::with_id(MenuAction::ExportMarkdown.id(), "Export Markdown...")
            .build(manager)?;
    let close_window = MenuItemBuilder::with_id(MenuAction::CloseWindow.id(), "Close Window")
        .accelerator(MENU_SHORTCUT_CLOSE_WINDOW)
        .build(manager)?;
    let quit_app = MenuItemBuilder::with_id(MenuAction::QuitApp.id(), "Quit Scriptum")
        .accelerator(MENU_SHORTCUT_QUIT_APP)
        .build(manager)?;

    let cut = PredefinedMenuItem::cut(manager, None)?;
    let copy = PredefinedMenuItem::copy(manager, None)?;
    let paste = PredefinedMenuItem::paste(manager, None)?;
    let select_all = PredefinedMenuItem::select_all(manager, None)?;
    let toggle_fullscreen =
        MenuItemBuilder::with_id(MenuAction::ToggleFullscreen.id(), "Toggle Full Screen")
            .accelerator(MENU_SHORTCUT_TOGGLE_FULLSCREEN)
            .build(manager)?;
    let open_help =
        MenuItemBuilder::with_id(MenuAction::OpenHelp.id(), "Scriptum Help").build(manager)?;
    let open_about =
        MenuItemBuilder::with_id(MenuAction::OpenAbout.id(), "About Scriptum").build(manager)?;
    let separator = PredefinedMenuItem::separator(manager)?;

    let file_menu = SubmenuBuilder::new(manager, "File")
        .items(&[
            &new_document,
            &save_document,
            &separator,
            &import_markdown,
            &export_markdown,
            &separator,
            &close_window,
            &quit_app,
        ])
        .build()?;

    let edit_menu = SubmenuBuilder::new(manager, "Edit")
        .items(&[&cut, &copy, &paste, &separator, &select_all])
        .build()?;

    let view_menu = SubmenuBuilder::new(manager, "View").items(&[&toggle_fullscreen]).build()?;

    let help_menu =
        SubmenuBuilder::new(manager, "Help").items(&[&open_help, &open_about]).build()?;

    MenuBuilder::new(manager).items(&[&file_menu, &edit_menu, &view_menu, &help_menu]).build()
}

pub fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let Some(action) = MenuAction::from_menu_id(event.id().as_ref()) else {
        return;
    };

    match action {
        MenuAction::ImportMarkdown => open_import_dialog(app),
        MenuAction::ExportMarkdown => open_export_dialog(app),
        MenuAction::CloseWindow => close_main_window(app),
        MenuAction::QuitApp => app.exit(0),
        MenuAction::ToggleFullscreen => toggle_main_window_fullscreen(app),
        _ => emit_menu_action(app, action),
    }
}

fn emit_menu_action<R: Runtime>(app: &AppHandle<R>, action: MenuAction) {
    let _ = app.emit(MENU_ACTION_EVENT, MenuActionPayload { action: action.frontend_action() });
}

fn close_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.close();
    }
}

fn toggle_main_window_fullscreen<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(fullscreen) = window.is_fullscreen() {
            let _ = window.set_fullscreen(!fullscreen);
        }
    }

    emit_menu_action(app, MenuAction::ToggleFullscreen);
}

fn open_import_dialog<R: Runtime>(app: &AppHandle<R>) {
    emit_menu_action(app, MenuAction::ImportMarkdown);

    let app_handle = app.clone();
    app.dialog()
        .file()
        .set_title("Import Markdown")
        .add_filter("Markdown", &["md", "markdown"])
        .pick_file(move |selected| {
            emit_selected_path(&app_handle, IMPORT_DIALOG_SELECTED_EVENT, selected);
        });
}

fn open_export_dialog<R: Runtime>(app: &AppHandle<R>) {
    emit_menu_action(app, MenuAction::ExportMarkdown);

    let app_handle = app.clone();
    app.dialog()
        .file()
        .set_title("Export Markdown")
        .set_file_name("Untitled.md")
        .add_filter("Markdown", &["md", "markdown"])
        .save_file(move |selected| {
            emit_selected_path(&app_handle, EXPORT_DIALOG_SELECTED_EVENT, selected);
        });
}

fn emit_selected_path<R: Runtime>(
    app: &AppHandle<R>,
    event_name: &str,
    selected: Option<FilePath>,
) {
    let Some(selected) = selected else {
        return;
    };

    let _ = app.emit(event_name, FileDialogSelectionPayload { path: selected.to_string() });
}

#[cfg(test)]
mod tests {
    use super::MenuAction;

    #[test]
    fn resolves_known_menu_ids() {
        assert_eq!(MenuAction::from_menu_id("menu.new-document"), Some(MenuAction::NewDocument));
        assert_eq!(MenuAction::from_menu_id("menu.save-document"), Some(MenuAction::SaveDocument));
        assert_eq!(MenuAction::from_menu_id("menu.close-window"), Some(MenuAction::CloseWindow));
        assert_eq!(
            MenuAction::from_menu_id("menu.toggle-fullscreen"),
            Some(MenuAction::ToggleFullscreen)
        );
        assert_eq!(
            MenuAction::from_menu_id("menu.export-markdown"),
            Some(MenuAction::ExportMarkdown)
        );
        assert_eq!(
            MenuAction::from_menu_id("menu.import-markdown"),
            Some(MenuAction::ImportMarkdown)
        );
        assert_eq!(MenuAction::from_menu_id("menu.quit-app"), Some(MenuAction::QuitApp));
    }

    #[test]
    fn returns_none_for_unknown_menu_ids() {
        assert_eq!(MenuAction::from_menu_id("menu.noop"), None);
        assert_eq!(MenuAction::from_menu_id(""), None);
    }

    #[test]
    fn required_shortcut_actions_are_stable() {
        assert_eq!(MenuAction::NewDocument.id(), "menu.new-document");
        assert_eq!(MenuAction::SaveDocument.id(), "menu.save-document");
        assert_eq!(MenuAction::CloseWindow.id(), "menu.close-window");
        assert_eq!(MenuAction::ToggleFullscreen.id(), "menu.toggle-fullscreen");
        assert_eq!(MenuAction::QuitApp.id(), "menu.quit-app");
    }
}

//! The macOS application menu bar.
//!
//! GPUI never installs a main menu on its own, so `NSApp.mainMenu` stays nil
//! and macOS paints an empty menu bar. Worse, the standard key equivalents that
//! AppKit only provides through menu items are missing too, so a GPUIX app
//! cannot be quit with cmd-q, hidden with cmd-h, or minimized with cmd-m.
//!
//! `gpui::App::set_menus` reads the key equivalent for each item out of the
//! keymap, so [`init`] must bind the keys before it sets the menus.
//!
//! The default menus have no Edit menu. Apps that want one set their own with
//! [`set_menus`]. A shortcut reaches the window first, as without a menu, and
//! the menu ignores it after. Clicking an item with `keys` presses those keys
//! in the window, so text fields and window key listeners handle a click
//! exactly as they handle the keyboard.

use std::cell::RefCell;
use std::sync::Mutex;

use gpui::{App, KeyBinding, Keystroke, Menu, MenuItem, SystemMenuType};
use napi::threadsafe_function::ThreadsafeFunctionCallMode;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEventType};

use crate::renderer::{MenuActionFn, MenuItemSpec, MenuSpec};

gpui::actions!(
    gpuix_app,
    [
        /// Quit the application.
        Quit,
        /// Hide the application.
        Hide,
        /// Hide every other application.
        HideOthers,
        /// Unhide every other application.
        ShowAll,
        /// Minimize the focused window to the Dock.
        MinimizeWindow,
        /// Toggle the focused window between its standard and zoomed size.
        ZoomWindow,
        /// Close the focused window.
        CloseWindow,
    ]
);

/// Binds the standard macOS shortcuts, registers the app-level handlers, and
/// installs the menu bar. The window-level actions (`MinimizeWindow`,
/// `ZoomWindow`, `CloseWindow`) are handled by the root element in
/// `GpuixView::render`, which is the only place a `Window` exists.
pub(crate) fn init(app_name: &str, cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("cmd-q", Quit, None),
        gpui::KeyBinding::new("cmd-h", Hide, None),
        gpui::KeyBinding::new("cmd-alt-h", HideOthers, None),
        gpui::KeyBinding::new("cmd-m", MinimizeWindow, None),
        gpui::KeyBinding::new("cmd-w", CloseWindow, None),
    ]);

    cx.on_action(|_: &Quit, cx: &mut App| cx.quit());
    cx.on_action(|_: &Hide, cx: &mut App| cx.hide());
    cx.on_action(|_: &HideOthers, cx: &mut App| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx: &mut App| cx.unhide_other_apps());
    cx.on_action(|choice: &MenuChoice, cx: &mut App| choose(choice.index, cx));

    APP_NAME.with(|name| *name.borrow_mut() = app_name.to_string());
    cx.set_menus(default_menus(app_name));
}

/// An item set from JS with [`set_menus`]. `index` points into `CHOICES`.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = gpuix_app, no_json)]
pub(crate) struct MenuChoice {
    index: usize,
}

/// Nothing listens for this one, so GPUI's menu validation
/// (`App::is_action_available`) greys the item out.
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = gpuix_app, no_json)]
pub(crate) struct DisabledMenuChoice {
    index: usize,
}

/// No element has this key context, so a binding in it only gives a menu item
/// its key equivalent and never fires inside the window.
const MENU_ONLY_CONTEXT: &str = "GpuixMenuOnly";

#[derive(Clone, Debug, PartialEq)]
enum Choice {
    Keys(String),
    Callback(String),
    Nothing,
}

static MENU_ACTION_JS: Mutex<Option<MenuActionFn>> = Mutex::new(None);

thread_local! {
    static APP_NAME: RefCell<String> = const { RefCell::new(String::new()) };
    static CHOICES: RefCell<Vec<Choice>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn set_menu_action(choose: Option<MenuActionFn>) {
    *MENU_ACTION_JS.lock().unwrap() = choose;
}

/// Replaces the whole menu bar. Bindings for the previous custom menus are
/// dropped first, because the keymap answers `set_menus` with the first
/// binding it finds for an action.
pub(crate) fn set_menus(specs: &[MenuSpec], cx: &mut App) -> Result<(), String> {
    let app_name = APP_NAME.with(|name| name.borrow().clone());
    let mut built = Built::default();
    let menus = specs
        .iter()
        .map(|spec| {
            Ok(Menu {
                name: spec.label.clone().into(),
                disabled: false,
                items: build_items(&spec.items, &app_name, &mut built)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let kept: Vec<KeyBinding> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|binding| !is_menu_choice(binding))
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(kept.into_iter().chain(built.bindings));
    CHOICES.with(|choices| *choices.borrow_mut() = built.choices);
    cx.set_menus(menus);
    Ok(())
}

#[derive(Default)]
struct Built {
    choices: Vec<Choice>,
    bindings: Vec<KeyBinding>,
}

fn is_menu_choice(binding: &KeyBinding) -> bool {
    let action = binding.action().as_any();
    action.is::<MenuChoice>() || action.is::<DisabledMenuChoice>()
}

fn build_items(
    items: &[MenuItemSpec],
    app_name: &str,
    built: &mut Built,
) -> Result<Vec<MenuItem>, String> {
    items
        .iter()
        .map(|item| build_item(item, app_name, built))
        .collect()
}

fn build_item(item: &MenuItemSpec, app_name: &str, built: &mut Built) -> Result<MenuItem, String> {
    let label = |fallback: String| item.label.clone().unwrap_or(fallback);
    let role = item.role.as_deref();
    Ok(match role {
        Some("separator") => MenuItem::separator(),
        Some("services") => {
            MenuItem::os_submenu(label("Services".into()), SystemMenuType::Services)
        }
        Some("hide") => MenuItem::action(label(format!("Hide {app_name}")), Hide),
        Some("hideOthers") => MenuItem::action(label("Hide Others".into()), HideOthers),
        Some("showAll") => MenuItem::action(label("Show All".into()), ShowAll),
        Some("quit") => MenuItem::action(label(format!("Quit {app_name}")), Quit),
        Some("minimize") => MenuItem::action(label("Minimize".into()), MinimizeWindow),
        Some("zoom") => MenuItem::action(label("Zoom".into()), ZoomWindow),
        Some("close") => MenuItem::action(label("Close Window".into()), CloseWindow),
        Some(other) => return Err(format!("Unknown menu role \"{other}\"")),
        None => {
            let label = item
                .label
                .clone()
                .ok_or("A menu item needs a label, a role, or both")?;
            match &item.items {
                Some(children) => MenuItem::submenu(Menu {
                    name: label.into(),
                    disabled: item.enabled == Some(false),
                    items: build_items(children, app_name, built)?,
                }),
                None => build_choice(item, label, built)?,
            }
        }
    })
}

fn build_choice(item: &MenuItemSpec, label: String, built: &mut Built) -> Result<MenuItem, String> {
    let index = built.choices.len();
    let keys = item
        .keys
        .as_deref()
        .map(str::trim)
        .filter(|keys| !keys.is_empty());
    let shown = match keys {
        Some(keys) => shown_keystroke(keys)?,
        None => None,
    };
    built.choices.push(match (keys, &item.id) {
        (Some(keys), _) => Choice::Keys(keys.to_string()),
        (None, Some(id)) => Choice::Callback(id.clone()),
        (None, None) => Choice::Nothing,
    });

    let enabled = item.enabled.unwrap_or(true);
    let menu_item = if enabled {
        MenuItem::action(label, MenuChoice { index })
    } else {
        MenuItem::action(label, DisabledMenuChoice { index })
    };
    if let Some(keys) = shown {
        let context = Some(MENU_ONLY_CONTEXT);
        built.bindings.push(if enabled {
            KeyBinding::new(keys, MenuChoice { index }, context)
        } else {
            KeyBinding::new(keys, DisabledMenuChoice { index }, context)
        });
    }
    Ok(menu_item.checked(item.checked.unwrap_or(false)))
}

/// The keystroke a menu item can show beside its label. A key equivalent with
/// no cmd or ctrl would make AppKit take that key from text fields, so those
/// items still press their keys when chosen but show no shortcut.
fn shown_keystroke(keys: &str) -> Result<Option<&str>, String> {
    let mut parsed = Vec::new();
    for part in keys.split_whitespace() {
        parsed.push(Keystroke::parse(part).map_err(|_| format!("Invalid shortcut \"{keys}\""))?);
    }
    let shown = match parsed.as_slice() {
        [single] if single.modifiers.platform || single.modifiers.control => Some(keys),
        _ => None,
    };
    Ok(shown)
}

/// GPUI's `performKeyEquivalent:` dispatches a shortcut to the window first.
/// AppKit offers it to the menu only when nothing there stopped it, so by then
/// the window has already handled it.
fn chosen_by_key() -> bool {
    MainThreadMarker::new().is_some_and(|mtm| {
        NSApplication::sharedApplication(mtm)
            .currentEvent()
            .is_some_and(|event| event.r#type() == NSEventType::KeyDown)
    })
}

/// Deferred: GPUI dispatches menu actions inside an update of the active
/// window, and pressing keys needs to update that window again.
fn choose(index: usize, cx: &mut App) {
    if chosen_by_key() {
        return;
    }
    let choice = CHOICES.with(|choices| choices.borrow().get(index).cloned());
    match choice {
        Some(Choice::Keys(keys)) => cx.defer(move |cx| {
            if let Some(window) = cx.active_window() {
                let _ = window.update(cx, |_view, window, cx| {
                    crate::automation::dispatch_keystrokes(window, cx, &keys)
                });
            }
        }),
        Some(Choice::Callback(id)) => {
            if let Some(callback) = MENU_ACTION_JS.lock().unwrap().as_ref() {
                callback.call(id, ThreadsafeFunctionCallMode::NonBlocking);
            }
        }
        Some(Choice::Nothing) | None => {}
    }
}

/// The App menu plus the Window menu, the minimum a macOS app is expected to
/// have. `create_menu_bar` gives the menu named `Window` to
/// `NSApplication.setWindowsMenu:`, which is what appends the window list.
pub(crate) fn default_menus(app_name: &str) -> Vec<Menu> {
    vec![
        Menu {
            name: app_name.to_string().into(),
            disabled: false,
            items: vec![
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(format!("Hide {app_name}"), Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action(format!("Quit {app_name}"), Quit),
            ],
        },
        // No "Enter Full Screen" item: AppKit prepends its own window-tiling
        // items, that one included, to whichever menu is given to
        // `setWindowsMenu:`. Adding ours produced two entries sharing a
        // shortcut.
        Menu {
            name: "Window".into(),
            disabled: false,
            items: vec![
                MenuItem::action("Minimize", MinimizeWindow),
                MenuItem::action("Zoom", ZoomWindow),
                MenuItem::separator(),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_names(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => "-".to_string(),
                MenuItem::Submenu(submenu) => submenu.name.to_string(),
                MenuItem::SystemMenu(os_menu) => os_menu.name.to_string(),
                MenuItem::Action { name, .. } => name.to_string(),
            })
            .collect()
    }

    #[test]
    fn app_menu_is_named_after_the_app() {
        let menus = default_menus("Chat");
        assert_eq!(menus[0].name.as_ref(), "Chat");
        assert_eq!(
            item_names(&menus[0]),
            vec![
                "Services",
                "-",
                "Hide Chat",
                "Hide Others",
                "Show All",
                "-",
                "Quit Chat",
            ]
        );
    }

    // `create_menu_bar` only calls `setWindowsMenu:` for this exact name.
    #[test]
    fn window_menu_keeps_the_name_appkit_looks_for() {
        let menus = default_menus("Chat");
        assert_eq!(menus[1].name.as_ref(), "Window");
        assert_eq!(
            item_names(&menus[1]),
            vec!["Minimize", "Zoom", "-", "Close Window"]
        );
    }

    fn item(label: &str) -> MenuItemSpec {
        MenuItemSpec {
            label: Some(label.into()),
            ..Default::default()
        }
    }

    #[test]
    fn only_cmd_or_ctrl_shortcuts_show_in_the_menu() {
        assert_eq!(shown_keystroke("cmd-k"), Ok(Some("cmd-k")));
        assert_eq!(
            shown_keystroke("ctrl-shift-tab"),
            Ok(Some("ctrl-shift-tab"))
        );
        assert_eq!(shown_keystroke("n"), Ok(None));
        assert_eq!(shown_keystroke("shift-h"), Ok(None));
        assert_eq!(shown_keystroke("alt-z"), Ok(None));
        assert_eq!(shown_keystroke("cmd-k cmd-s"), Ok(None));
        assert!(shown_keystroke("cmd-k-x").is_err());
    }

    #[test]
    fn items_press_their_keys_or_call_back_with_their_id() {
        let items = vec![
            MenuItemSpec {
                id: Some("palette".into()),
                keys: Some("cmd-k".into()),
                ..item("Search Everything")
            },
            MenuItemSpec {
                id: Some("rename".into()),
                keys: Some("n".into()),
                ..item("Rename")
            },
            MenuItemSpec {
                id: Some("about".into()),
                ..item("About")
            },
            item("Nothing"),
        ];
        let mut built = Built::default();
        build_items(&items, "Chat", &mut built).unwrap();
        assert_eq!(
            built.choices,
            vec![
                Choice::Keys("cmd-k".into()),
                Choice::Keys("n".into()),
                Choice::Callback("about".into()),
                Choice::Nothing,
            ]
        );
        assert_eq!(built.bindings.len(), 1);
        assert!(built.bindings[0]
            .action()
            .partial_eq(&MenuChoice { index: 0 }));
    }

    #[test]
    fn disabled_items_use_the_action_nothing_handles() {
        let items = vec![MenuItemSpec {
            enabled: Some(false),
            keys: Some("cmd-d".into()),
            ..item("Debug")
        }];
        let mut built = Built::default();
        let menu = build_items(&items, "Chat", &mut built).unwrap();
        let MenuItem::Action { action, .. } = &menu[0] else {
            panic!("expected an action item");
        };
        assert!(action.partial_eq(&DisabledMenuChoice { index: 0 }));
        assert!(built.bindings[0]
            .action()
            .partial_eq(&DisabledMenuChoice { index: 0 }));
    }

    #[test]
    fn roles_and_submenus_build_like_the_defaults() {
        let items = vec![
            MenuItemSpec {
                role: Some("services".into()),
                ..Default::default()
            },
            MenuItemSpec {
                role: Some("separator".into()),
                ..Default::default()
            },
            MenuItemSpec {
                items: Some(vec![item("Inner")]),
                ..item("Recent")
            },
            MenuItemSpec {
                role: Some("quit".into()),
                ..Default::default()
            },
        ];
        let menu = Menu {
            name: "Chat".into(),
            disabled: false,
            items: build_items(&items, "Chat", &mut Built::default()).unwrap(),
        };
        assert_eq!(
            item_names(&menu),
            vec!["Services", "-", "Recent", "Quit Chat"]
        );
    }

    #[test]
    fn unknown_roles_and_missing_labels_are_errors() {
        let unknown = MenuItemSpec {
            role: Some("print".into()),
            ..Default::default()
        };
        assert!(build_items(&[unknown], "Chat", &mut Built::default()).is_err());
        assert!(build_items(&[MenuItemSpec::default()], "Chat", &mut Built::default()).is_err());
    }
}

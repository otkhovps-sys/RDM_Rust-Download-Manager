//! System tray icon with a small menu, like IDM: closing the window keeps
//! RDM (and its downloads) running here until "Exit" is chosen.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Clone, Copy, Debug)]
pub enum TrayAction {
    Show,
    AddUrl,
    ResumeAll,
    PauseAll,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    actions: Receiver<TrayAction>,
    tooltip: String,
}

impl Tray {
    /// `None` when the desktop has no tray; RDM then closes normally.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let entries = [
            (Some("Show RDM"), TrayAction::Show),
            (Some("Add URL..."), TrayAction::AddUrl),
            (None, TrayAction::Show),
            (Some("Resume all"), TrayAction::ResumeAll),
            (Some("Stop all"), TrayAction::PauseAll),
            (None, TrayAction::Show),
            (Some("Exit"), TrayAction::Quit),
        ];
        let menu = Menu::new();
        let mut ids: Vec<(MenuId, TrayAction)> = Vec::new();
        for (label, action) in entries {
            match label {
                Some(label) => {
                    let item = MenuItem::new(label, true, None);
                    ids.push((item.id().clone(), action));
                    menu.append(&item).ok()?;
                }
                None => menu.append(&PredefinedMenuItem::separator()).ok()?,
            }
        }

        let image = crate::icons::app_icon();
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("RDM")
            .with_icon(Icon::from_rgba(image.rgba, image.width, image.height).ok()?)
            .build()
            .ok()?;

        // The handlers run on the event loop; they only queue the action and
        // wake the app, which may be hidden at that moment.
        let (tx, actions) = channel();
        let send = |tx: Sender<TrayAction>, ctx: egui::Context| {
            let tx = Mutex::new(tx);
            move |action| {
                let _ = tx.lock().unwrap().send(action);
                ctx.request_repaint();
            }
        };
        let on_menu = send(tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some((_, action)) = ids.iter().find(|(id, _)| *id == event.id) {
                on_menu(*action);
            }
        }));
        let on_click = send(tx, ctx.clone());
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            let left_click = matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            );
            if left_click {
                on_click(TrayAction::Show);
            }
        }));

        Some(Self {
            icon,
            actions,
            tooltip: String::new(),
        })
    }

    pub fn poll(&self) -> Vec<TrayAction> {
        self.actions.try_iter().collect()
    }

    pub fn set_tooltip(&mut self, text: String) {
        if text != self.tooltip {
            let _ = self.icon.set_tooltip(Some(&text));
            self.tooltip = text;
        }
    }
}

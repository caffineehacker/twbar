use async_channel::Sender;
use async_std::{stream::StreamExt, sync::{Arc, Mutex, Weak}, task};
use futures::{select, future::{select, Either}};
use serde::Deserialize;

use super::{commands::HyprlandCommands, events::{EventData, HyprlandEvent, HyprlandEvents, LatestEventValue, LatestEventValueListener}, wayland_manager::{ExtForeignToplevel, WaylandManager}};

pub enum WindowEvent {
    NewWindow(HyprlandWindow),
    ModifiedWindow(HyprlandWindow),
    ClosedWindow(String),
}

#[derive(Deserialize, Clone, Default)]
pub struct HyprlandWorkspace {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize, Clone, Default)]
pub struct HyprlandWindow {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    pub at: [i32; 2],
    pub size: [i32; 2],
    pub workspace: HyprlandWorkspace,
    pub floating: bool,
    pub pseudo: bool,
    pub monitor: i32,
    pub class: String,
    pub title: String,
    #[serde(rename = "initialClass")] 
    pub initial_class: String,
    #[serde(rename = "initialTitle")] 
    pub initial_title: String,
    pub pid: u64,
    pub xwayland: bool,
    pub pinned: bool,
    pub fullscreen: bool,
    #[serde(rename = "fullscreenMode")] 
    pub fullscreen_mode: i32,
    #[serde(rename = "fakeFullscreen")] 
    pub fake_fullscreen: bool,
    pub grouped: Vec<String>,
    pub tags: Vec<String>,
    pub swallowing: String,
    #[serde(rename = "focusHistoryID")] 
    pub focus_history_id: i32,
}

impl HyprlandWindow {
    fn update_from(&mut self, foreign_toplevel: &ExtForeignToplevel) {
        self.title = foreign_toplevel.title.clone();
    }
}

impl From<ExtForeignToplevel> for HyprlandWindow {
    fn from(value: ExtForeignToplevel) -> Self {
        let mut new_window = Self::default();
        new_window.update_from(&value);
        new_window
    }
}

impl EventData for Vec<HyprlandWindow> {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        // TODO: Handle errors without panic
        let window_data = serde_json::from_str::<Vec<HyprlandWindow>>(data).unwrap();
        Some(window_data)
    }
}

pub struct HyprlandWindows {
    windows: Arc<LatestEventValue<Vec<HyprlandWindow>>>,
    event_listeners: Arc<Mutex<Vec<Sender<WindowEvent>>>>,
    wayland_manager: Arc<WaylandManager>,
}

impl HyprlandWindows {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandWindows>> = Mutex::new(Weak::new());
        
        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new().await;
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    async fn new() -> Arc<Self> {
        let windows = Arc::new(LatestEventValue::new());

        let instance = Arc::new(Self {
            windows,
            event_listeners: Arc::new(Mutex::new(Vec::new())),
            wayland_manager: WaylandManager::instance().await,
        });

        {
            let instance = instance.clone();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.force_refresh().await;

                loop {
                    match events.recv().await.unwrap() {
                        HyprlandEvent::ActiveWindow(w) => {},
                        HyprlandEvent::FocusedMon(_) => {},
                        HyprlandEvent::Workspace(_) => {},
                        HyprlandEvent::WorkspaceV2(_) => {},
                    }
                }
            });
        }

        {
            let instance = instance.clone();
            task::spawn(async move {
                let wayland_manager = WaylandManager::instance().await;
                let (_current_windows, mut wayland_window_listener) = wayland_manager.create_window_listener().await;

                loop {
                    match wayland_window_listener.recv().await.unwrap() {
                        super::wayland_manager::WaylandWindowEvent::UpdatedWindowExt(updated_window) => {
                            instance.windows.update_fn(|current_windows| {
                                let updated_windows: Vec<HyprlandWindow> = current_windows.clone().into_iter().map(|mut w| {
                                    // Address is formatted with {:x} and identifier is ->{:016x}
                                    if updated_window.identifier.split_once("->").unwrap().1.ends_with(w.address.strip_prefix("0x").unwrap_or("FFFFFFFFFFFFFFFFFFF")) {
                                        w.update_from(&updated_window);
                                    }
                                    w
                                }).collect();

                                Some(updated_windows)
                            }).await;
                        },
                        super::wayland_manager::WaylandWindowEvent::NewWindowExt(new_window) => {
                            instance.windows.update_fn(|current_windows| {
                                // While this is a new window event, it may have already been added by they Hyprland events
                                if !current_windows.iter().any(|w| new_window.identifier.split_once("->").unwrap().1.ends_with(w.address.strip_prefix("0x").unwrap_or("FFFFFFFFFFFFFFFFFFFFFFFFFFF"))) {
                                    let mut updated_windows = current_windows.clone();
                                    updated_windows.push(new_window.into());
                                    Some(updated_windows)
                                } else {
                                    None
                                }
                            }).await;
                        },
                        super::wayland_manager::WaylandWindowEvent::RemovedWindowExt(deleted_window) => {
                            instance.windows.update_fn(|current_windows| {
                                let updated_windows: Vec<HyprlandWindow> = current_windows.clone().into_iter().filter(|w| {
                                    // Address is formatted with {:x} and identifier is ->{:016x}
                                    !deleted_window.identifier.split_once("->").unwrap().1.ends_with(w.address.strip_prefix("0x").unwrap_or("FFFFFFFFFFFFFFFFFFF"))
                                }).collect();

                                Some(updated_windows)
                            }).await;
                        },
                        _ => {},
                    }
                }
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        let commands = HyprlandCommands::instance().await;
        self.windows.update(Vec::<HyprlandWindow>::parse(&commands.send_command("j/clients").await).unwrap()).await;
    }

    pub fn get_windows_update_emitter(&self) -> LatestEventValueListener<Vec<HyprlandWindow>> {
        LatestEventValueListener::new(self.windows.clone())
    }
}
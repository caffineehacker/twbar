use async_broadcast::{broadcast, InactiveReceiver, Receiver};
use async_std::{
    sync::{Arc, Mutex, Weak},
    task,
};
use gtk4::glib;
use serde::Deserialize;

use super::{
    commands::HyprlandCommands,
    events::{
        EventData, HyprlandEvent, HyprlandEvents, LatestEventValue, LatestEventValueListener,
    },
    wayland_manager::{ExtForeignToplevel, WaylandManager},
};

#[derive(Clone, Debug)]
pub enum WindowEvent {
    NewWindow(HyprlandWindow),
    ModifiedWindow(HyprlandWindow),
    // Argument is the window address
    ClosedWindow(String),
}

#[derive(Deserialize, Clone, Default, Debug, PartialEq, Eq)]
pub struct HyprlandPartialWorkspace {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize, Clone, Default, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "HyprlandWindow")]
pub struct HyprlandWindow {
    pub address: String,
    pub mapped: bool,
    pub hidden: bool,
    pub at: (i32, i32),
    pub size: (i32, i32),
    pub workspace: HyprlandPartialWorkspace,
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
        self.address = "0x".to_owned()
            + foreign_toplevel
                .identifier
                .split_once("->")
                .unwrap()
                .1
                .trim_start_matches('0');
        self.title.clone_from(&foreign_toplevel.title);
    }

    fn update_from_event(&mut self, event: &HyprlandEvent) {
        match event {
            HyprlandEvent::MoveWindowV2(move_window) => {
                self.workspace.id = move_window.workspace_id;
                self.workspace.name.clone_from(&move_window.workspace_name);
            }
            HyprlandEvent::OpenWindow(open_window) => {
                assert!(self.address.is_empty() || self.address == open_window.address);
                open_window.address.clone_into(&mut self.address);
                open_window.class.clone_into(&mut self.initial_class);
                open_window.class.clone_into(&mut self.class);
                open_window.title.clone_into(&mut self.initial_title);
                open_window.title.clone_into(&mut self.title);

                // FIXME: Set workspace based on the workspace name.
            }
            _ => {
                panic!("Unexpected message type: {:?}", event);
            }
        }
    }

    pub async fn activate(&self) {
        HyprlandCommands::set_active_window(&self.address).await;
    }
}

impl From<ExtForeignToplevel> for HyprlandWindow {
    fn from(value: ExtForeignToplevel) -> Self {
        let mut new_window = Self::default();
        new_window.update_from(&value);
        new_window
    }
}

impl From<&HyprlandEvent> for HyprlandWindow {
    fn from(value: &HyprlandEvent) -> Self {
        let mut new_window = Self::default();
        new_window.update_from_event(value);
        new_window
    }
}

impl EventData for Vec<HyprlandWindow> {
    fn parse(data: &str) -> Option<Self>
    where
        Self: Sized,
    {
        // TODO: Handle errors without panic
        let window_data = serde_json::from_str::<Vec<HyprlandWindow>>(data);
        match window_data {
            Err(err) => {
                println!("Failed to deserialize: {}", data);
                println!("Error is {:?}", err);
                Some(vec![])
            }
            Ok(window_data) => Some(window_data),
        }
    }
}

pub struct HyprlandWindows {
    windows: Arc<LatestEventValue<Vec<HyprlandWindow>>>,
    event_receiver: Arc<InactiveReceiver<WindowEvent>>,
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
        let (mut sender, receiver) = broadcast(1024);
        sender.set_await_active(false);
        sender.set_overflow(true);

        let instance = Arc::new(Self {
            windows,
            event_receiver: Arc::new(receiver.deactivate()),
            wayland_manager: WaylandManager::instance().await,
        });

        {
            let instance = instance.clone();
            let sender = sender.clone();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.force_refresh().await;

                // // Send open window command for every window
                // {
                //     let windows = instance.windows.current_value.lock().await;
                //     for window in &windows.1 {
                //         sender.broadcast(WindowEvent::NewWindow(window.clone())).await.ok();
                //     }
                // }

                loop {
                    let event = events.recv().await.unwrap();
                    println!("Hyprland Event: {:?}", event);
                    match &event {
                        HyprlandEvent::CloseWindow(address) => {
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    let updated_windows: Vec<HyprlandWindow> = current_windows
                                        .clone()
                                        .into_iter()
                                        .filter(|w| w.address != *address)
                                        .collect();

                                    Some(updated_windows)
                                })
                                .await;
                            sender
                                .broadcast(WindowEvent::ClosedWindow(address.clone()))
                                .await
                                .ok();
                            instance.force_refresh().await;
                        }
                        HyprlandEvent::MoveWindowV2(move_window) => {
                            let mut updated_hyprland_window = None;
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    let updated_windows: Vec<HyprlandWindow> = current_windows
                                        .clone()
                                        .into_iter()
                                        .map(|mut w| {
                                            if w.address == move_window.window_address {
                                                w.update_from_event(&event);
                                                updated_hyprland_window = Some(w.clone());
                                            }
                                            w
                                        })
                                        .collect();

                                    Some(updated_windows)
                                })
                                .await;

                            if updated_hyprland_window.is_some() {
                                sender
                                    .broadcast(WindowEvent::ModifiedWindow(
                                        updated_hyprland_window.unwrap(),
                                    ))
                                    .await
                                    .ok();
                            }
                            instance.force_refresh().await;
                        }
                        HyprlandEvent::OpenWindow(open_window) => {
                            let mut updated_hyprland_window = None;
                            let mut new_hyprland_window: Option<HyprlandWindow> = None;
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    let mut updated_windows: Vec<HyprlandWindow> = current_windows
                                        .clone()
                                        .into_iter()
                                        .map(|mut w| {
                                            if w.address == open_window.address {
                                                w.update_from_event(&event);
                                                updated_hyprland_window = Some(w.clone());
                                            }
                                            w
                                        })
                                        .collect();

                                    if updated_hyprland_window.is_none() {
                                        println!("New window from Hyprland: {:?}", open_window);
                                        let new_window: HyprlandWindow = (&event).into();
                                        new_hyprland_window = Some(new_window.clone());
                                        updated_windows.push(new_window);
                                    }

                                    Some(updated_windows)
                                })
                                .await;

                            println!(
                                "About to send hyprland openwindow event: {:?}, {:?}",
                                updated_hyprland_window, new_hyprland_window
                            );

                            if updated_hyprland_window.is_some() {
                                sender
                                    .broadcast(WindowEvent::ModifiedWindow(
                                        updated_hyprland_window.unwrap(),
                                    ))
                                    .await
                                    .ok();
                            } else if new_hyprland_window.is_some() {
                                sender
                                    .broadcast(WindowEvent::NewWindow(new_hyprland_window.unwrap()))
                                    .await
                                    .ok();
                            }
                            instance.force_refresh().await;
                        }
                        _ => {}
                    }
                }
            });
        }

        {
            let instance = instance.clone();
            let sender = sender.clone();
            task::spawn(async move {
                let wayland_manager = WaylandManager::instance().await;
                let (_current_windows, mut wayland_window_listener) =
                    wayland_manager.create_window_listener().await;

                loop {
                    match wayland_window_listener.recv().await.unwrap() {
                        super::wayland_manager::WaylandWindowEvent::UpdatedWindowExt(
                            updated_window,
                        ) => {
                            let mut updated_hyprland_window = None;
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    let updated_windows: Vec<HyprlandWindow> = current_windows
                                        .clone()
                                        .into_iter()
                                        .map(|mut w| {
                                            // Address is formatted with {:x} and identifier is ->{:016x}
                                            if updated_window
                                                .identifier
                                                .split_once("->")
                                                .unwrap()
                                                .1
                                                .ends_with(
                                                    w.address
                                                        .strip_prefix("0x")
                                                        .unwrap_or("FFFFFFFFFFFFFFFFFFF"),
                                                )
                                            {
                                                w.update_from(&updated_window);
                                            }
                                            updated_hyprland_window = Some(w.clone());
                                            w
                                        })
                                        .collect();

                                    Some(updated_windows)
                                })
                                .await;

                            if updated_hyprland_window.is_some() {
                                sender
                                    .broadcast(WindowEvent::ModifiedWindow(
                                        updated_hyprland_window.unwrap(),
                                    ))
                                    .await
                                    .ok();
                            }
                        }
                        super::wayland_manager::WaylandWindowEvent::NewWindowExt(new_window) => {
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    // While this is a new window event, it may have already been added by they Hyprland events
                                    if !current_windows.iter().any(|w| {
                                        new_window.identifier.split_once("->").unwrap().1.ends_with(
                                            w.address
                                                .strip_prefix("0x")
                                                .unwrap_or("FFFFFFFFFFFFFFFFFFFFFFFFFFF"),
                                        )
                                    }) {
                                        let mut updated_windows = current_windows.clone();
                                        updated_windows.push(new_window.clone().into());
                                        Some(updated_windows)
                                    } else {
                                        None
                                    }
                                })
                                .await;

                            sender
                                .broadcast(WindowEvent::NewWindow(new_window.into()))
                                .await
                                .ok();
                        }
                        super::wayland_manager::WaylandWindowEvent::RemovedWindowExt(
                            deleted_window,
                        ) => {
                            instance
                                .windows
                                .update_fn(|current_windows| {
                                    let updated_windows: Vec<HyprlandWindow> = current_windows
                                        .clone()
                                        .into_iter()
                                        .filter(|w| {
                                            // Address is formatted with {:x} and identifier is ->{:016x}
                                            !deleted_window
                                                .identifier
                                                .split_once("->")
                                                .unwrap()
                                                .1
                                                .ends_with(
                                                    w.address
                                                        .strip_prefix("0x")
                                                        .unwrap_or("FFFFFFFFFFFFFFFFFFF"),
                                                )
                                        })
                                        .collect();

                                    Some(updated_windows)
                                })
                                .await;

                            let addr = format!(
                                "0x{}",
                                deleted_window
                                    .identifier
                                    .split_once("->")
                                    .unwrap()
                                    .1
                                    .trim_start_matches('0')
                            );
                            println!("Broadcasting removal for {}", addr);
                            sender.broadcast(WindowEvent::ClosedWindow(addr)).await.ok();
                        }
                        _ => {}
                    }
                }
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        self.windows
            .update(
                Vec::<HyprlandWindow>::parse(&HyprlandCommands::send_command("j/clients").await)
                    .unwrap(),
            )
            .await;
    }

    pub fn get_windows_update_emitter(&self) -> LatestEventValueListener<Vec<HyprlandWindow>> {
        LatestEventValueListener::new(self.windows.clone())
    }

    pub async fn get_window_event_emitter(&self) -> (Vec<HyprlandWindow>, Receiver<WindowEvent>) {
        self.force_refresh().await;
        let current_windows_guard = self.windows.current_value.lock().await;
        (
            current_windows_guard.1.clone(),
            self.event_receiver.activate_cloned(),
        )
    }
}

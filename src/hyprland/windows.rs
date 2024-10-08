use async_std::{
    sync::{Arc, Mutex, Weak},
    task,
};
use gtk4::glib;
use log::error;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{
    commands::HyprlandCommands,
    events::{
        EventData, HyprlandEvent, HyprlandEvents, LatestEventValue, LatestEventValueListener,
    },
    wayland_manager::ExtForeignToplevel,
};

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
    #[serde(deserialize_with = "deserialize_bool")]
    pub fullscreen: bool,
    #[serde(rename = "fullscreenClient", deserialize_with = "deserialize_bool")]
    pub fullscreen_client: bool,
    pub grouped: Vec<String>,
    pub tags: Vec<String>,
    pub swallowing: String,
    #[serde(rename = "focusHistoryID")]
    pub focus_history_id: i32,
}

fn deserialize_bool<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(match serde::de::Deserialize::deserialize(deserializer)? {
        Value::Bool(b) => b,
        Value::String(s) => s == "yes" || s == "1",
        Value::Number(num) => {
            num.as_i64()
                .ok_or(serde::de::Error::custom("Invalid number"))?
                != 0
        }
        Value::Null => false,
        _ => return Err(serde::de::Error::custom("Wrong type, expected boolean")),
    })
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
                error!("Failed to deserialize: '{}' error is {:?}", data, err);
                Some(vec![])
            }
            Ok(window_data) => Some(window_data),
        }
    }
}

pub struct HyprlandWindows {
    windows: Arc<LatestEventValue<Vec<HyprlandWindow>>>,
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

        let instance = Arc::new(Self { windows });

        {
            let instance = instance.clone();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.force_refresh().await;

                loop {
                    let event = events.recv().await.unwrap();
                    match &event {
                        HyprlandEvent::CloseWindow(_)
                        | HyprlandEvent::MoveWindowV2(_)
                        | HyprlandEvent::OpenWindow(_)
                        | HyprlandEvent::MonitorAddedV2(_)
                        | HyprlandEvent::MonitorRemoved(_)
                        | HyprlandEvent::ChangeFloatingMode(_) => {
                            instance.force_refresh().await;
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
}

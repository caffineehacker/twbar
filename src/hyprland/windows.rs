use async_std::{sync::{Arc, Mutex, Weak}, task};
use serde::Deserialize;

use super::{commands::HyprlandCommands, events::{EventData, EventStream, EventStreamData}};

#[derive(Deserialize, Clone)]
pub struct HyprlandWorkspace {
    pub id: i32,
    pub name: String,
}

#[derive(Deserialize, Clone)]
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

impl EventData for Vec<HyprlandWindow> {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        // TODO: Handle errors without panic
        let window_data = serde_json::from_str::<Vec<HyprlandWindow>>(data).unwrap(); // .map_or(None, |windows| Some(windows))
        Some(window_data)
    }
}

pub struct HyprlandWindows {
    windows: Arc<EventStreamData<Vec<HyprlandWindow>>>,
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
        let windows = Arc::new(EventStreamData::new(vec![]));

        let instance = Arc::new(Self {
            windows
        });

        {
            let instance = instance.clone();
            task::spawn(async move {
                instance.force_refresh().await;

                // TODO: Listen on HyprlandEvents to update the window list
                loop {}
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        let commands = HyprlandCommands::instance().await;
        self.windows.update(&commands.send_command("j/clients").await).await;
    }

    pub fn get_windows_update_emitter(&self) -> EventStream<Vec<HyprlandWindow>> {
        EventStream::new(self.windows.clone())
    }
}
use async_broadcast::{broadcast, InactiveReceiver};
use async_std::{sync::{Arc, Mutex, RwLock, Weak}, task};
use gio::glib::clone::Downgrade;
use serde::Deserialize;

use super::{commands::HyprlandCommands, events::{HyprlandEvents, LatestEventValue, LatestEventValueListener}};

#[derive(Clone, Default, Deserialize)]
pub struct HyprlandWorkspace {
    pub id: i32,
    pub name: String,
    pub monitor: String,
    #[serde(rename = "monitorID")] 
    pub monitor_id: i32,
    pub windows: i32,
    #[serde(rename = "hasfullscreen")] 
    pub has_fullscreen: bool,
    #[serde(rename = "lastwindow")] 
    pub last_window: String,
    #[serde(rename = "lastwindowtitle")] 
    pub last_window_title: String,
}

pub struct HyprlandWorkspaces {
    workspaces: Arc<LatestEventValue<Vec<HyprlandWorkspace>>>,
}

impl HyprlandWorkspaces {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandWorkspaces>> = Mutex::new(Weak::new());
        
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
        let workspaces = Arc::new(LatestEventValue::new());

        let instance = Arc::new(Self {
            workspaces: workspaces.clone(),
        });

        {
            let instance = instance.downgrade();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.upgrade().unwrap().force_refresh().await;

                loop {
                    let event = events.recv().await.unwrap();
                    match event {
                        super::events::HyprlandEvent::CreateWorkspace(_) => instance.upgrade().unwrap().force_refresh().await,
                        super::events::HyprlandEvent::CreateWorkspaceV2(_) => {},
                        super::events::HyprlandEvent::MoveWorkspace(_) => instance.upgrade().unwrap().force_refresh().await,
                        super::events::HyprlandEvent::MoveWorkspaceV2(_) => {},
                        super::events::HyprlandEvent::RenameWorkspace(_) => instance.upgrade().unwrap().force_refresh().await,
                        super::events::HyprlandEvent::ActiveSpecial(_) => instance.upgrade().unwrap().force_refresh().await,
                        _ => {},
                    }
                }
            });
        }

        instance
    }

    pub async fn force_refresh(&self) {
        self.workspaces.update_fn(| _ | {
            task::block_on(async {
                let workspaces = HyprlandCommands::send_command("j/workspaces").await;
                let deserialized = serde_json::from_str::<Vec<HyprlandWorkspace>>(&workspaces);
                if deserialized.is_err() {
                    println!("Failed to deserialize: {}, {}", workspaces, deserialized.err().unwrap());
                    return None;
                }

                Some(deserialized.unwrap())
            })
        }).await;
    }

    pub fn get_workspaces_state_emitter(&self) -> LatestEventValueListener<Vec<HyprlandWorkspace>> {
        LatestEventValueListener::new(self.workspaces.clone())
    }
}
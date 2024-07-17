use async_broadcast::{broadcast, InactiveReceiver};
use async_std::{sync::{Arc, Mutex, RwLock, Weak}, task};
use serde::Deserialize;

use super::{commands::HyprlandCommands, events::{HyprlandEvents, LatestEventValue}};

#[derive(Clone, Default, Deserialize)]
pub struct HyprlandWorkspace {
    id: u32,
    name: String,
    monitor: String,
    #[serde(rename = "monitorID")] 
    monitor_id: u32,
    windows: u32,
    #[serde(rename = "hasFullscreen")] 
    has_fullscreen: bool,
    #[serde(rename = "lastwindow")] 
    last_window: String,
    #[serde(rename = "lastwindowtitle")] 
    last_window_title: String,
}

enum WorkspaceEvent {
    WorkspaceAdded,
    WorkspaceRemoved,
    WorkspaceMonitorChanged,
}

pub struct HyprlandWorkspaces {
    event_receiver: Arc<InactiveReceiver<WorkspaceEvent>>,
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
        let (mut sender, receiver) = broadcast(1024);
        sender.set_await_active(false);
        sender.set_overflow(true);

        let instance = Arc::new(Self {
            event_receiver: Arc::new(receiver.deactivate()),
            workspaces: workspaces.clone(),
        });

        {
            let instance = instance.clone();
            let sender = sender.clone();
            task::spawn(async move {
                let mut events = HyprlandEvents::instance().await.get_event_stream().await;

                instance.force_refresh().await;

                loop {
                    let event = events.recv().await.unwrap();
                    match event {
                        super::events::HyprlandEvent::CreateWorkspace(_cw) => {},
                        super::events::HyprlandEvent::CreateWorkspaceV2(cw) => {
                            
                        },
                        super::events::HyprlandEvent::MoveWorkspace(_) => todo!(),
                        super::events::HyprlandEvent::MoveWorkspaceV2(_) => todo!(),
                        super::events::HyprlandEvent::RenameWorkspace(_) => todo!(),
                        super::events::HyprlandEvent::ActiveSpecial(_) => todo!(),
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
                let workspaces = HyprlandCommands::send_command("workspaces").await;
                let deserialized = serde_json::from_str::<Vec<HyprlandWorkspace>>(&workspaces);
                if deserialized.is_err() {
                    println!("Failed to deserialize: {}, {}", workspaces, deserialized.err().unwrap());
                    return None;
                }

                Some(deserialized.unwrap())
            })
        }).await;
    }
}
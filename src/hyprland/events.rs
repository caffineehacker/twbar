use async_broadcast::{broadcast, InactiveReceiver, Receiver, Sender};
use async_std::{
    io::{prelude::BufReadExt, BufReader},
    stream::StreamExt,
    sync::{Arc, Condvar, Mutex, Weak},
    task,
};
use gio::glib::clone::Downgrade;

use super::utils::Utils;

pub trait EventData: Clone {
    fn parse(data: &str) -> Option<Self> where Self: Sized;
}

#[derive(Clone, Debug)]
pub enum HyprlandEvent {
    Workspace(String),
    WorkspaceV2(WorkspaceV2),
    FocusedMon(FocusedMon),
    ActiveWindow(ActiveWindow),
    // Window address
    ActiveWindowV2(String),
    Fullscreen(bool),
    // Monitor name
    MonitorRemoved(String),
    // Monitor name
    MonitorAdded(String),
    MonitorAddedV2(MonitorAddedV2),
    // Workspace name
    CreateWorkspace(String),
    CreateWorkspaceV2(CreateWorkspaceV2),
    MoveWorkspace(MoveWorkspace),
    MoveWorkspaceV2(MoveWorkspaceV2),
    RenameWorkspace(RenameWorkspace),
    ActiveSpecial(ActiveSpecial),
    ActiveLayout(ActiveLayout),
    OpenWindow(OpenWindow),
    // Window address
    CloseWindow(String),
    MoveWindow(MoveWindow),
    MoveWindowV2(MoveWindowV2),
    // Namespace
    OpenLayer(String),
    // Namespace
    CloseLayer(String),
    // Submap name
    Submap(String),
    ChangeFloatingMode(ChangeFloatingMode),
    // Window address
    Urgent(String),
    Minimize(Minimize),
    Screencast(Screencast),
    // Window address
    WindowTitle(String),
    WindowTitleV2(WindowTitleV2),
    ToggleGroup(ToggleGroup),
    // Window address
    MoveIntoGroup(String),
    // Window address
    MoveOutOfGroup(String),
    IgnoreGroupLock(bool),
    LockGroups(bool),
    ConfigReloaded(),
    Pin(Pin),
    //WindowTitle(
}

impl EventData for HyprlandEvent {
    fn parse(message: &str) -> Option<Self> where Self: Sized {
        if let Some((command, data)) = message.split_once(">>") {
            match command {
              "workspace" => Some(Self::Workspace(data.to_owned())),
              "workspacev2" => WorkspaceV2::parse(data).map(|d| Self::WorkspaceV2(d)),
              "focusedmon" => FocusedMon::parse(data).map(|d| Self::FocusedMon(d)),
              "activewindow" => ActiveWindow::parse(data).map(|d| Self::ActiveWindow(d)),
              "activewindowv2" => Some(Self::ActiveWindowV2(format!("0x{}", data.to_owned()))),
              "fullscreen" => Some(Self::Fullscreen(data == "1")),
              "monitorremoved" => Some(Self::MonitorRemoved(data.to_owned())),
              "monitoradded" => Some(Self::MonitorAdded(data.to_owned())),
              "monitoraddedv2" => MonitorAddedV2::parse(data).map(|ma| Self::MonitorAddedV2(ma)),
              "createworkspace" => Some(Self::CreateWorkspace(data.to_owned())),
              "createworkspacev2" => CreateWorkspaceV2::parse(data).map(|cw| Self::CreateWorkspaceV2(cw)),
              "moveworkspace" => MoveWorkspace::parse(data).map(|mw| Self::MoveWorkspace(mw)),
              "moveworkspacev2" => MoveWorkspaceV2::parse(data).map(|mw| Self::MoveWorkspaceV2(mw)),
              "renameworkspace" => RenameWorkspace::parse(data).map(|rw| Self::RenameWorkspace(rw)),
              "openwindow" => OpenWindow::parse(data).map(|ow| Self::OpenWindow(ow)),
              "closewindow" => Some(Self::CloseWindow(format!("0x{}", data.to_owned()))),
              "movewindow" => MoveWindow::parse(data).map(|mw| Self::MoveWindow(mw)),
              _ => { println!("Unhandled event: {}>>{}", command, data); None }
            }
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub struct MonitorAddedV2 {
    pub id: String,
    pub name: String,
    pub description: String,
}

impl EventData for MonitorAddedV2 {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let mut parts = data.splitn(3, ",");
        Some(Self {
            id: parts.next()?.to_owned(),
            name: parts.next()?.to_owned(),
            description: parts.next()?.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct CreateWorkspaceV2 {
    pub id: String,
    pub name: String,
}

impl EventData for CreateWorkspaceV2 {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let (id, name) = data.split_once(",")?;
        Some(Self {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct DestroyWorkspaceV2 {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct MoveWorkspace {
    pub name: String,
    pub monitor_name: String,
}

impl EventData for MoveWorkspace {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let (name, monitor_name) = data.split_once(",")?;
        Some(Self {
            name: name.to_owned(),
            monitor_name: monitor_name.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct MoveWorkspaceV2 {
    pub id: String,
    pub name: String,
    pub monitor_name: String,
}

impl EventData for MoveWorkspaceV2 {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let mut parts = data.splitn(3, ",");
        Some(Self {
            id: parts.next()?.to_owned(),
            name: parts.next()?.to_owned(),
            monitor_name: parts.next()?.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct RenameWorkspace {
    pub id: String,
    pub new_name: String,
}

impl EventData for RenameWorkspace {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let (id, new_name) = data.split_once(",")?;
        Some(Self {
            id: id.to_owned(),
            new_name: new_name.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ActiveSpecial {
    pub name: String,
    pub monitor_name: String,
}

#[derive(Clone, Debug)]
pub struct ActiveLayout {
    pub keyboard_name: String,
    pub layout_name: String,
}

#[derive(Clone, Debug)]
pub struct OpenWindow {
    pub address: String,
    pub workspace_name: String,
    pub class: String,
    pub title: String,
}

impl EventData for OpenWindow {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let mut parts = data.splitn(4, ",");
        Some(Self {
            address: format!("0x{}", parts.next()?.to_owned()),
            workspace_name: parts.next()?.to_owned(),
            class: parts.next()?.to_owned(),
            title: parts.next()?.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct MoveWindow {
    pub window_address: String,
    pub workspace_name: String
}

impl EventData for MoveWindow {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let (window_address, workspace_name) = data.split_once(",")?;
        Some(Self {
            window_address: window_address.to_owned(),
            workspace_name: workspace_name.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct MoveWindowV2 {
    pub window_address: String,
    pub workspace_id: i32,
    pub workspace_name: String,
}

impl EventData for MoveWindowV2 {
    fn parse(data: &str) -> Option<Self> where Self: Sized {
        let mut parts = data.splitn(3, ",");
        Some(Self {
            window_address: format!("0x{}", parts.next()?.to_owned()),
            workspace_id: parts.next()?.to_owned().parse::<i32>().ok()?,
            workspace_name: parts.next()?.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct ChangeFloatingMode {
    window_address: String,
    is_floating: bool,
}

#[derive(Clone, Debug)]
pub struct Minimize {
    window_address: String,
    is_minimized: bool,
}

#[derive(Clone, Debug)]
pub struct Screencast {
    state: bool,
    // Should be an enum with 0 == monitor and 1 == window sharing
    owner: bool,
}

#[derive(Clone, Debug)]
pub struct WindowTitleV2 {
    address: String,
    title: String,
}

#[derive(Clone, Debug)]
pub struct ToggleGroup {
    state: bool,
    window_addresses: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Pin {
    window_address: String,
    pin_state: String,
}

#[derive(Clone, Debug)]
pub struct WorkspaceV2 {
    pub id: String,
    pub name: String,
}

impl EventData for WorkspaceV2 {
    fn parse(data: &str) -> Option<Self> {
        let (id, name) = data.split_once(",")?;
        
        Some(Self {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct FocusedMon {
    pub id: String,
    pub name: String,
}

impl EventData for FocusedMon {
    fn parse(data: &str) -> Option<Self> {
        let (id, name) = data.split_once(",")?;
        
        Some(Self {
            id: id.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[derive(Clone, Default, Debug)]
pub struct ActiveWindow {
    pub title: String,
}

impl EventData for ActiveWindow {
    fn parse(data: &str) -> Option<Self> {
        let (_class_name, title) = data.split_once(",")?;
        
        Some(Self {
            title: title.to_owned()
        })
    }
}

#[derive(Clone)]
pub struct Workspace {

}

pub(super) struct LatestEventValue<T> {
    pub current_value: Mutex<(i64, T)>,

    trigger: Condvar,
}

impl<T: Clone + Default> LatestEventValue<T> {
    pub fn new() -> Self {
        Self {
            current_value: Mutex::new((0, T::default())),
            trigger: Condvar::new(),
        }
    }

    pub async fn update(&self, new_value: T) {
        let mut data_lock = self.current_value.lock().await;
        *data_lock = (data_lock.0 + 1, new_value);
        self.trigger.notify_all();
    }

    pub async fn update_fn<F>(&self, update_func: F) where F: FnOnce(&T) -> Option<T> {
        let mut data_lock = self.current_value.lock().await;
        let updated_data = (update_func)(&data_lock.1);
        if updated_data.is_some() {
            *data_lock = (data_lock.0 + 1, updated_data.unwrap());
            self.trigger.notify_all();
        }
    }
}

pub struct LatestEventValueListener<T: Clone> {
    data: Arc<LatestEventValue<T>>,
    last_seen_iteration: i64,
}

impl<T: Clone> LatestEventValueListener<T> {
    pub(super) fn new(data: Arc<LatestEventValue<T>>) -> Self {
        Self {
            data,
            last_seen_iteration: 0,
        }
    }

    pub async fn next(&mut self) -> T {
        let guard = self
            .data
            .trigger
            .wait_until(
                self.data.current_value.lock().await,
                |(iteration, _data)| *iteration != self.last_seen_iteration,
            )
            .await;

        self.last_seen_iteration = guard.0;

        guard.1.clone()
    }
}

pub struct HyprlandEvents {
    active_window: Arc<LatestEventValue<ActiveWindow>>,
    event_sender: Arc<Mutex<Sender<HyprlandEvent>>>,
    event_receiver: InactiveReceiver<HyprlandEvent>,
}

impl HyprlandEvents {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandEvents>> = Mutex::new(Weak::new());
        
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
        let active_window = Arc::new(LatestEventValue::new());
        let (mut sender, receiver) = broadcast(256);
        sender.set_await_active(false);
        sender.set_overflow(true);

        let instance = Arc::new(HyprlandEvents {
            active_window,
            event_sender: Arc::new(Mutex::new(sender)),
            event_receiver: receiver.deactivate()
        });

        let instance_weak = instance.downgrade();
        task::spawn(async move {
            let event_stream = Utils::create_event_socket().await.unwrap();
            let mut lines = BufReader::new(event_stream).lines();

            while let Some(Ok(line)) = lines.next().await {
                let instance = instance_weak.upgrade();
                if instance.is_none() {
                    return;
                }
                let instance = instance.as_ref().unwrap();

                if let Some(event) = HyprlandEvent::parse(&line) {
                    match &event {
                        HyprlandEvent::ActiveWindow(active_window) => instance.active_window.update(active_window.clone()).await,
                        _ => (),
                    };

                    let event_sender = instance.event_sender.lock().await;

                    let is_sent = event_sender.broadcast_direct(event).await;
                    if is_sent.is_err() {
                        println!("Error on sending event");
                    }
                }
            }
        });

        instance
    }

    pub fn get_active_window_emitter(&self) -> LatestEventValueListener<ActiveWindow> {
        LatestEventValueListener::new(self.active_window.clone())
    }

    pub async fn get_event_stream(&self) -> Receiver<HyprlandEvent> {
        self.event_receiver.activate_cloned()
    }
}

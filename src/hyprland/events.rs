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

#[derive(Clone)]
pub enum HyprlandEvent {
    Workspace(String),
    WorkspaceV2(WorkspaceV2),
    FocusedMon(FocusedMon),
    ActiveWindow(ActiveWindow),
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
              _ => None
            }
        } else {
            None
        }
    }
}

#[derive(Clone)]
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

#[derive(Clone)]
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

#[derive(Clone, Default)]
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
    current_value: Mutex<(i64, T)>,

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

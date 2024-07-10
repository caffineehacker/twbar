use async_std::{
    io::{prelude::BufReadExt, BufReader},
    stream::StreamExt,
    sync::{Arc, Condvar, Mutex, Weak},
    task::{self, JoinHandle},
};

use super::utils::Utils;

pub trait EventData: Clone {
    fn parse(data: &str) -> Option<Self> where Self: Sized;
}

#[derive(Clone)]
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

pub(super) struct EventStreamData<T: EventData> {
    current_value: Mutex<(i64, T)>,

    trigger: Condvar,
}

impl<T: EventData> EventStreamData<T> {
    pub fn new(initial_value: T) -> Self {
        Self {
            current_value: Mutex::new((0, initial_value)),
            trigger: Condvar::new(),
        }
    }

    pub async fn update(&self, new_data: &str) {
        if let Some(new_value) = T::parse(new_data) {
            let mut data_lock = self.current_value.lock().await;
            *data_lock = (data_lock.0 + 1, new_value);
            self.trigger.notify_all();
        }
    }
}

pub struct EventStream<T: EventData> {
    data: Arc<EventStreamData<T>>,
    last_seen_iteration: i64,
}

impl<T: EventData> EventStream<T> {
    pub(super) fn new(data: Arc<EventStreamData<T>>) -> Self {
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
    active_window: Arc<EventStreamData<ActiveWindow>>,
    
    event_task: JoinHandle<()>,
}

impl HyprlandEvents {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<HyprlandEvents>> = Mutex::new(Weak::new());
        
        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Arc::new(Self::new().await);
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    async fn new() -> Self {
        let active_window = Arc::new(EventStreamData::new(ActiveWindow {
                title: "".to_owned(),
            }));
        let active_window_weak = Arc::downgrade(&active_window);

        HyprlandEvents {
            active_window,

            event_task: task::spawn(async move {
                let event_stream = Utils::create_event_socket().await.unwrap();
                let mut lines = BufReader::new(event_stream).lines();

                while let Some(Ok(line)) = lines.next().await {
                    if let Some((command, data)) = line.split_once(">>") {
                        // We unwrap because we're ok with failing as it means the instance is done and the thread should die.
                        // TODO: Handle the unwrap better so we don't log a panic.
                        match command {
                          "activewindow" => active_window_weak.upgrade().unwrap().update(data).await,
                          _ => ()
                        }
                    }
                }
            }),
        }
    }

    pub fn get_active_window_emitter(&self) -> EventStream<ActiveWindow> {
        EventStream::new(self.active_window.clone())
    }
}

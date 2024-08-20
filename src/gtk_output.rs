use std::sync::{Arc, Weak};

use async_std::{sync::Mutex, task};
use gdk4_wayland::{WaylandDisplay, WaylandMonitor};
use gio::prelude::Cast;
use gtk4::gdk::{Display, Monitor};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::wl_registry::WlRegistry,
    Connection, Dispatch, EventQueue, Proxy,
};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

pub struct GtkOutputs {
    output_manager: zxdg_output_manager_v1::ZxdgOutputManagerV1,
    queue: Mutex<EventQueue<GtkOutputsQueue>>,
}

unsafe impl Send for GtkOutputs {}

unsafe impl Sync for GtkOutputs {}

struct GtkOutputsQueue;

impl GtkOutputs {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<GtkOutputs>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Arc::new(Self::new());
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    fn new() -> Self {
        let display = Display::default().unwrap();
        let wayland_display: &WaylandDisplay = display.dynamic_cast_ref().unwrap();

        let wl_display = wayland_display.wl_display().unwrap();
        let connection = Connection::from_backend(wl_display.backend().upgrade().unwrap());

        let (globals, queue) = registry_queue_init::<GtkOutputsQueue>(&connection).unwrap();

        // now you can bind the globals you need for your app
        let output_manager: zxdg_output_manager_v1::ZxdgOutputManagerV1 =
            globals.bind(&queue.handle(), 3..=3, ()).unwrap();

        Self {
            output_manager,
            queue: Mutex::new(queue),
        }
    }

    pub async fn get_name(&self, monitor: &Monitor) -> String {
        let (name_sender, mut name_receiver) = async_broadcast::broadcast(1);

        let wayland_monitor: &WaylandMonitor = monitor.dynamic_cast_ref().unwrap();
        let mut queue = self.queue.lock().await;
        self.output_manager.get_xdg_output(
            &wayland_monitor.wl_output().unwrap(),
            &queue.handle(),
            name_sender,
        );

        queue.roundtrip(&mut GtkOutputsQueue {}).unwrap();

        name_receiver.recv_direct().await.unwrap()
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for GtkOutputsQueue {
    fn event(
        _state: &mut Self,
        _proxy: &WlRegistry,
        _event: <WlRegistry as Proxy>::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for GtkOutputsQueue {
    fn event(
        _state: &mut Self,
        _proxy: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _event: <zxdg_output_manager_v1::ZxdgOutputManagerV1 as Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, async_broadcast::Sender<String>> for GtkOutputsQueue {
    fn event(
        _state: &mut Self,
        proxy: &zxdg_output_v1::ZxdgOutputV1,
        event: <zxdg_output_v1::ZxdgOutputV1 as Proxy>::Event,
        data: &async_broadcast::Sender<String>,
        _conn: &Connection,
        _qhandle: &wayland_client::QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::Name { name } = event {
            task::block_on(async {
                data.broadcast_direct(name).await.unwrap();
                proxy.destroy();
            });
        }
    }
}

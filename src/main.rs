use async_std::sync::Mutex;
use async_std::task::{self, JoinHandle, Task};
use std::cell::RefCell;
use std::sync::{mpsc, Arc, Condvar};

use glib::clone;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4::{self as gtk, Label};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use hyprland::event_listener::EventListener;

struct HyprlandEvents {
    active_window: Arc<(Mutex<String>, Condvar)>,
}

impl HyprlandEvents {
    fn new() -> Self {
        Self {
            active_window: Arc::new((Mutex::new("".to_owned()), Condvar::new())),
        }
    }
}

unsafe impl Sync for HyprlandEvents {}
unsafe impl Send for HyprlandEvents {}

fn bar_window(app: &Application, hyprland_events: &HyprlandEvents) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.auto_exclusive_zone_enable();

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);

    let label = gtk::Label::new(Some("Test label"));
    window.set_child(Some(&label));

    glib::spawn_future_local(clone!(@weak label => async move {
        let (sender, receiver) = async_channel::bounded(1);
        gio::spawn_blocking(move || {
            let mut event_listener = hyprland::event_listener::EventListener::new();
            sender.send_blocking("Foo Bar".to_owned()).expect("Channel needs to be open");
            let sender = sender.clone();
            event_listener.add_active_window_change_handler(move |data| {
                println!("Got active window change notification");
                sender.send_blocking(data.unwrap().window_title).expect("Window title channel to be open");
            });

            println!("Starting listening");
            event_listener.start_listener().unwrap();
        });

        println!("Waiting on window title");
        while let Ok(active_window) = receiver.recv().await {
            println!("Got window title");
            label.set_text(&active_window);
        }
    }));

    window
}

fn activate(app: &Application, hyprland_events: &HyprlandEvents) {
    bar_window(app, hyprland_events).show()
}

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    let hyprland_events = HyprlandEvents::new();

    app.connect_activate(move |app| activate(app, &hyprland_events));

    app.run()
}

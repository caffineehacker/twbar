use std::sync::Arc;

use glib::clone;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4::{self as gtk, Button, Label, Orientation, Widget};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

mod hyprland;

use hyprland::events::HyprlandEvents;
use hyprland::windows::HyprlandWindows;

fn taskbar_widget(monitor: i32) -> Widget {
    let container = gtk::Box::new(Orientation::Horizontal, 8);

    glib::spawn_future_local(clone!(@weak container => async move {
        let hyprland_windows = HyprlandWindows::instance().await;
        let mut emitter = hyprland_windows.get_windows_update_emitter();

        loop {
            let mut windows = emitter.next().await;
            println!("Windows updated event!");
            windows.sort_by(|a, b| a.workspace.id.cmp(&b.workspace.id));

            while let Some(child) = container.last_child() {
                container.remove(&child);
            }

            // FIXME: CHECK FOR EXISTING CHILDREN AND ADD / REMOVE / REORDER
            for window in windows.iter().filter(|x| x.monitor == monitor) {
                let button = Button::new();
                button.set_label(&window.title);
                container.append(&button);
            }
        }
    }));

    container.into()
}

fn bar_window(app: &Application, hyprland_events: Arc<HyprlandEvents>) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.auto_exclusive_zone_enable();

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);

    let hbox = gtk::Box::new(Orientation::Horizontal, 8);

    let label = Label::new(Some("Window Label"));
    hbox.append(&label);
    hbox.append(&taskbar_widget(0));
    window.set_child(Some(&hbox));

    glib::spawn_future_local(clone!(@weak label, @strong hyprland_events => async move {
        let mut active_window_stream = hyprland_events.get_active_window_emitter();

        loop {
            let active_window = active_window_stream.next().await;
            label.set_text(&active_window.title);
        }
    }));

    window
}

fn activate(app: &Application, hyprland_events: Arc<HyprlandEvents>) {
    bar_window(app, hyprland_events).show()
}

#[async_std::main]
async fn main() -> Result<glib::ExitCode, ()> {
    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    let hyprland_events = HyprlandEvents::instance().await;

    app.connect_activate(
        clone!(@strong hyprland_events => move |app| activate(app, hyprland_events.clone())),
    );

    Ok(app.run())
}

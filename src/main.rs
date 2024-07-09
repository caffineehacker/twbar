use std::sync::Arc;

use glib::clone;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4::{self as gtk, Label};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

mod hyprland;

use hyprland::events::HyprlandEvents;

fn bar_window(app: &Application, hyprland_events: Arc<HyprlandEvents>) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.auto_exclusive_zone_enable();

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);

    let label = Label::new(Some("Window Label"));
    window.set_child(Some(&label));

    glib::spawn_future_local(clone!(@weak label, @strong hyprland_events => async move {
        let mut active_window_stream = hyprland_events.get_active_window_emitter().await;

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

    let hyprland_events = Arc::new(HyprlandEvents::new().await);

    app.connect_activate(
        clone!(@strong hyprland_events => move |app| activate(app, hyprland_events.clone())),
    );

    Ok(app.run())
}

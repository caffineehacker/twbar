use std::collections::HashMap;
use std::sync::Arc;

use gio::glib::property::PropertyGet;
use gio::glib::WeakRef;
use glib::clone;
use gtk::prelude::*;
use gtk::{glib, Application, ApplicationWindow};
use gtk4::gdk::{Display, Monitor};
use gtk4::{self as gtk, Button, Label, Orientation, Widget};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

mod hyprland;
mod xdg_applications;

use hyprland::commands::HyprlandCommands;
use hyprland::events::HyprlandEvents;
use hyprland::windows::{HyprlandWindow, HyprlandWindows};
use xdg_applications::XdgApplicationsCache;

fn taskbar_button(window: &HyprlandWindow, cache: &XdgApplicationsCache) -> Button {
    let button = Button::new();
    button.set_focusable(false);

    assert!(!window.address.is_empty());
    let address = window.address.clone();
    button.connect_clicked(move |_button| {
        let address = address.clone();
        glib::spawn_future_local(async move {
            HyprlandCommands::set_active_window(&address).await;
        });
    });
    update_taskbar_button(&button, window, cache);

    button
}

fn update_taskbar_button(button: &Button, window: &HyprlandWindow, cache: &XdgApplicationsCache) {
    if !window.title.is_empty() {
        // Any tooltip currently seg faults on hover for some reason...
        //button.set_tooltip_text(Some(&window.title));
    }

    let mut app_info = cache.get_application_by_class(&window.initial_class);
    if app_info.is_none() {
        app_info = cache.get_application_by_class(&window.class);
    }

    if app_info.is_some() {
        let app_info = app_info.unwrap();
        let icon = app_info.string("Icon");
        if icon.is_some() {
            let button_box = gtk::Box::new(Orientation::Horizontal, 8);
            let image = gtk::Image::new();
            image.set_icon_name(icon.unwrap().as_str().into());
            button_box.append(&image);
            let label = Label::new(app_info.name().as_str().into());
            button_box.append(&label);
            button.set_child(Some(&button_box));
        } else {
            button.set_label(app_info.name().as_str());
        }
    } else {
        button.set_label(&window.initial_class);
    }
}

fn taskbar_widget(monitor: i32) -> Widget {
    let container = gtk::Box::new(Orientation::Horizontal, 8);

    glib::spawn_future_local(clone!(@weak container => async move {
        let application_cache = XdgApplicationsCache::new();
        let hyprland_windows = HyprlandWindows::instance().await;
        let mut windows = hyprland_windows.get_windows_update_emitter();

        let mut buttons: Vec<(HyprlandWindow, Button)> = Vec::new();

        loop {
            let mut current_windows = windows.next().await;

            current_windows.sort_by(|a, b| a.workspace.id.cmp(&b.workspace.id));
            let current_windows: Vec<HyprlandWindow> = current_windows.into_iter().filter(|w| w.monitor == monitor).collect();
            let number_of_windows = current_windows.len();

            for (index, window) in current_windows.into_iter().enumerate() {
                let current_value = buttons.iter_mut().enumerate().find(|(_, (w, _b))| w.address == window.address);
                if current_value.is_some() {
                    let (current_index, (current_window, current_button)) = current_value.unwrap();
                    if window != *current_window {
                        update_taskbar_button(&current_button, &current_window, &application_cache);
                        *current_window = window;
                    }
                    if current_index != index {
                        let current_button = current_button.clone();
                        buttons.swap(index, current_index);
                        let mut sibling = None;
                        if index > 0 {
                            sibling = buttons.get(index - 1).map(|(_, b)| b);
                        }
                        container.reorder_child_after(&current_button, sibling)
                    }
                } else {
                    println!("Adding current window {}: {:?}", index, window);
                    let button = taskbar_button(&window, &application_cache);
                    if index > 0 {
                        let current_button = buttons.get(index - 1).unwrap();
                        println!("New sibling: {:?}", current_button.1);
                        container.append(&button);
                        container.reorder_child_after(&button, buttons.get(index - 1).map(|(_, b)| b))
                    } else {
                        container.prepend(&button);
                    }
                    buttons.insert(index, (window, button));
                }
            }

            for i in number_of_windows..buttons.len() {
                container.remove(&buttons.get(i).unwrap().1);
            }
            buttons.truncate(number_of_windows);
        }
    }));

    container.into()
}

fn bar_window(app: &Application, monitor: &Monitor, hyprland_events: Arc<HyprlandEvents>) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.auto_exclusive_zone_enable();
    window.set_monitor(monitor);

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);

    let vbox = gtk::Box::new(Orientation::Vertical, 1);

    // FIXME: Figure out the monitor index.
    //let connector = monitor.connector();

    let hbox = gtk::Box::new(Orientation::Horizontal, 8);
    hbox.append(&taskbar_widget(0));
    

    vbox.append(&hbox);
    
    let label = Label::new(Some("Window Label"));
    vbox.append(&label);
    window.set_child(Some(&vbox));

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
    // FIXME: Handle multiple monitors and remove / add bars as needed
    let display = Display::default().unwrap();
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        bar_window(app, monitors.item(i).unwrap().dynamic_cast_ref().unwrap(), hyprland_events.clone()).show()
    }
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

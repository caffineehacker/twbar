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
use hyprland::monitors::HyprlandMonitors;
use hyprland::windows::{HyprlandWindow, HyprlandWindows};
use hyprland::workspaces::{HyprlandWorkspace, HyprlandWorkspaces};
use xdg_applications::XdgApplicationsCache;

fn workspace_button(workspace: &HyprlandWorkspace) -> gtk::Button {
    let button = gtk::Button::new();

    button.set_label(&workspace.name);

    let workspace_id = workspace.id;
    button.connect_clicked(move |_b| {
        glib::spawn_future_local(async move {
            HyprlandCommands::set_active_workspace(workspace_id).await;
        });
    });

    button
}

fn workspaces_bar(monitor_id: i32) -> gtk::Widget {
    let container = gtk::Box::new(Orientation::Horizontal, 1);

    glib::spawn_future_local(clone!(@weak container => async move {
        let hyprland_workspaces = HyprlandWorkspaces::instance().await;
        let mut workspaces_state = hyprland_workspaces.get_workspaces_state_emitter();

        loop {
            let workspaces = workspaces_state.next().await;
            let mut workspaces: Vec<HyprlandWorkspace> = workspaces.into_iter().filter(|w| w.monitor_id == monitor_id).collect();
            workspaces.sort_by_key(|w| w.id);
            
            println!("Redoing workspace buttons");
            while let Some(button) = container.first_child() {
                container.remove(&button);
            }

            for workspace in workspaces.iter() {
                container.append(&workspace_button(workspace));
            }
        }
    }));

    container.into()
}

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
        // button.set_tooltip_text(Some(&window.title));
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

fn bar_window(app: &Application, monitor: &Monitor) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.auto_exclusive_zone_enable();
    window.set_monitor(monitor);

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);
    window.set_default_height(1);

    let connector = monitor.connector().map(|c| c.as_str().to_owned()).unwrap_or("".to_owned());
    glib::spawn_future_local(clone!(@strong window => async move {
        let hyprland_monitors = HyprlandMonitors::instance().await;
        let mut monitors_emitter = hyprland_monitors.get_monitor_state_emitter();
        let monitors = monitors_emitter.next().await;
        let hyprland_monitor = monitors.iter().find(|m| m.name == connector).expect(&format!("Failed to find monitor match {}", connector));

        let hbox = gtk::Box::new(Orientation::Horizontal, 8);
        hbox.append(&workspaces_bar(hyprland_monitor.id));
        hbox.append(&taskbar_widget(hyprland_monitor.id));
        
        let vbox = gtk::Box::new(Orientation::Vertical, 1);
        vbox.append(&hbox);
        
        let label = Label::new(Some("Window Label"));
        vbox.append(&label);
        window.set_child(Some(&vbox));

        glib::spawn_future_local(clone!(@weak label => async move {
            let events = HyprlandEvents::instance().await;
            let mut active_window_stream = events.get_active_window_emitter();

            loop {
                let active_window = active_window_stream.next().await;
                label.set_text(&active_window.title);
            }
        }));
        window.show();
    }));

    window
}

fn activate(app: &Application) {
    // FIXME: Handle multiple monitors and remove / add bars as needed
    let display = Display::default().unwrap();
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        bar_window(app, monitors.item(i).unwrap().dynamic_cast_ref().unwrap());
    }
}

#[async_std::main]
async fn main() -> Result<glib::ExitCode, ()> {
    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    app.connect_activate(
        move |app| {
            activate(&app)
        }
    );

    Ok(app.run())
}

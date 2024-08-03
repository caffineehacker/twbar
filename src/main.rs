use std::cell::RefCell;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use async_std::task;
use glib::clone;
use gtk4::gdk::{Display, Monitor};
use gtk4::prelude::DisplayExt;
use gtk4::{self as gtk, Button, CssProvider, DebugFlags, Label, Orientation, Widget};
use gtk4::{glib, prelude::*};
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use gtk_output::GtkOutputs;
use std::process::Command;
use wayland_client::Proxy;

mod gtk_output;
mod hyprland;
mod xdg_applications;

use hyprland::commands::HyprlandCommands;
use hyprland::events::HyprlandEvents;
use hyprland::monitors::HyprlandMonitors;
use hyprland::windows::{HyprlandWindow, HyprlandWindows};
use hyprland::workspaces::{HyprlandWorkspace, HyprlandWorkspaces};
use xdg_applications::XdgApplicationsCache;

fn launch_wofi_button() -> Button {
    let button = gtk::Button::new();
    let container = gtk::Box::new(Orientation::Horizontal, 0);
    let label = gtk::Label::new(Some(""));
    label.set_halign(gtk4::Align::Center);
    container.append(&label);
    container.set_halign(gtk4::Align::Center);
    button.set_child(Some(&container));
    button.set_has_frame(false);
    // button.add_css_class("workspace");
    button.add_css_class("circular");
    button.set_focusable(false);

    button.connect_clicked(|_button| {
        Command::new("pkill")
            .args(["wofi"])
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .output()
            .ok();

        Command::new("wofi")
            .args(["-c", "/home/tim/.config/wofi/config-bmenu"])
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .ok();
    });

    button
}

// "custom/launch_wofi" = {
//                 format = "";
//                 on-click = "pkill wofi || wofi -c ~/.config/wofi/config-bmenu";
//                 tooltip = false;
//               };

//               "custom/power_btn" = {
//                 format = "";
//                 on-click = "sh -c '(sleep 0.5s; wlogout --protocol layer-shell)' & disown";
//                 tooltip = false;
//               };

//               "custom/lock_screen" = {
//                 format = "";
//                 on-click = "sh -c '(sleep 0.5s; swaylock)' & disown";
//                 tooltip = false;
//               };

fn workspace_button(workspace: &HyprlandWorkspace) -> gtk::Button {
    let button = gtk::Button::new();

    let container = gtk::Box::new(Orientation::Horizontal, 0);
    let label = gtk::Label::new(Some(&workspace.name));
    label.set_halign(gtk4::Align::Center);
    container.append(&label);
    container.set_halign(gtk4::Align::Center);
    button.set_child(Some(&container));
    button.set_has_frame(false);
    button.add_css_class("workspace");
    button.add_css_class("circular");
    button.set_focusable(false);

    let workspace_id = workspace.id;
    button.connect_clicked(move |_b| {
        glib::spawn_future_local(async move {
            HyprlandCommands::set_active_workspace(workspace_id).await;
        });
    });

    button
}

fn workspaces_bar(monitor_id: i32) -> gtk::Widget {
    let container = gtk::Box::new(Orientation::Horizontal, 0);
    container.set_widget_name("workspaces");

    glib::spawn_future_local(clone!(
        #[weak]
        container,
        async move {
            let hyprland_workspaces = HyprlandWorkspaces::instance().await;
            let mut workspaces_state = hyprland_workspaces.get_workspaces_state_emitter();

            loop {
                let workspaces = workspaces_state.next().await;
                let mut workspaces: Vec<HyprlandWorkspace> = workspaces
                    .into_iter()
                    .filter(|w| w.monitor_id == monitor_id)
                    .collect();
                workspaces.sort_by_key(|w| w.id);

                println!("Redoing workspace buttons");
                while let Some(button) = container.first_child() {
                    container.remove(&button);
                }

                for workspace in workspaces.iter() {
                    container.append(&workspace_button(workspace));
                }
            }
        }
    ));

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
    update_taskbar_button(&button, window, None, cache);

    button
}

fn update_taskbar_button(
    button: &Button,
    current_window: &HyprlandWindow,
    previous_window: Option<&HyprlandWindow>,
    cache: &XdgApplicationsCache,
) {
    if !current_window.title.is_empty() {
        // Any tooltip currently seg faults on hover for some reason...
        // button.set_tooltip_text(Some(&current_window.title));
    }

    if previous_window.is_some_and(|pw| {
        pw.class != current_window.class || pw.initial_class != current_window.initial_class
    }) || previous_window.is_none()
    {
        let mut app_info = cache.get_application_by_class(&current_window.initial_class);
        if app_info.is_none() {
            app_info = cache.get_application_by_class(&current_window.class);
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
            button.set_label(&current_window.initial_class);
        }
    }
}

fn taskbar_widget(monitor: i32) -> Widget {
    let container = gtk::Box::new(Orientation::Horizontal, 8);

    glib::spawn_future_local(clone!(
        #[weak]
        container,
        async move {
            let application_cache = XdgApplicationsCache::new();
            let hyprland_windows = HyprlandWindows::instance().await;
            let mut windows = hyprland_windows.get_windows_update_emitter();

            let mut buttons: Vec<(HyprlandWindow, Button)> = Vec::new();

            loop {
                let mut new_windows = windows.next().await;

                new_windows.sort_by(|a, b| a.workspace.id.cmp(&b.workspace.id));
                let new_windows: Vec<HyprlandWindow> = new_windows
                    .into_iter()
                    .filter(|w| w.monitor == monitor)
                    .collect();
                let number_of_windows = new_windows.len();

                for (index, new_window) in new_windows.into_iter().enumerate() {
                    let previous_value = buttons
                        .iter_mut()
                        .enumerate()
                        .find(|(_, (w, _b))| w.address == new_window.address);
                    if previous_value.is_some() {
                        let (current_index, (previous_window, current_button)) =
                            previous_value.unwrap();
                        if new_window != *previous_window {
                            update_taskbar_button(
                                current_button,
                                &new_window,
                                Some(previous_window),
                                &application_cache,
                            );
                            *previous_window = new_window;
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
                        println!("Adding current window {}: {:?}", index, new_window);
                        let button = taskbar_button(&new_window, &application_cache);
                        if index > 0 {
                            let current_button = buttons.get(index - 1).unwrap();
                            println!("New sibling: {:?}", current_button.1);
                            container.append(&button);
                            container.reorder_child_after(
                                &button,
                                buttons.get(index - 1).map(|(_, b)| b),
                            )
                        } else {
                            container.prepend(&button);
                        }
                        buttons.insert(index, (new_window, button));
                    }
                }

                for i in number_of_windows..buttons.len() {
                    container.remove(&buttons.get(i).unwrap().1);
                }
                buttons.truncate(number_of_windows);
            }
        }
    ));

    container.into()
}

fn bar_window(app: &Application, monitor: &Monitor, connector: &str) -> ApplicationWindow {
    let window = ApplicationWindow::new(app);

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.auto_exclusive_zone_enable();
    window.set_monitor(monitor);
    // We use this for the interactive UI debugger since we need ctrl+shift+I to open it.
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);

    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Top, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Bottom, false);
    window.set_default_height(1);

    let connector = connector.to_owned();
    glib::spawn_future_local(clone!(
        #[strong]
        window,
        async move {
            let hyprland_monitors = HyprlandMonitors::instance().await;
            let mut monitors_emitter = hyprland_monitors.get_monitor_state_emitter();
            let monitors = monitors_emitter.next().await;
            let hyprland_monitor = monitors
                .iter()
                .find(|m| m.name == connector)
                .unwrap_or_else(|| panic!("Failed to find monitor match {}", connector));

            let hbox = gtk::Box::new(Orientation::Horizontal, 8);
            hbox.append(&launch_wofi_button());
            hbox.append(&workspaces_bar(hyprland_monitor.id));
            hbox.append(&taskbar_widget(hyprland_monitor.id));

            let vbox = gtk::Box::new(Orientation::Vertical, 1);
            vbox.append(&hbox);

            let label = Label::new(Some("Window Label"));
            vbox.append(&label);
            window.set_child(Some(&vbox));

            glib::spawn_future_local(clone!(
                #[weak]
                label,
                async move {
                    let events = HyprlandEvents::instance().await;
                    let mut active_window_stream = events.get_active_window_emitter();

                    loop {
                        let active_window = active_window_stream.next().await;
                        label.set_text(&active_window.title);
                    }
                }
            ));
            window.set_visible(true);
        }
    ));

    window
}

fn activate(app: &Application) {
    let display = Display::default().unwrap();

    let monitors = display.monitors();
    let windows = Arc::new(RefCell::new(HashMap::new()));
    for i in 0..monitors.n_items() {
        let monitor = monitors.item(i).unwrap();
        let monitor: &Monitor = monitor.dynamic_cast_ref().unwrap();
        if let Some(connector) = monitor.connector().map(|c| c.as_str().to_owned()) {
            windows.borrow_mut().insert(
                connector.clone(),
                bar_window(app, monitor, &connector).downgrade(),
            );
        }
    }

    glib::spawn_future_local(clone!(
        #[weak]
        app,
        #[strong]
        monitors,
        async move {
            let gtk_outputs = GtkOutputs::instance().await;
            monitors.connect_items_changed(clone!(
                #[weak]
                app,
                #[strong]
                windows,
                #[strong]
                gtk_outputs,
                move |monitors, _position, _removed, _added| {
                    glib::spawn_future_local(clone!(
                        #[weak]
                        app,
                        #[strong]
                        windows,
                        #[strong]
                        monitors,
                        #[strong]
                        gtk_outputs,
                        async move {
                            let mut windows = windows.borrow_mut();
                            let monitor_names = (0..monitors.n_items())
                                .map(|index| {
                                    let gdk_monitor = monitors.item(index).unwrap();
                                    let gdk_monitor: &Monitor =
                                        gdk_monitor.dynamic_cast_ref().unwrap();
                                    if let Some(gdk_connector) =
                                        gdk_monitor.connector().map(|c| c.as_str().to_owned())
                                    {
                                        (gdk_monitor.clone(), gdk_connector)
                                    } else {
                                        let gtk_outputs = gtk_outputs.clone();
                                        (
                                            gdk_monitor.clone(),
                                            task::block_on(async move {
                                                gtk_outputs.get_name(gdk_monitor).await
                                            }),
                                        )
                                    }
                                })
                                .collect::<Vec<(Monitor, String)>>();
                            // First remove any windows that are not in the new list
                            'windows_loop: for (connector, window) in windows.clone().iter() {
                                for (_, name) in monitor_names.iter() {
                                    if *name == *connector {
                                        println!("Monitor {} is still connected", connector);
                                        continue 'windows_loop;
                                    }
                                }

                                if let Some(window) = window.upgrade() {
                                    window.close();
                                }
                                windows.remove(connector.as_str());
                            }

                            // Now add new monitors
                            for (monitor, name) in monitor_names.iter() {
                                if !windows.contains_key(name) {
                                    println!("New monitor found: {}", name);
                                    windows.insert(
                                        name.clone(),
                                        bar_window(&app, monitor, name).downgrade(),
                                    );
                                }
                            }
                        }
                    ));
                }
            ));
        }
    ));

    // glib::spawn_future_local(clone!(@weak app => async move {
    //     let monitor_events = HyprlandMonitors::instance().await;
    //     let mut monitor_stream = monitor_events.get_monitor_state_emitter();

    //     loop {
    //         let _ = monitor_stream.next().await;

    //         println!("Monitors changed!");

    //         let display = Display::default().unwrap();
    //         let gdk_monitors = display.monitors();

    //         // First remove any windows that are not in the new list
    //         'windows_loop: for (connector, window) in windows.clone().iter() {
    //             for i in 0..gdk_monitors.n_items() {
    //                 let gdk_monitor = gdk_monitors.item(i).unwrap();
    //                 let gdk_monitor: &Monitor = gdk_monitor.dynamic_cast_ref().unwrap();
    //                 let gdk_connector = gdk_monitor
    //                     .connector()
    //                     .map(|c| c.as_str().to_owned()).unwrap();

    //                 if gdk_connector == *connector {
    //                     println!("Monitor {} is still connected", connector);
    //                     continue 'windows_loop
    //                 }
    //             }

    //             if let Some(window) = window.upgrade() {
    //                 window.close();
    //             }
    //             windows.remove(connector.as_str());
    //         }

    //         // Now add new monitors
    //         for i in 0..gdk_monitors.n_items() {
    //             let gdk_monitor = gdk_monitors.item(i).unwrap();
    //             let gdk_monitor: &Monitor = gdk_monitor.dynamic_cast_ref().unwrap();
    //             let gdk_connector = gdk_monitor
    //                 .connector()
    //                 .map(|c| c.as_str().to_owned()).unwrap();

    //             if !windows.contains_key(&gdk_connector) {
    //                 println!("New monitor found: {}", gdk_connector);
    //                 windows.insert(
    //                     gdk_connector.clone(),
    //                     bar_window(&app, gdk_monitor, &gdk_connector).downgrade(),
    //                 );
    //             }
    //         }
    //     }
    // }));

    gtk::set_debug_flags(DebugFlags::INTERACTIVE);
}

#[async_std::main]
async fn main() -> Result<glib::ExitCode, ()> {
    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    app.connect_startup(|_| {
        let provider = CssProvider::new();
        provider.load_from_string(
            "
.workspace {
    padding: 5px;
    margin-right: 5px;
    border-radius: 10px;
}

#workspaces button {
    padding: 5px;
    margin-right: 5px;
}

#workspaces button.active {
    border-radius: 10px;
}

#workspaces button:hover {
    border-radius: 10px;
}

#workspaces {
    opacity: 1;
    padding: 0px 8px;
    margin: 0px 3px;
    border: 0px;
}

#workspaces {
    padding-right: 0px;
    padding-left: 5px;
}
        ",
        );
        gtk::style_context_add_provider_for_display(
            &Display::default().unwrap(),
            &provider,
            // We want to override the user style. Otherwise nothing actually applies because I have most settings already set.
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    });

    app.connect_activate(activate);

    Ok(app.run())
}

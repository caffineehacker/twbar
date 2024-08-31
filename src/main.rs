use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use async_std::task;
use glib::clone;
use gtk4::gdk::{Display, Monitor};
use gtk4::prelude::DisplayExt;
use gtk4::{self as gtk, CssProvider, DebugFlags, Label, Orientation};
use gtk4::{glib, prelude::*};
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use gtk_output::GtkOutputs;
use log::trace;
use widgets::command_button::ButtonCommand;

mod gtk_output;
mod hyprland;
mod widgets;
mod xdg_applications;

use hyprland::events::HyprlandEvents;
use hyprland::monitors::HyprlandMonitors;

fn launch_wofi_button() -> gtk::Widget {
    widgets::command_button::CommandButton::new(
        "",
        vec![
            ButtonCommand {
                command: "pkill".to_owned(),
                args: vec!["wofi".to_owned()],
                allow_failure: true,
            },
            ButtonCommand {
                command: "wofi".to_owned(),
                args: vec![
                    "-c".to_owned(),
                    "/home/tim/.config/wofi/config-bmenu".to_owned(),
                ],
                allow_failure: true,
            },
        ],
    )
    .into()
}

fn power_button() -> gtk::Widget {
    widgets::command_button::CommandButton::new(
        "",
        vec![ButtonCommand {
            command: "sh".to_owned(),
            args: vec![
                "-c".to_owned(),
                "(sleep 0.5s; wlogout --protocol layer-shell) & disown".to_owned(),
            ],
            allow_failure: false,
        }],
    )
    .into()
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
            hbox.append(&power_button());
            hbox.append(&widgets::workspaces::Workspaces::new(hyprland_monitor.id));
            hbox.append(&widgets::taskbar::Taskbar::new(hyprland_monitor.id));
            hbox.append(&widgets::cpu_usage::CpuUsage::new());

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
                            trace!("Monitors changed");
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
                                        trace!("Monitor {} is still connected", connector);
                                        continue 'windows_loop;
                                    }
                                }

                                if let Some(window) = window.upgrade() {
                                    trace!(monitor_connector = connector.as_str(); "Closing window due to monitor removal");
                                    window.close();
                                }
                                windows.remove(connector.as_str());
                            }

                            // Now add new monitors
                            for (monitor, name) in monitor_names.iter() {
                                if !windows.contains_key(name) {
                                    trace!(monitor_name = name.as_str(); "New monitor found");
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

    gtk::set_debug_flags(DebugFlags::INTERACTIVE);
}

#[async_std::main]
async fn main() -> Result<glib::ExitCode, ()> {
    env_logger::init();

    let app = Application::builder()
        .application_id("com.timwaterhouse.twbar")
        .build();

    app.connect_startup(|_| {
        let provider = CssProvider::new();
        provider.load_from_string(
            "
.workspace_button {
    padding: 5px;
    margin-right: 0px;
}

.workspace_button.active {
	background-color: rgba(198,208,245,0.12);
}

.workspaces {
    padding: 0px 8px;
    margin: 0px 3px;
    border: 0px;
    padding-right: 0px;
    padding-left: 5px;
}

.taskbar_button {
    border-radius: 0px;
    padding-left: 8px;
    padding-right: 8px;
}

.taskbar_button.active {
	background-color: rgba(198,208,245,0.12);
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

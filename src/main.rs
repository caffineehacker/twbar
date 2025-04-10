use glib::clone;
use gtk_output::GtkOutputs;
use gtk4::gdk::{Display, Monitor};
use gtk4::prelude::DisplayExt;
use gtk4::{self as gtk, Align, CssProvider, DebugFlags, Label, Orientation};
use gtk4::{Application, ApplicationWindow};
use gtk4::{glib, prelude::*};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use log::trace;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
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
    trace!("In bar_window");
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
    trace!("bar_window - about to spawn future local");
    glib::spawn_future_local(clone!(
        #[strong]
        window,
        async move {
            trace!("In bar_window - future_local");
            let hyprland_monitors = HyprlandMonitors::instance().await;
            trace!("bar_window - future local - hyprland monitors have instance");
            let mut monitors_emitter = hyprland_monitors.get_monitor_state_emitter();
            trace!("bar_window - future local - hyprland monitors have state emitter");
            let monitors = monitors_emitter.next().await;
            trace!("bar_window - future local - have monitors");
            let hyprland_monitor = monitors
                .iter()
                .find(|m| m.name == connector)
                .unwrap_or_else(|| panic!("Failed to find monitor match {}", connector));
            trace!("bar_window - future local - found monitor match");

            let left_box = gtk::Box::new(Orientation::Horizontal, 8);
            left_box.set_halign(Align::Start);
            trace!("bar_window - future local - adding wofi button");
            left_box.append(&launch_wofi_button());
            trace!("bar_window - future local - adding power button");
            left_box.append(&power_button());
            trace!("bar_window - future local - adding workspaces widget");
            left_box.append(&widgets::workspaces::Workspaces::new(hyprland_monitor.id));

            let center_box = gtk::Box::new(Orientation::Horizontal, 8);
            trace!("bar_window - future local - adding taskbar widget");
            center_box.append(&widgets::taskbar::Taskbar::new(hyprland_monitor.id));

            let right_box = gtk::Box::new(Orientation::Horizontal, 8);
            trace!("bar_window - future local - adding cpu widget");
            right_box.append(&widgets::cpu_usage::CpuUsage::new());
            trace!("bar_window - future local - adding ram widget");
            right_box.append(&widgets::ram_usage::RamUsage::new());
            trace!("bar_window - future local - adding battery info widget");
            right_box.append(&widgets::battery_info::BatteryInfo::new());
            trace!("bar_window - future local - all widgets added");
            right_box.append(&widgets::clock::Clock::new());

            let hbox = gtk::CenterBox::new();
            hbox.set_start_widget(Some(&left_box));
            hbox.set_center_widget(Some(&center_box));
            hbox.set_end_widget(Some(&right_box));

            let vbox = gtk::Box::new(Orientation::Vertical, 1);
            vbox.append(&hbox);

            let label = Label::new(Some("Window Label"));
            vbox.append(&label);
            window.set_child(Some(&vbox));

            glib::spawn_future_local(clone!(
                #[weak]
                label,
                async move {
                    trace!("bar_window - future local - future local");
                    let events = HyprlandEvents::instance().await;
                    trace!("bar_window - future local - future local - have events instance");
                    let mut active_window_stream = events.get_active_window_emitter();
                    trace!("bar_window - future local - future local - have active window stream");

                    loop {
                        let active_window = active_window_stream.next().await;
                        label.set_text(&active_window.title);
                    }
                }
            ));
            trace!("bar_window - future local - setting window visible");
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
                            let monitor_names =
                                futures::future::join_all((0..monitors.n_items()).map(|index| {
                                    let gdk_monitor = monitors.item(index).unwrap();
                                    let gdk_monitor: Monitor = gdk_monitor.dynamic_cast().unwrap();
                                    let gtk_outputs = gtk_outputs.clone();
                                    async move {
                                        match gdk_monitor.connector().map(|c| c.as_str().to_owned())
                                        {
                                            Some(gdk_connector) => {
                                                Some((gdk_monitor.clone(), gdk_connector))
                                            }
                                            _ => {
                                                let output_name =
                                                    gtk_outputs.get_name(&gdk_monitor).await;
                                                if let Ok(name) = output_name {
                                                    Some((gdk_monitor.clone(), name))
                                                } else {
                                                    None
                                                }
                                            }
                                        }
                                    }
                                }))
                                .await
                                .into_iter()
                                .flatten()
                                .collect::<Vec<(Monitor, String)>>();
                            trace!("Monitors: {:?}", monitor_names);
                            // First remove any windows that are not in the new list
                            let mut windows = windows.borrow_mut();
                            'windows_loop: for (connector, window) in windows.clone().iter() {
                                for (_, name) in monitor_names.iter() {
                                    if *name == *connector {
                                        trace!("Monitor {} is still connected", connector);
                                        continue 'windows_loop;
                                    }
                                }

                                if let Some(window) = window.upgrade() {
                                    trace!(
                                        "Closing window due to monitor removal: {}",
                                        connector.as_str()
                                    );
                                    window.close();
                                }
                                windows.remove(connector.as_str());
                            }

                            // Now add new monitors
                            for (monitor, name) in monitor_names.iter() {
                                if !windows.contains_key(name) {
                                    trace!("New monitor found: {}", name.as_str());
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
    trace!("Booting app");

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

tooltip {
    background: rgba(198,208,245,0.12);
    opacity: 0.8;
    border-radius: 10px;
    border-width: 2px;
    border-style: solid;
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

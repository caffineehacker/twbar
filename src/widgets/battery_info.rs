use std::cell::OnceCell;
use std::collections::HashMap;
use std::fs::{self};
use std::io::Error;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::thread;
use std::time::Duration;

use async_std::channel;
use async_std::sync::{Arc, Barrier, Mutex, Weak};
use async_std::task::{self, sleep};
use async_std::{fs::File, io::ReadExt};

use gio::glib::clone::Downgrade;
use gio::glib::{clone, SendWeakRef, WeakRef};
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Buildable, ConstraintTarget, EventControllerMotion, Orientable, Popover,
    Widget,
};
use gtk4::{prelude::*, Label};
use log::trace;
use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token};
use udev::{Device, Enumerator, MonitorBuilder};

struct BatteryData {
    syspath: String,
    charge: i64,
    time_to_empty: i64,
}

impl BatteryData {
    fn new(syspath: String) -> BatteryData {
        BatteryData {
            syspath,
            charge: -1,
            time_to_empty: -1,
        }
    }
    fn update(&mut self) {
        // TODO: GET CHARGE LIMIT, TIME TO FULL, TIME TO EMPTY, ETC...
        match fs::read_to_string(self.syspath.clone() + "/charge_now") {
            Ok(v) => match v.trim().parse::<i64>() {
                Ok(charge) => {
                    self.charge = charge;
                }
                Err(err) => {
                    log::error!("Failed to parse: {}, {}", v, err);
                }
            },
            Err(_) => {
                log::error!("{}: failed to read charge", self.syspath);
            }
        };
    }
}

struct MainsData {
    syspath: String,
    present: bool,
}

impl MainsData {
    fn new(syspath: String) -> MainsData {
        MainsData {
            syspath,
            present: false,
        }
    }
}

struct BatteryListener {
    controls: Mutex<Vec<SendWeakRef<BatteryInfo>>>,
    batteries: Mutex<HashMap<String, BatteryData>>,
    // True if plugged in, false otherwise.
    mains: Mutex<HashMap<String, MainsData>>,
}

impl BatteryListener {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<BatteryListener>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new().await;
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    async fn new() -> Arc<Self> {
        trace!("BatterListener::new");
        let instance = Arc::new(Self {
            controls: Mutex::new(Vec::new()),
            batteries: Mutex::new(HashMap::new()),
            mains: Mutex::new(HashMap::new()),
        });

        // Process:
        // - Lock the batteries and mains mutexes
        // - Start the polling thread
        // - Wait for polling thread to create the event listener
        // - Enumerate devices
        // - Unlock the mutexes
        let instance_clone = instance.clone();
        let mut batteries_lock = instance_clone.batteries.lock().await;
        let mut mains_lock = instance_clone.mains.lock().await;

        let barrier = Arc::new(Barrier::new(2));

        let weak_me = Arc::downgrade(&instance);
        let listener_barrier = barrier.clone();
        task::spawn(async move {
            let event_monitor = MonitorBuilder::new()
                .unwrap()
                .match_subsystem("power_supply")
                .unwrap()
                .listen()
                .unwrap();

            let mut poll = Poll::new().unwrap();
            let mut events = Events::with_capacity(1);
            poll.registry()
                .register(
                    &mut SourceFd(&event_monitor.as_fd().as_raw_fd()),
                    Token(0),
                    Interest::READABLE,
                )
                .unwrap();

            task::block_on(async { listener_barrier.wait().await });

            loop {
                if let Err(_) = poll.poll(&mut events, None) {
                    continue;
                }
                for event in event_monitor.iter() {
                    log::info!("{:#?}", event);
                    if let Some(power_supply_type) =
                        event.device().property_value("POWER_SUPPLY_TYPE")
                    {
                        if power_supply_type == "Battery" {
                            match weak_me.upgrade() { Some(me) => {
                                let device = event.device();
                                let sys_path = device.syspath().to_string_lossy();
                                let mut batteries_lock = task::block_on(me.batteries.lock());
                                if !batteries_lock.contains_key(&sys_path.to_string()) {
                                    batteries_lock.insert(
                                        sys_path.to_string(),
                                        BatteryData::new(sys_path.to_owned().to_string()),
                                    );
                                }
                            } _ => {
                                return;
                            }}
                        } else if power_supply_type == "Mains" {
                            match weak_me.upgrade() { Some(me) => {
                                let device = event.device();
                                let sys_path = device.syspath().to_string_lossy();
                                let mut mains_lock = task::block_on(me.mains.lock());
                                if !mains_lock.contains_key(&sys_path.to_string()) {
                                    mains_lock.insert(
                                        sys_path.to_string(),
                                        MainsData::new(sys_path.to_owned().to_string()),
                                    );
                                }
                            } _ => {
                                return;
                            }}
                        }
                    }
                }

                thread::sleep(Duration::from_secs(10));
            }
        });

        barrier.wait().await;

        let mut device_enumerator = Enumerator::new().unwrap();
        device_enumerator.match_subsystem("power_supply").unwrap();
        for device in device_enumerator.scan_devices().unwrap() {
            let device_type = device.property_value("POWER_SUPPLY_TYPE");
            if device_type.is_some_and(|t| t == "Battery") {
                log::info!(
                    "Battery found: {:?}",
                    device.property_value("POWER_SUPPLY_NAME")
                );
                let syspath = device.syspath().to_string_lossy();
                batteries_lock.insert(syspath.to_string(), BatteryData::new(syspath.to_string()));
            } else if device_type.is_some_and(|t| t == "Mains") {
                log::info!(
                    "Mains found: {:?}",
                    device.property_value("POWER_SUPPLY_NAME")
                );
                let syspath = device.syspath().to_string_lossy();
                mains_lock.insert(syspath.to_string(), MainsData::new(syspath.to_string()));
            }
            log::trace!("{:?}", device.syspath());
            log::trace!("  [properties]");
            for property in device.properties() {
                log::trace!("    - {:?} {:?}", property.name(), property.value());
            }

            log::trace!("  [attributes]");
            for attribute in device.attributes() {
                log::trace!("    - {:?} {:?}", attribute.name(), attribute.value());
            }
        }

        // TODO: Make a loop which executes every N seconds and asks all of the batteries and mains to update their status. Then update the controls.
        // This should also wake up anytime a batter or mains is inserted / removed.
        let me = instance.clone();
        glib::spawn_future(async move {
            loop {
                let mut batteries = me.batteries.lock().await;
                batteries.values_mut().for_each(|battery| battery.update());

                let charge: i64 = batteries
                    .values()
                    .map(|battery| battery.charge)
                    .sum::<i64>()
                    / (batteries.len() as i64);
                let time_to_empty = batteries
                    .values()
                    .take(1)
                    .last()
                    .map(|battery| battery.time_to_empty);

                log::trace!("Charge: {}", charge);

                let mut controls = me.controls.lock().await;

                for (index, control) in controls.clone().iter().enumerate().rev() {
                    match control.upgrade() {
                        Some(control) => {
                            control.imp().update_labels(
                                &format!("   {}%", charge as i64),
                                &format!("Time to charge: {}", time_to_empty.unwrap_or(0),),
                            );
                        }
                        None => {
                            controls.remove(index);
                        }
                    }
                }
                sleep(Duration::from_secs(10)).await;
            }
        });

        instance
    }

    fn refresh_batteries(&self) {
        let mut batteries = Vec::new();
        let mut mains = Vec::new();
        for dir in Path::new("/sys/class/power_supply")
            .read_dir()
            .unwrap()
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().unwrap().is_dir())
        {
            let type_path = dir.path().join("type");
            if !type_path.exists() {
                continue;
            }
            let type_contents = fs::read_to_string(&type_path).unwrap();
            if type_contents == "Battery" {
                batteries.push(dir);
            } else if type_contents == "Mains" {
                mains.push(dir);
            }
        }
    }

    pub async fn register_control(&self, control: SendWeakRef<BatteryInfo>) {
        self.controls.lock().await.push(control);
    }
}

// Object holding the state
#[derive(Default)]
pub struct BatteryInfoImpl {
    label_ref: OnceCell<WeakRef<Label>>,
    popup_label_ref: OnceCell<WeakRef<Label>>,
}

impl BatteryInfoImpl {
    pub fn update_labels(&self, text: &str, popup_text: &str) {
        match self.label_ref.get().and_then(|l| l.upgrade()) {
            Some(label) => label.set_text(text),
            None => {
                log::trace!("Label ref upgrade failed");
                return;
            }
        };

        match self.popup_label_ref.get().and_then(|l| l.upgrade()) {
            Some(label) => label.set_text(popup_text),
            None => {
                log::trace!("Popup label upgrade failed");
                return;
            }
        };
    }
}

// The central trait for subclassing a GObject
#[glib::object_subclass]
impl ObjectSubclass for BatteryInfoImpl {
    const NAME: &'static str = "TwBarBatteryInfo";
    type Type = BatteryInfo;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
impl ObjectImpl for BatteryInfoImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("battery_monitor");
        let label = Label::new(Some(""));
        self.obj().append(&label);

        self.label_ref.set(Downgrade::downgrade(&label)).unwrap();

        let popup_label = Label::new(Some(""));
        let popup = Popover::new();
        popup.set_child(Some(&popup_label));
        popup.set_parent(self.obj().upcast_ref::<Widget>());
        popup.set_autohide(false);
        popup.set_focusable(false);
        popup.set_can_focus(false);

        self.popup_label_ref
            .set(Downgrade::downgrade(&popup_label))
            .unwrap();

        let weak_me: SendWeakRef<BatteryInfo> = self.obj().downgrade().into();
        task::spawn(async move {
            BatteryListener::instance()
                .await
                .register_control(weak_me)
                .await;
        });

        let event_controller = EventControllerMotion::new();
        event_controller.connect_enter(clone!(
            #[weak]
            popup,
            move |_ec, _, _| {
                popup.popup();
            }
        ));
        event_controller.connect_leave(clone!(
            #[weak]
            popup,
            move |_| {
                popup.popdown();
            }
        ));
        self.obj().add_controller(event_controller);
        // Unparent to avoid the warning about a destroyed widget having children.
        self.obj().connect_destroy(move |_| {
            log::trace!("In destroy");
            popup.unparent();
        });
    }
}

// Trait shared by all widgets
impl WidgetImpl for BatteryInfoImpl {}

// Trait shared by all boxes
impl BoxImpl for BatteryInfoImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct BatteryInfo(ObjectSubclass<BatteryInfoImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl BatteryInfo {
    pub fn new() -> Self {
        Object::builder().build()
    }
}

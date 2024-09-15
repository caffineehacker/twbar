use std::cell::OnceCell;
use std::sync::Arc;
use std::time::Duration;
use std::{io::Error, str::FromStr};

use async_std::sync::{Mutex, Weak};
use async_std::task::{self, sleep};
use async_std::{fs::File, io::ReadExt};

use gio::glib::{clone, random_int, SendWeakRef, WeakRef};
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{
    glib, Accessible, Buildable, ConstraintTarget, EventControllerMotion, Orientable, Popover,
    Widget,
};
use gtk4::{prelude::*, Label};

#[allow(dead_code)]
struct CpuStat {
    name: String,
    user: i64,
    nice: i64,
    system: i64,
    idle: i64,
    iowait: i64,
    irq: i64,
    softirq: i64,
    steal: i64,
    guest: i64,
    guest_nice: i64,
}

impl CpuStat {
    fn from_proc_stat_line(line: &str) -> Result<Self, <i64 as FromStr>::Err> {
        let parts = line.split_ascii_whitespace().collect::<Vec<&str>>();
        if parts.len() != 11 {
            log::error!("Expected 11 parts, got {:?}", parts);
        }

        Ok(Self {
            name: parts[0].to_owned(),
            user: parts[1].parse::<i64>()?,
            nice: parts[2].parse::<i64>()?,
            system: parts[3].parse::<i64>()?,
            idle: parts[4].parse::<i64>()?,
            iowait: parts[5].parse::<i64>()?,
            irq: parts[6].parse::<i64>()?,
            softirq: parts[7].parse::<i64>()?,
            steal: parts[8].parse::<i64>()?,
            guest: parts[9].parse::<i64>()?,
            guest_nice: parts[10].parse::<i64>()?,
        })
    }

    pub fn total_idle_time(&self) -> i64 {
        self.idle + self.iowait
    }

    fn total_system_time(&self) -> i64 {
        self.system + self.irq + self.softirq
    }

    #[allow(dead_code)]
    fn virtual_time(&self) -> i64 {
        self.guest + self.guest_nice
    }

    pub fn total_time(&self) -> i64 {
        // We don't include virtual time since guest is included in user and guest_nice is included in nice
        self.user + self.nice + self.total_system_time() + self.total_idle_time() + self.steal
    }
}

#[allow(dead_code)]
struct CpuStatDiff {
    total: i64,
    idle: i64,
    percent_usage: i64,
}

struct CpuStatMonitor {
    controls: Mutex<Vec<SendWeakRef<CpuUsage>>>,
}

impl CpuStatMonitor {
    pub async fn instance() -> Arc<Self> {
        static INSTANCE: Mutex<Weak<CpuStatMonitor>> = Mutex::new(Weak::new());

        let mut mutex_guard = INSTANCE.lock().await;
        match mutex_guard.upgrade() {
            Some(instance) => instance,
            None => {
                let instance = Self::new();
                *mutex_guard = Arc::downgrade(&instance);
                instance
            }
        }
    }

    fn new() -> Arc<Self> {
        let instance = Arc::new(Self {
            controls: Mutex::new(Vec::new()),
        });

        let me = instance.clone();
        glib::spawn_future_local(async move {
            let mut prev_cpu_info: Vec<CpuStat> = Vec::new();
            loop {
                let cpu_info = Self::read_cpu_info().await;
                match cpu_info {
                    Err(e) => {
                        log::error!("Failed to read cpu info: {}", e);
                    }
                    Ok(cpu_info) => {
                        if prev_cpu_info.len() == cpu_info.len() && cpu_info.len() > 0 {
                            let diffs: Vec<CpuStatDiff> = prev_cpu_info
                                .iter()
                                .zip(cpu_info.iter())
                                .map(|(prev, current)| {
                                    let total = current.total_time() - prev.total_time();
                                    let idle = current.total_idle_time() - prev.total_idle_time();
                                    CpuStatDiff {
                                        total,
                                        idle,
                                        percent_usage: ((total - idle) * 100) / total.max(1),
                                    }
                                })
                                .collect();

                            let mut tooltip_text = "".to_owned();
                            let label_text = if diffs.len() > 0 {
                                format!("   {}%", diffs[0].percent_usage)
                            } else {
                                "No diffs".to_owned()
                            };
                            for i in 0..diffs.len() {
                                if i == 0 {
                                    tooltip_text
                                        .push_str(&format!("Total: {}%", diffs[i].percent_usage));
                                } else {
                                    tooltip_text.push_str(&format!(
                                        "\nCore {}: {}%",
                                        i, diffs[i].percent_usage
                                    ));
                                }
                            }

                            let mut controls = me.controls.lock().await;
                            for (index, control) in controls.clone().iter().enumerate().rev() {
                                match control.upgrade() {
                                    Some(control) => {
                                        control.imp().update(&label_text, &tooltip_text);
                                    }
                                    None => {
                                        controls.remove(index);
                                    }
                                }
                            }
                        }

                        prev_cpu_info = cpu_info;
                    }
                };
                sleep(Duration::from_secs(1)).await;
            }
        });

        instance
    }

    pub async fn register_control(&self, control: SendWeakRef<CpuUsage>) {
        self.controls.lock().await.push(control);
    }

    async fn read_cpu_info() -> Result<Vec<CpuStat>, Error> {
        let mut stat = File::open("/proc/stat").await?;
        let mut buf: String = String::default();
        stat.read_to_string(&mut buf).await?;
        Ok(buf
            .lines()
            .take_while(|line| line.starts_with("cpu"))
            .map(|line| CpuStat::from_proc_stat_line(line).unwrap())
            .collect::<Vec<CpuStat>>())
    }
}

// Object holding the state
#[derive(Default)]
pub struct CpuUsageImpl {
    label_ref: OnceCell<WeakRef<Label>>,
    popup_label_ref: OnceCell<WeakRef<Label>>,
}

impl CpuUsageImpl {
    fn update(&self, text: &str, popup_text: &str) {
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
impl ObjectSubclass for CpuUsageImpl {
    const NAME: &'static str = "TwBarCpuUsage";
    type Type = CpuUsage;
    type ParentType = gtk4::Box;
}

// Trait shared by all GObjects
impl ObjectImpl for CpuUsageImpl {
    fn constructed(&self) {
        self.parent_constructed();

        self.obj().add_css_class("cpu_usage");
        let label = Label::new(Some(""));
        self.obj().append(&label);

        let label_ref = label.downgrade();

        let popup_label = Label::new(Some(""));
        let popup = Popover::new();
        popup.set_child(Some(&popup_label));
        popup.set_parent(self.obj().upcast_ref::<Widget>());
        popup.set_autohide(false);
        popup.set_focusable(false);
        popup.set_can_focus(false);

        let random_id = random_int();

        let popup_label_ref = popup_label.downgrade();

        self.label_ref.set(label_ref).unwrap();
        self.popup_label_ref.set(popup_label_ref).unwrap();

        let weak_me = SendWeakRef::from(self.obj().downgrade());
        task::block_on(async move {
            CpuStatMonitor::instance()
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
            log::trace!("CPU Usage destroy: {}", random_id);
            popup.unparent();
        });
    }
}

// Trait shared by all widgets
impl WidgetImpl for CpuUsageImpl {}

// Trait shared by all boxes
impl BoxImpl for CpuUsageImpl {}

// Self encapsulated button that triggers the appropriate workspace on click
glib::wrapper! {
    pub struct CpuUsage(ObjectSubclass<CpuUsageImpl>)
        @extends gtk4::Box, Widget,
        @implements Accessible, Buildable, ConstraintTarget, Orientable;
}

impl CpuUsage {
    pub fn new() -> Self {
        Object::builder().build()
    }
}

use std::time::Duration;
use std::{io::Error, str::FromStr};

use async_std::task::sleep;
use async_std::{fs::File, io::ReadExt};

use gio::glib::clone;
use gio::prelude::*;
use gtk4::glib::Object;
use gtk4::subclass::prelude::*;
use gtk4::{glib, Accessible, Buildable, ConstraintTarget, Orientable, Widget};
use gtk4::{prelude::*, Label};

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

    fn virtual_time(&self) -> i64 {
        self.guest + self.guest_nice
    }

    pub fn total_time(&self) -> i64 {
        // We don't include virtual time since guest is included in user and guest_nice is included in nice
        self.user + self.nice + self.total_system_time() + self.total_idle_time() + self.steal
    }
}

struct CoreTiming {
    idle_time: i64,
    total_time: i64,
}

struct CpuUsageStats {}

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

// Object holding the state
#[derive(Default)]
pub struct CpuUsageImpl {}

impl CpuUsageImpl {}

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

        glib::spawn_future_local(async move {
            let mut prev_cpu_info = Vec::new();
            loop {
                let cpu_info = read_cpu_info().await;
                match cpu_info {
                    Err(e) => {
                        log::error!("Failed to read cpu info: {}", e);
                    }
                    Ok(cpu_info) => {
                        if prev_cpu_info.len() != cpu_info.len() {
                            prev_cpu_info = cpu_info;
                        } else if cpu_info.len() > 0 {
                            let mut tooltip_text = String::new();
                            for i in 0..cpu_info.len() {
                                // Let's start with just the CPU total before supporting per core
                                let total_diff =
                                    cpu_info[i].total_time() - prev_cpu_info[i].total_time();
                                let idle_diff = cpu_info[i].total_idle_time()
                                    - prev_cpu_info[i].total_idle_time();
                                let usage_percentage =
                                    ((total_diff - idle_diff) * 100) / total_diff.max(1);

                                if i == 0 {
                                    label.set_text(&format!("   {}%", usage_percentage));
                                    tooltip_text.push_str(&format!("Total: {}%", usage_percentage));
                                } else {
                                    tooltip_text
                                        .push_str(&format!("\nCore {}: {}%", i, usage_percentage));
                                }
                            }
                            label.set_tooltip_text(Some(&tooltip_text));
                        }
                    }
                };
                sleep(Duration::from_secs(1)).await;
            }
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
use log::Level;

use crate::{arch::cpu::cpuid, print::FGColor, printk_with_color, sync::spin::Once};

struct KernelLogger {
    targets: heapless::Vec<&'static str, 40>,
}

pub fn log_init() {
    static LOGGER: Once<KernelLogger> = Once::new();

    let logger = LOGGER.get_or_init(|| {
        let mut targets = heapless::Vec::new();
        targets.extend_from_slice(&["inode"]).unwrap();
        KernelLogger { targets }
    });

    log::set_logger(logger).unwrap();
    log::set_max_level(match option_env!("LOG") {
        Some("error") => log::LevelFilter::Error,
        Some("warn") => log::LevelFilter::Warn,
        Some("info") => log::LevelFilter::Info,
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        None | Some(_) => log::LevelFilter::Off,
    });
}

impl log::Log for KernelLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        !self.targets.contains(&metadata.target())
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            printk_with_color!(
                level_color(record.level()),
                "{:>5}[{},-] {}\n",
                record.level(),
                cpuid(),
                record.args()
            );
        }
    }

    fn flush(&self) {}
}

fn level_color(level: Level) -> u8 {
    (match level {
        Level::Error => FGColor::Red,
        Level::Warn => FGColor::White,
        Level::Info => FGColor::Green,
        Level::Debug => FGColor::Yellow,
        Level::Trace => FGColor::BrightBlack,
    }) as u8
}

struct Diagnostics;
impl log::Log for Diagnostics {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Error
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            use std::io::Write;
            let message = format!("{}: {}\n", r.target(), r.args());
            eprint!("{message}");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::env::temp_dir().join("xgpui-color-lab-errors.log"))
            {
                let _ = f.write_all(message.as_bytes());
            }
        }
    }
    fn flush(&self) {}
}
static DIAGNOSTICS: Diagnostics = Diagnostics;
pub(super) fn init() {
    let _ = log::set_logger(&DIAGNOSTICS);
    log::set_max_level(log::LevelFilter::Error);
}

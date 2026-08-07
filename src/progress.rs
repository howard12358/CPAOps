use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};

pub trait ProgressReporter: Send + Sync {
    fn stage(&self, message: &str);
    fn begin_download(&self, file_name: &str, total_bytes: Option<u64>);
    fn advance(&self, bytes: u64);
    fn finish_download(&self);
    fn clear(&self);
}

pub struct NoProgress;

impl ProgressReporter for NoProgress {
    fn stage(&self, _: &str) {}
    fn begin_download(&self, _: &str, _: Option<u64>) {}
    fn advance(&self, _: u64) {}
    fn finish_download(&self) {}
    fn clear(&self) {}
}

pub struct TerminalProgress {
    bar: ProgressBar,
    active: Mutex<bool>,
}

impl TerminalProgress {
    pub fn new() -> Self {
        Self {
            bar: ProgressBar::new_spinner(),
            active: Mutex::new(false),
        }
    }
}

impl Default for TerminalProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressReporter for TerminalProgress {
    fn stage(&self, message: &str) {
        self.bar.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}").expect("固定进度样式必须有效"),
        );
        self.bar.set_message(message.to_owned());
        self.bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        *self.active.lock().expect("进度状态锁不可中毒") = true;
    }

    fn begin_download(&self, file_name: &str, total_bytes: Option<u64>) {
        self.bar.set_style(
            ProgressStyle::with_template(
                "{spinner:.cyan} {msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
            )
            .expect("固定进度样式必须有效")
            .progress_chars("=> "),
        );
        self.bar.reset();
        self.bar.set_length(total_bytes.unwrap_or(0));
        self.bar.set_message(file_name.to_owned());
        self.bar
            .enable_steady_tick(std::time::Duration::from_millis(100));
        *self.active.lock().expect("进度状态锁不可中毒") = true;
    }

    fn advance(&self, bytes: u64) {
        self.bar.inc(bytes);
    }

    fn finish_download(&self) {
        self.bar.finish_and_clear();
        *self.active.lock().expect("进度状态锁不可中毒") = false;
    }

    fn clear(&self) {
        if *self.active.lock().expect("进度状态锁不可中毒") {
            self.bar.finish_and_clear();
        }
    }
}

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use reedline::ExternalPrinter;

/// Route log lines around the reedline editor's prompt.
#[derive(Clone)]
pub struct LogWriter {
    shared: Arc<Shared>,
}

struct Shared {
    prompt_active: AtomicBool,
    sender: SyncSender<String>,
    // constructed as a part of the LogWriter, but taken by the reedline editor
    printer: Mutex<Option<ExternalPrinter<String>>>,
    dropped: AtomicUsize,
}

impl Default for LogWriter {
    fn default() -> Self {
        let printer = ExternalPrinter::default();
        LogWriter {
            shared: Arc::new(Shared {
                prompt_active: AtomicBool::new(false),
                sender: printer.sender(),
                printer: Mutex::new(Some(printer)),
                dropped: AtomicUsize::new(0),
            }),
        }
    }
}

impl LogWriter {
    pub(crate) fn take_printer(&self) -> ExternalPrinter<String> {
        self.shared
            .printer
            .lock()
            .expect("printer lock poisoned")
            .take()
            .expect("the printer is attached to one editor")
    }

    pub(crate) fn set_prompt_active(&self, active: bool) {
        self.shared.prompt_active.store(active, Ordering::Release);
    }

    fn send(&self, line: String) -> Result<(), TrySendError<String>> {
        self.shared.sender.try_send(line)
    }
}

impl Write for LogWriter {
    /// Take a log message from tracing, and hand it off to the reedline ExternalPrinter to write
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // if the prompt is not active (a blocking task is running), just write to stderr
        if !self.shared.prompt_active.load(Ordering::Acquire) {
            return io::stderr().write(buf);
        }
        let dropped = self.shared.dropped.swap(0, Ordering::AcqRel);
        if dropped > 0 && self.send(format!("{dropped} log lines dropped")).is_err() {
            self.shared.dropped.fetch_add(dropped, Ordering::AcqRel);
        }
        let line = String::from_utf8_lossy(buf).trim_end().to_string();
        if self.send(line).is_err() {
            self.shared.dropped.fetch_add(1, Ordering::AcqRel);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

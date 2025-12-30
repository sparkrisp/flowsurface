use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    process,
    sync::mpsc,
    thread,
};

pub use data::log::Error;

const MAX_LOG_FILE_SIZE: u64 = 50 * 1024 * 1024; // 50 MB

/// Wrap a writer and treat BrokenPipe as a no-op (common when stdout/stderr is piped and the reader exits).
struct IgnoreBrokenPipe<W>(W);

impl<W: Write> Write for IgnoreBrokenPipe<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.0.write(buf) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(buf.len()),
            Err(e) => Err(e),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.0.flush() {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(e) => Err(e),
        }
    }
}

enum LogMessage {
    Content(Vec<u8>),
    Flush,
    Shutdown,
}

pub fn setup(is_debug: bool) -> Result<(), Error> {
    let default_level = if is_debug {
        log::Level::Debug
    } else {
        log::Level::Info
    };

    let level_filter = std::env::var("RUST_LOG")
        .ok()
        .as_deref()
        .map(str::parse::<log::Level>)
        .transpose()?
        .unwrap_or(default_level)
        .to_level_filter();

    let mut io_sink = fern::Dispatch::new().format(|out, message, record| {
        out.finish(format_args!(
            "{}:{} -- {}",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            record.level(),
            message
        ));
    });

    if is_debug {
        let stdout: Box<dyn Write + Send> = Box::new(IgnoreBrokenPipe(std::io::stdout()));
        io_sink = io_sink.chain(stdout);
    } else {
        let log_path = data::log::path()?;
        initial_rotation(&log_path)?;

        match BackgroundLogger::new(log_path) {
            Ok(logger) => {
                let logger: Box<dyn Write + Send> = Box::new(logger);
                io_sink = io_sink.chain(logger);
            }
            Err(e) => {
                // Fail-open: if we can't write to a log file, don't spam BrokenPipe errors.
                // Fall back to stdout so the app remains usable.
                eprintln!("Failed to initialize file logger, falling back to stdout: {e}");
                let stdout: Box<dyn Write + Send> = Box::new(IgnoreBrokenPipe(std::io::stdout()));
                io_sink = io_sink.chain(stdout);
            }
        }
    }

    fern::Dispatch::new()
        .level(log::LevelFilter::Off)
        .level_for("panic", log::LevelFilter::Error)
        .level_for("iced_wgpu", log::LevelFilter::Info)
        .level_for("data", level_filter)
        .level_for("exchange", level_filter)
        .level_for("flowsurface", level_filter)
        .chain(io_sink)
        .apply()?;

    Ok(())
}

fn initial_rotation(log_path: &PathBuf) -> io::Result<()> {
    let path = PathBuf::from(".");

    let dir = log_path.parent().unwrap_or(&path);

    let previous_log_path = dir.join("flowsurface-previous.log");

    if previous_log_path.exists() {
        fs::remove_file(&previous_log_path)?;
    }

    if log_path.exists() {
        fs::rename(log_path, &previous_log_path)?;
    }

    Ok(())
}

struct BackgroundLogger {
    sender: mpsc::Sender<LogMessage>,
    _thread_handle: thread::JoinHandle<()>,
}

impl BackgroundLogger {
    fn new(path: PathBuf) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();

        // Try opening the log file on the caller thread so we can surface errors here (and fall back
        // to stdout). Also avoids any stderr/stdout writes from the logger thread itself.
        let mut logger = Logger::new(&path)?;

        let thread_handle = thread::Builder::new()
            .name("logger-thread".to_string())
            .spawn(move || {
                loop {
                    match receiver.recv() {
                        Ok(LogMessage::Content(data)) => {
                            // Never print I/O errors to stdout/stderr (can be BrokenPipe depending on
                            // how the app is launched). Just drop logs on failure.
                            let _ = logger.write_all(&data);
                        }
                        Ok(LogMessage::Flush) => {
                            let _ = logger.flush();
                        }
                        Ok(LogMessage::Shutdown) | Err(_) => break,
                    }
                }
            })?;

        Ok(BackgroundLogger {
            sender,
            _thread_handle: thread_handle,
        })
    }
}

impl Write for BackgroundLogger {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = buf.len();
        // If the logger thread is gone, drop logs silently (fail-open) to avoid spamming errors
        // and impacting UI performance.
        let _ = self.sender.send(LogMessage::Content(buf.to_vec()));
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = self.sender.send(LogMessage::Flush);
        Ok(())
    }
}

impl Drop for BackgroundLogger {
    fn drop(&mut self) {
        let _ = self.sender.send(LogMessage::Shutdown);
    }
}

struct Logger {
    file: fs::File,
    current_size: u64,
}

impl Logger {
    fn new(path: &PathBuf) -> io::Result<Self> {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        let size = file.metadata()?.len();

        Ok(Logger {
            file,
            current_size: size,
        })
    }
}

impl Write for Logger {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let buf_len = buf.len() as u64;

        if self.current_size + buf_len > MAX_LOG_FILE_SIZE {
            let timestamp = chrono::Local::now().format("%H:%M:%S%.3f");
            let error_msg = format!(
                "\n{}:FATAL -- Log file size would exceed the maximum allowed size of {} bytes\n",
                timestamp, MAX_LOG_FILE_SIZE
            );

            eprintln!("{error_msg}");

            let _ = self.file.write_all(error_msg.as_bytes());
            let _ = self.file.flush();

            process::abort();
        }

        let bytes = self.file.write(buf)?;
        self.current_size += bytes as u64;

        Ok(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

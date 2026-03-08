mod ai;
mod app;
mod db;

use std::os::unix::net::{UnixListener, UnixStream};

fn socket_path() -> std::path::PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("notepad.sock")
}

fn main() {
    let sock = socket_path();

    // If already running, signal toggle and exit
    if let Ok(mut stream) = UnixStream::connect(&sock) {
        use std::io::Write;
        let _ = stream.write_all(b"toggle");
        return;
    }

    // First instance
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).expect("Failed to bind socket");
    listener.set_nonblocking(true).ok();

    let result = app::run(listener);

    let _ = std::fs::remove_file(&sock);
    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }
}

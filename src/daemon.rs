use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub fn get_pid_file_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(runtime_dir);
        if dir.is_dir() {
            return dir.join("keysound.pid");
        }
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/keysound-{}.pid", uid))
}

pub fn get_log_file_path() -> PathBuf {
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        let dir = PathBuf::from(runtime_dir);
        if dir.is_dir() {
            return dir.join("keysound.log");
        }
    }
    let uid = unsafe { libc::getuid() };
    PathBuf::from(format!("/tmp/keysound-{}.log", uid))
}

pub fn log_error(msg: &str) {
    let log_path = get_log_file_path();
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(file, "{}", msg);
    }
}

pub fn is_process_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

pub fn read_pid_file() -> Option<u32> {
    let path = get_pid_file_path();
    if !path.exists() {
        return None;
    }
    let content = fs::read_to_string(&path).ok()?;
    content.trim().parse::<u32>().ok()
}

pub fn write_pid_file(pid: u32) -> Result<(), String> {
    let path = get_pid_file_path();
    fs::write(&path, format!("{}\n", pid))
        .map_err(|e| format!("Failed to write PID file '{}': {}", path.display(), e))
}

pub fn remove_pid_file() {
    let path = get_pid_file_path();
    let _ = fs::remove_file(path);
}

pub fn check_running() -> Option<u32> {
    if let Some(pid) = read_pid_file() {
        if is_process_alive(pid) {
            return Some(pid);
        } else {
            remove_pid_file();
        }
    }
    None
}

pub fn spawn_daemon(file: &Path, volume: f32, max_ms: Option<u64>, overlap: bool) -> Result<u32, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Failed to determine current executable path: {e}"))?;

    let mut cmd = Command::new(exe);
    cmd.arg("--daemon")
        .arg(file.as_os_str())
        .arg("--volume")
        .arg(volume.to_string());

    if let Some(ms) = max_ms {
        cmd.arg("--max-ms").arg(ms.to_string());
    }

    if overlap {
        cmd.arg("--overlap");
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    cmd.process_group(0);

    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn daemon process: {e}"))?;
    let pid = child.id();

    write_pid_file(pid)?;
    Ok(pid)
}

pub fn stop_daemon() -> Result<(), String> {
    let pid_opt = read_pid_file();
    match pid_opt {
        None => {
            println!("keysound is not running");
            Ok(())
        }
        Some(pid) => {
            if !is_process_alive(pid) {
                remove_pid_file();
                println!("keysound is not running (cleaned up stale pid file)");
                return Ok(());
            }

            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }

            // Wait up to 1.5s for process to shut down
            for _ in 0..30 {
                if !is_process_alive(pid) {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }

            // If still alive, force kill
            if is_process_alive(pid) {
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGKILL);
                }
                thread::sleep(Duration::from_millis(50));
            }

            remove_pid_file();
            println!("keysound stopped");
            Ok(())
        }
    }
}

pub fn status_daemon() {
    if let Some(pid) = check_running() {
        println!("keysound is running (pid {})", pid);
    } else {
        println!("keysound is not running");
    }
}

pub fn run_daemon(sound_path: PathBuf, volume: f32, max_ms: Option<u64>, overlap: bool) {
    std::panic::set_hook(Box::new(|info| {
        log_error(&format!("Daemon panic: {}", info));
    }));

    let sound = match crate::audio::load_sound(&sound_path, volume, max_ms) {
        Ok(s) => s,
        Err(e) => {
            log_error(&format!("Audio load error: {}", e));
            remove_pid_file();
            std::process::exit(1);
        }
    };

    let audio_manager = match crate::audio::AudioManager::new(sound, overlap) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            log_error(&format!("Audio manager error: {}", e));
            remove_pid_file();
            std::process::exit(1);
        }
    };

    if let Err(e) = crate::keys::run_listeners(audio_manager) {
        log_error(&format!("Listener error: {}", e));
    }

    remove_pid_file();
}

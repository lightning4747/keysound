use evdev::{Device, EventType, KeyCode};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::audio::AudioManager;

pub fn validate_keyboards() -> Result<usize, String> {
    let dev_input = Path::new("/dev/input");
    if !dev_input.exists() {
        return Err("Directory /dev/input does not exist.".to_string());
    }

    let entries = match fs::read_dir(dev_input) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(
                "Permission denied accessing /dev/input.\n\
                 Ensure your user is in the 'input' group: sudo usermod -aG input $USER (then re-login)."
                    .to_string(),
            );
        }
        Err(err) => return Err(format!("Failed to read /dev/input: {err}")),
    };

    let mut event_nodes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("event") {
                event_nodes.push(path);
            }
        }
    }

    if event_nodes.is_empty() {
        return Err("No /dev/input/event* devices found.".to_string());
    }

    // Check permissions by attempting to open the first event node
    if let Some(first_node) = event_nodes.first() {
        if let Err(err) = fs::File::open(first_node) {
            if err.kind() == std::io::ErrorKind::PermissionDenied {
                return Err(
                    "Permission denied opening /dev/input/event* devices.\n\
                     Ensure your user is in the 'input' group: sudo usermod -aG input $USER (then re-login)."
                        .to_string(),
                );
            }
        }
    }

    let mut keyboard_count = 0;
    for (_path, device) in evdev::enumerate() {
        if device
            .supported_keys()
            .map_or(false, |keys| keys.contains(KeyCode::KEY_A))
        {
            keyboard_count += 1;
        }
    }

    if keyboard_count == 0 {
        return Err("No keyboard devices supporting KEY_A were found in /dev/input.".to_string());
    }

    Ok(keyboard_count)
}

pub fn get_keyboards() -> Result<Vec<(PathBuf, Device)>, String> {
    validate_keyboards()?;

    let mut keyboards = Vec::new();
    for (path, device) in evdev::enumerate() {
        if device
            .supported_keys()
            .map_or(false, |keys| keys.contains(KeyCode::KEY_A))
        {
            keyboards.push((path, device));
        }
    }

    if keyboards.is_empty() {
        return Err("No keyboards found supporting KEY_A".to_string());
    }

    Ok(keyboards)
}

pub fn listen_device(path: PathBuf, mut device: Device, audio: Arc<AudioManager>) {
    let name = device.name().unwrap_or("Unknown keyboard").to_string();
    loop {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() == EventType::KEY && ev.value() == 1 {
                        audio.play();
                    }
                }
            }
            Err(e) => {
                crate::daemon::log_error(&format!(
                    "Device error on '{}' ({}): {}",
                    name,
                    path.display(),
                    e
                ));
                break;
            }
        }
    }
}

pub fn run_listeners(audio: Arc<AudioManager>) -> Result<(), String> {
    let keyboards = get_keyboards()?;

    let mut handles = Vec::new();
    for (path, device) in keyboards {
        let audio_clone = Arc::clone(&audio);
        let handle = std::thread::spawn(move || {
            listen_device(path, device, audio_clone);
        });
        handles.push(handle);
    }

    for handle in handles {
        let _ = handle.join();
    }

    crate::daemon::log_error("All keyboard listener threads ended; daemon shutting down.");
    Ok(())
}

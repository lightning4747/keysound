mod audio;
mod daemon;
mod keys;

use std::env;
use std::path::PathBuf;

struct AudioConfig {
    file: PathBuf,
    volume: f32,
    max_ms: Option<u64>,
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  keysound run <file> [--volume 0.8] [--max-ms 400]     run in foreground (Ctrl+C to stop)");
    eprintln!("  keysound start <file> [--volume 0.8] [--max-ms 400]   start in background");
    eprintln!("  keysound stop                                         stop the daemon");
    eprintln!("  keysound status                                       print running/not running");
    eprintln!("  keysound restart <file> [--volume 0.8] [--max-ms 400] stop, then start with a new sound");
}

fn parse_audio_args(args: &[String]) -> Result<AudioConfig, String> {
    let mut file: Option<PathBuf> = None;
    let mut volume: f32 = 1.0;
    let mut max_ms: Option<u64> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--volume" {
            i += 1;
            if i >= args.len() {
                return Err("Missing value for --volume".to_string());
            }
            volume = args[i]
                .parse::<f32>()
                .map_err(|_| format!("Invalid volume value: '{}'", args[i]))?;
            if volume < 0.0 {
                return Err("Volume must be non-negative".to_string());
            }
        } else if let Some(val) = arg.strip_prefix("--volume=") {
            volume = val
                .parse::<f32>()
                .map_err(|_| format!("Invalid volume value: '{}'", val))?;
            if volume < 0.0 {
                return Err("Volume must be non-negative".to_string());
            }
        } else if arg == "--max-ms" {
            i += 1;
            if i >= args.len() {
                return Err("Missing value for --max-ms".to_string());
            }
            let val = args[i]
                .parse::<u64>()
                .map_err(|_| format!("Invalid max-ms value: '{}'", args[i]))?;
            if val == 0 {
                return Err("max-ms must be greater than 0".to_string());
            }
            max_ms = Some(val);
        } else if let Some(val_str) = arg.strip_prefix("--max-ms=") {
            let val = val_str
                .parse::<u64>()
                .map_err(|_| format!("Invalid max-ms value: '{}'", val_str))?;
            if val == 0 {
                return Err("max-ms must be greater than 0".to_string());
            }
            max_ms = Some(val);
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: '{}'", arg));
        } else if file.is_none() {
            file = Some(PathBuf::from(arg));
        } else {
            return Err(format!("Unexpected argument: '{}'", arg));
        }
        i += 1;
    }

    let file = file.ok_or_else(|| "Missing sound file argument".to_string())?;

    Ok(AudioConfig {
        file,
        volume,
        max_ms,
    })
}

fn handle_start(args: &[String]) {
    if let Some(pid) = daemon::check_running() {
        eprintln!("Error: keysound is already running (pid {}).", pid);
        std::process::exit(1);
    }

    let config = match parse_audio_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            print_usage();
            std::process::exit(1);
        }
    };

    let abs_file = match std::fs::canonicalize(&config.file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: Cannot access sound file '{}': {}", config.file.display(), e);
            std::process::exit(1);
        }
    };

    // Validate sound file decoding in foreground before daemonizing
    if let Err(e) = audio::validate_sound(&abs_file) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Validate keyboard access in foreground before daemonizing
    if let Err(e) = keys::validate_keyboards() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Foreground validation succeeded: spawn background daemon
    match daemon::spawn_daemon(&abs_file, config.volume, config.max_ms) {
        Ok(pid) => {
            println!("keysound started (pid {})", pid);
        }
        Err(e) => {
            eprintln!("Error starting keysound daemon: {}", e);
            std::process::exit(1);
        }
    }
}

fn handle_stop() {
    if let Err(e) = daemon::stop_daemon() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn handle_status() {
    daemon::status_daemon();
}

fn handle_restart(args: &[String]) {
    // If currently running, stop it first
    if daemon::check_running().is_some() {
        let _ = daemon::stop_daemon();
    }
    // Now start
    handle_start(args);
}

fn handle_run(args: &[String]) {
    let config = match parse_audio_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {}", e);
            print_usage();
            std::process::exit(1);
        }
    };

    let abs_file = match std::fs::canonicalize(&config.file) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: Cannot access sound file '{}': {}", config.file.display(), e);
            std::process::exit(1);
        }
    };

    if let Err(e) = keys::validate_keyboards() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    let sound = match audio::load_sound(&abs_file, config.volume, config.max_ms) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let audio_manager = match audio::AudioManager::new(sound) {
        Ok(m) => std::sync::Arc::new(m),
        Err(e) => {
            eprintln!("Audio error: {}", e);
            std::process::exit(1);
        }
    };

    println!("keysound is running in the foreground with '{}'.", abs_file.display());
    println!("Press Ctrl+C to stop.");

    if let Err(e) = keys::run_listeners(audio_manager) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn handle_daemon(args: &[String]) {
    let config = match parse_audio_args(args) {
        Ok(c) => c,
        Err(e) => {
            daemon::log_error(&format!("Daemon argument parse error: {}", e));
            daemon::remove_pid_file();
            std::process::exit(1);
        }
    };

    daemon::run_daemon(config.file, config.volume, config.max_ms);
}

fn main() {
    let raw_args: Vec<String> = env::args().collect();
    if raw_args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = &raw_args[1];
    let remaining = &raw_args[2..];

    match command.as_str() {
        "run" => handle_run(remaining),
        "start" => handle_start(remaining),
        "stop" => handle_stop(),
        "status" => handle_status(),
        "restart" => handle_restart(remaining),
        "--daemon" => handle_daemon(remaining),
        "-h" | "--help" | "help" => {
            print_usage();
            std::process::exit(0);
        }
        cmd => {
            eprintln!("Unknown command: '{}'\n", cmd);
            print_usage();
            std::process::exit(1);
        }
    }
}

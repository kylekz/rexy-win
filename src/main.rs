// A release build on Windows opens no console window of its own.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#[cfg(unix)]
use std::path::PathBuf;

use eframe::egui;

const HELP: &str = "\
rexy — a minimal Rex client

Usage: rexy [--server ENDPOINT]

  -S, --server  Unix socket, host name, ssh://, wsl://, ws:// or wss:// Rex endpoint
      --probe   Verify attachment and Ghostty decoding without opening a window
  -h, --help    Show this help
      --version Show version

Defaults to your Rex home, as REX_HOME or `rex home set` chose it, then the
local Rex socket. On Windows, home is the one Rex in WSL follows, and
wsl://DISTRO[/SOCKET] reaches a server in WSL. Closing the client leaves
your sessions running.";

/// Rex's home, where one has been chosen: the server this machine follows for its
/// hosts. `named` is what REX_HOME holds, which comes first, and `file` what `rex
/// home set` wrote to home.json. REX_SERVER is not asked: in a Rex terminal it
/// names that terminal's server, and Rex does not take it for home either.
fn home(named: Option<String>, file: Option<String>) -> Option<String> {
    let chosen = || {
        let home: serde_json::Value = serde_json::from_str(&file?).ok()?;
        Some(home["endpoint"].as_str()?.to_owned())
    };
    let set = |home: &String| !home.is_empty();
    named.filter(set).or_else(chosen).filter(set)
}

fn default_endpoint(probe: bool) -> String {
    // On Windows Rex keeps its home in WSL, where `rex hosts` is asked for it.
    #[cfg(unix)]
    let file = rexy::ui::state_home().map(|state| state.join("rex/home.json"));
    #[cfg(unix)]
    let file = file.and_then(|file| std::fs::read_to_string(file).ok());
    #[cfg(not(unix))]
    let file = None;
    let chosen = file.is_some();
    if let Some(home) = home(std::env::var("REX_HOME").ok(), file) {
        return home;
    }
    local_endpoint(chosen, probe)
}

#[cfg(unix)]
fn local_endpoint(chosen: bool, _probe: bool) -> String {
    if chosen {
        eprintln!("Rex's home.json names no home rexy can read; opening the local server.");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| {
        // SAFETY: geteuid has no arguments and cannot fail.
        PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() }))
    });
    format!("unix:{}", runtime.join("rex/server.sock").display())
}

/// Rex does not run on Windows. Home is the one Rex in WSL follows, and where it
/// cannot be found the window opens all the same, to say why and be told another.
#[cfg(windows)]
fn local_endpoint(_chosen: bool, probe: bool) -> String {
    let unfound = match rexy::wsl::find_home() {
        Ok((distro, home)) => {
            // Its Rex follows home, wherever that is, so `ssh` there reaches it.
            rexy::wsl::follow(&distro);
            return home;
        }
        Err(default) => default,
    };
    eprintln!(
        "No Rex home was found in WSL. Install Rex in a WSL distribution and start its \
         server, set REX_HOME, or pass --server."
    );
    if probe {
        std::process::exit(1);
    }
    format!("wsl://{}", unfound.unwrap_or_default())
}

/// What is printed goes to the console the program was run from, if it was. Not
/// for a window alone: closing that console would close the window with it.
#[cfg(windows)]
fn console(args: &[String]) {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    let window = match args {
        [] => true,
        [flag, _] => flag == "--server" || flag == "-S",
        _ => false,
    };
    if !window {
        // SAFETY: AttachConsole takes a process id, and fails where there is no
        // console to attach to, or this process has one already.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

#[cfg(not(windows))]
const RENDERER: eframe::Renderer = eframe::Renderer::Glow;
/// Through Direct3D: what OpenGL draws on Windows can stay off the screen.
#[cfg(windows)]
const RENDERER: eframe::Renderer = eframe::Renderer::Wgpu;

/// How the window is drawn through Direct3D. For Mica to show through it, what
/// is drawn goes to Windows' compositor as a picture of its own, which keeps
/// what is see-through in it: drawn for the window itself, none of it is. Only
/// Direct3D 12 draws so, and it is then the only one tried. Otherwise it is all
/// as eframe has it.
#[cfg(windows)]
fn wgpu(mica: bool) -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::{
        egui_wgpu::WgpuSetup,
        wgpu::{Backends, Dx12SwapchainKind},
    };
    let mut options = eframe::egui_wgpu::WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_setup
        && mica
    {
        setup.instance_descriptor.backends = Backends::DX12;
        let dx12 = &mut setup.instance_descriptor.backend_options.dx12;
        dx12.presentation_system = Dx12SwapchainKind::DxgiFromVisual;
    }
    options
}

fn main() -> eframe::Result {
    #[cfg(windows)]
    console(&std::env::args().skip(1).collect::<Vec<_>>());
    let mut endpoint = None;
    let mut probe = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" | "-S" => {
                endpoint = Some(args.next().unwrap_or_else(|| {
                    eprintln!("--server requires an endpoint");
                    std::process::exit(2);
                }));
            }
            "--probe" => probe = true,
            "--help" | "-h" => {
                println!("{HELP}");
                return Ok(());
            }
            "--version" => {
                println!("rexy {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {
                eprintln!("Unknown argument: {arg}. Try --help.");
                std::process::exit(2);
            }
        }
    }
    let endpoint = endpoint.unwrap_or_else(|| default_endpoint(probe));
    rexy::wsl::adopt(&endpoint);
    if probe {
        match rexy::backend::probe(endpoint, std::time::Duration::from_secs(12)) {
            Ok(summary) => println!("{summary}"),
            Err(error) => {
                eprintln!("{error:#}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }
    // Windows knows the window, and its toasts, by the name the process goes by.
    #[cfg(windows)]
    rexy::notify::claim();
    let remembered = rexy::ui::Remembered::load();
    // Windows' Mica shows through the window where it is asked for, and where
    // Windows can draw it.
    let mica = rexy::ui::mica(&remembered);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rexy")
            // The window draws its own frame, in the row its tabs and buttons have.
            // Windows still sizes it from its edges, gives it a shadow and rounds
            // its corners.
            .with_decorations(false)
            // Its corners are round, and what lies behind the window shows past
            // them. Windows rounds them itself, and Mica shows through it.
            .with_transparent(cfg!(not(windows)) || mica)
            .with_inner_size(remembered.kept.window)
            .with_min_inner_size(rexy::ui::LEAST_WINDOW)
            .with_app_id(rexy::notify::APP_ID)
            .with_icon(rexy::ui::icon()),
        renderer: RENDERER,
        // Ctrl+C, X and V belong to terminal programs; the UI maps the clipboard itself.
        clipboard_shortcuts: false,
        #[cfg(windows)]
        wgpu_options: wgpu(mica),
        ..Default::default()
    };
    eframe::run_native(
        "Rexy",
        options,
        Box::new(move |cc| Ok(Box::new(rexy::ui::App::new(cc, endpoint, remembered)))),
    )
}

#[cfg(test)]
#[path = "../tests/unit/home.rs"]
mod tests;

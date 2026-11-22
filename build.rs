//! Bundles whichever program icons are in `assets/icons`. They are not kept in
//! this repository, and the app builds without them: see the notice there.
use std::{env, fs, path::Path};

fn main() {
    println!("cargo::rerun-if-changed=assets/icons");
    let folder = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/icons");
    let files = fs::read_dir(&folder).into_iter().flatten().flatten();
    let files = files.map(|file| file.path());
    let mut icons: Vec<_> = files.filter(|file| file.extension() == Some("svg".as_ref())).collect();
    icons.sort();
    let row = |icon: &std::path::PathBuf| {
        let name = icon.file_stem().unwrap().to_str().unwrap();
        format!("({name:?}, include_str!({icon:?})),\n")
    };
    let rows: String = icons.iter().map(row).collect();
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("icons.rs");
    fs::write(out, format!("[{rows}]")).unwrap();
    // Rex's own icon, which is not kept here either: see assets/windows/NOTICE.md.
    println!("cargo::rerun-if-changed=assets/windows");
    let rex = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/windows/rex.png");
    let rex = if rex.exists() { format!("Some(include_bytes!({rex:?}))") } else { "None".into() };
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("rex_icon.rs"), rex).unwrap();
    #[cfg(windows)]
    resources();
}

/// On Windows the program carries its icon, and says what it is and its version.
#[cfg(windows)]
fn resources() {
    println!("cargo::rerun-if-changed=assets/rexy.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    // Made by `cargo run --example icon`, from Rex's icon or else Rexy's drawing.
    let icon = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets/rexy.ico");
    if icon.exists() {
        resource.set_icon(icon.to_str().unwrap());
    }
    resource.set("ProductName", "Rexy");
    // Task Manager names the process by this.
    resource.set("FileDescription", "Rexy");
    resource.set("OriginalFilename", "rexy.exe");
    resource.compile().expect("the icon and version are given to the program");
}

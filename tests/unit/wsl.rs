use super::*;

#[test]
fn the_bridge_survives_the_way_windows_passes_it_on() {
    assert!(!BRIDGE.contains('\\'), "wsl.exe would collapse a backslash");
    assert!(!BRIDGE.contains('"'), "Windows would escape a double quote");
    assert!(!HOSTS.contains('\\') && !HOSTS.contains('"'));
    assert!(!STATUS.contains('\\') && !STATUS.contains('"'));
    assert!(!PLACE.contains('\\') && !PLACE.contains('"'));
    // Neither starts a server that is not running.
    assert!(HOSTS.contains("--autostart=false") && STATUS.contains("--autostart=false"));
}

#[test]
fn a_wsl_endpoint_names_its_distribution() {
    assert_eq!(distro("wsl://Ubuntu"), Some("Ubuntu"));
    assert_eq!(distro("wsl://Ubuntu/run/user/1000/rex/server.sock"), Some("Ubuntu"));
    assert_eq!(distro("wsl://"), None);
    assert_eq!(distro("unix:///run/rex/server.sock"), None);
}

#[test]
fn home_in_wsl_is_reached_through_wsl_and_any_other_as_it_is() {
    let hosts = |endpoint: &str, this: bool| {
        let home = serde_json::json!({"label": "", "endpoint": endpoint, "this_machine": this});
        serde_json::json!({"home": home, "hosts": [], "warnings": []}).to_string()
    };
    let local = hosts("unix:///run/user/1000/rex/server.sock", true);
    let wsl = "wsl://Ubuntu/run/user/1000/rex/server.sock";
    assert_eq!(home_in("Ubuntu", &local).as_deref(), Some(wsl));
    let far = hosts("wss://studio.tail1234.ts.net", false);
    assert_eq!(home_in("Ubuntu", &far).as_deref(), Some("wss://studio.tail1234.ts.net"));
    assert_eq!(home_in("Ubuntu", &hosts("ssh://studio", false)).as_deref(), Some("ssh://studio"));
    assert_eq!(home_in("Ubuntu", &hosts("", true)), None);
    assert_eq!(home_in("Ubuntu", "sh: 1: rex: not found"), None);
    // The name comes first, as the distribution has it.
    let both = format!("rexy-name:Ubuntu\n{local}");
    assert_eq!(answered(&both), (Some("Ubuntu"), Some(wsl.into())));
    let none = "rexy-name:Debian\nsh: 1: rex: not found\n";
    assert_eq!(answered(none), (Some("Debian"), None));
    // WSL's own warnings may come first.
    let warned = "wsl: A localhost proxy configuration was detected but not mirrored into WSL.\r\n";
    let warned = format!("{warned}rexy-name:Ubuntu\n{local}");
    assert_eq!(answered(&warned), (Some("Ubuntu"), Some(wsl.into())));
    // `wsl.exe` prints why it could not ask, where the name would be.
    let none = "Windows Subsystem for Linux has no installed distributions.\r\n";
    assert_eq!(answered(none), (None, None));
    assert_eq!(answered(""), (None, None));
}

#[test]
fn a_distribution_says_its_name() {
    assert!(!NAME.contains('\\') && !NAME.contains('"'));
    assert!(NAME.contains(NAMED) && HOSTS.starts_with(NAME));
    assert_eq!(name_in("rexy-name:Ubuntu\n"), Some("Ubuntu"));
    assert_eq!(name_in("rexy-name:Ubuntu-24.04\r\n"), Some("Ubuntu-24.04"));
    let warned = "wsl: Failed to start the systemd user session for 'kyle'.\r\nrexy-name:Arch\n";
    assert_eq!(name_in(warned), Some("Arch"));
    // One with no name prints none, and `wsl.exe` with no distribution says why.
    assert_eq!(name_in("rexy-name:\n"), None);
    assert_eq!(name_in("\n"), None);
    assert_eq!(name_in("Ubuntu\n"), None);
    let none = "Windows Subsystem for Linux has no installed distributions.\r\n";
    assert_eq!(name_in(none), None);
}

#[test]
fn home_is_found_where_some_distribution_answers_and_with_it() {
    use std::{collections::BTreeMap, time::Duration};
    let local =
        r#"{"home": {"endpoint": "unix:///run/user/1000/rex/server.sock", "this_machine": true}}"#;
    let far = r#"{"home": {"endpoint": "ssh://studio", "this_machine": false}}"#;
    // What each run of `wsl.exe` prints, by the distribution it names, and how long
    // each was given.
    let search_with = |answers: &[(&str, String)]| {
        let answers: BTreeMap<&str, String> = answers.iter().cloned().collect();
        let mut given = Vec::new();
        let found = search(|args, patience| {
            let distro = match args {
                ["-l", ..] => "running",
                ["-d", distro, ..] => distro,
                _ => "default",
            };
            given.push((distro.to_owned(), patience));
            answers.get(distro).cloned().unwrap_or_default()
        });
        (found, given)
    };
    // The default's home, in WSL.
    let (found, given) = search_with(&[("default", format!("rexy-name:Ubuntu\n{local}"))]);
    let wsl = "wsl://Ubuntu/run/user/1000/rex/server.sock".to_owned();
    assert_eq!(found, Ok(("Ubuntu".into(), wsl)));
    assert_eq!(given, [("default".into(), Duration::from_secs(10))]);
    // Home elsewhere, as a distribution that is not the default has it: that is the
    // distribution `ssh` runs in.
    let (found, given) = search_with(&[
        ("default", "rexy-name:Ubuntu\nsh: 1: rex: not found\n".into()),
        ("running", "Ubuntu\r\nDebian\r\n".into()),
        ("Debian", format!("wsl: a warning\nrexy-name:Debian\n{far}")),
    ]);
    assert_eq!(found, Ok(("Debian".into(), "ssh://studio".into())));
    let quick = Duration::from_secs(3);
    let asked = [("running".into(), quick), ("Debian".into(), quick)];
    assert_eq!(given[1..], asked);
    // Where none answers, the default is said.
    let (found, _) = search_with(&[("default", "rexy-name:Ubuntu\n".into())]);
    assert_eq!(found, Err(Some("Ubuntu".into())));
    assert_eq!(search_with(&[]).0, Err(None));
}

#[test]
fn a_socket_is_never_taken_for_a_switch_of_perls() {
    let args = bridge_args("/run/user/1000/rex/server.sock").unwrap();
    assert_eq!(args, ["perl", "-e", BRIDGE, "--", "/run/user/1000/rex/server.sock"]);
    for bad in ["-eBEGIN{system(1)}", "run/rex.sock", "", "--"] {
        assert!(bridge_args(bad).is_err(), "{bad:?} is no socket");
    }
}

#[test]
fn what_would_not_reach_wsl_as_it_is_is_refused() {
    let args = bridge_args("/run/rex/server.sock").unwrap();
    assert!(passable("Ubuntu-24.04", &args).is_ok());
    let error = passable("Ubuntu", &["perl", "-e", BRIDGE, "--", r"/run/x\y.sock"]);
    assert!(error.unwrap_err().to_string().contains("backslash"));
    assert!(passable("Ubuntu", &["rm", "-f", "--", "/run/\"x\".sock"]).is_err());
    for distro in ["", "-d", "Ubu ntu", "Ubu\"ntu", r"Ubu\ntu"] {
        assert!(passable(distro, &["true"]).is_err(), "{distro:?} is no name");
    }
}

#[test]
fn the_servers_answer_comes_after_wsls_warnings() {
    assert_eq!(answer_in(b"HTTP/1.1 101 Switching"), (0, Some(true)));
    assert_eq!(answer_in(b"HTT"), (0, None));
    assert_eq!(answer_in(b""), (0, None));
    assert_eq!(answer_in(b"There is no distribution"), (0, Some(false)));
    let warning = b"wsl: Localhost proxy is not supported\r\n".as_slice();
    let warned = [warning, b"HTTP/1.1 101"].concat();
    assert_eq!(answer_in(&warned), (warning.len(), Some(true)));
    // A warning not yet whole, or nothing yet after it, is not known.
    assert_eq!(answer_in(b"wsl: Localhost"), (0, None));
    assert_eq!(answer_in(b"ws"), (0, None));
    assert_eq!(answer_in(b"wsl: one\nwsl: two\n"), (18, None));
    assert_eq!(answer_in(b"wsl: one\nrexy bridge: gone"), (9, Some(false)));
    assert_eq!(unwarned(b"wsl: one\n{}"), b"{}");
}

#[test]
fn home_is_looked_for_in_the_default_distribution_then_those_running() {
    let running = "Ubuntu\r\ndocker-desktop\r\ndocker-desktop-data\r\nDebian\r\n\r\n";
    assert_eq!(distros(Some("Ubuntu"), running), ["Ubuntu", "Debian"]);
    assert_eq!(distros(Some("Arch"), running), ["Arch", "Ubuntu", "Debian"]);
    assert_eq!(distros(None, running), ["Ubuntu", "Debian"]);
    assert_eq!(distros(Some("ubuntu"), "Ubuntu\r\n"), ["ubuntu"]);
    assert_eq!(distros(Some("docker-desktop"), ""), Vec::<String>::new());
    assert_eq!(distros(None, ""), Vec::<String>::new());
}

#[test]
fn a_file_on_windows_is_named_as_wsl_reads_it() {
    let at = |path: &str| path_in(path, "Ubuntu", "/mnt/");
    assert_eq!(at(r"C:\x y\z.png").as_deref(), Some("/mnt/c/x y/z.png"));
    assert_eq!(at(r"d:\Photos\a.jpg").as_deref(), Some("/mnt/d/Photos/a.jpg"));
    assert_eq!(at(r"\\?\C:\long\a.txt").as_deref(), Some("/mnt/c/long/a.txt"));
    assert_eq!(path_in(r"C:\a", "Ubuntu", "/").as_deref(), Some("/c/a"));
    // The distribution's own files, by their names inside it.
    assert_eq!(at(r"\\wsl.localhost\Ubuntu\home\kyle\a").as_deref(), Some("/home/kyle/a"));
    assert_eq!(at(r"\\wsl$\Ubuntu\home\kyle\a").as_deref(), Some("/home/kyle/a"));
    assert_eq!(at(r"\\WSL.LOCALHOST\ubuntu\etc").as_deref(), Some("/etc"));
    assert_eq!(at(r"\\?\UNC\wsl.localhost\Ubuntu\tmp\b").as_deref(), Some("/tmp/b"));
    // Another distribution's, a share's, and what is no path, it cannot read.
    assert_eq!(at(r"\\wsl.localhost\Debian\home\kyle\a"), None);
    assert_eq!(at(r"\\nas\photos\a.jpg"), None);
    assert_eq!(at(r"relative\a.txt"), None);
    assert_eq!(at(""), None);
}

#[test]
fn the_drives_are_mounted_where_wslpath_says() {
    assert_eq!(mount_root("/mnt/c/\n").as_deref(), Some("/mnt/"));
    assert_eq!(mount_root("/c/\n").as_deref(), Some("/"));
    assert_eq!(mount_root("/windows/c").as_deref(), Some("/windows/"));
    assert_eq!(mount_root("wslpath: C:/: Invalid argument"), None);
}

#[test]
fn the_reason_is_the_last_thing_said_in_words() {
    let wsl = b"There is no distribution with the supplied name.\r\nError code: Wsl/Service/WSL_E_DISTRO_NOT_FOUND\r\n";
    assert_eq!(reason(wsl).as_deref(), Some("There is no distribution with the supplied name."));
    let perl = b"rexy bridge: /run/x.sock: No such file or directory at -e line 1.\n";
    assert_eq!(
        reason(perl).as_deref(),
        Some("rexy bridge: /run/x.sock: No such file or directory")
    );
    assert_eq!(reason(b"\n\n"), None);
}

/// A bridge whose program prints something other than a server's answer, and then
/// waits on its input: what it printed is why the connection failed.
#[cfg(windows)]
#[test]
fn what_is_no_answer_is_why() {
    use tokio::io::AsyncReadExt;
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let mut command = Command::new("cmd.exe");
        command.args(["/c", "echo There is no distribution with the supplied name."]);
        let mut bridge = Bridge::spawn(command).unwrap();
        let mut read = Vec::new();
        let error = bridge.read_to_end(&mut read).await.unwrap_err();
        assert_eq!(error.to_string(), "There is no distribution with the supplied name.");
        assert!(read.is_empty());
    });
}

/// Windows reads a child's output on threads that cannot be stopped, which a runtime
/// waits for as it ends. Dropping the bridges with it kills what they wait on.
#[cfg(windows)]
#[test]
fn a_runtime_ends_without_waiting_on_its_bridges() {
    use tokio::io::AsyncReadExt;
    let (done, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            for _ in 0..3 {
                // It goes on for a minute whatever its input does, as a bridge
                // does while its server talks.
                let mut command = Command::new("ping.exe");
                command.args(["-n", "60", "127.0.0.1"]);
                let mut bridge = Bridge::spawn(command).unwrap();
                tokio::spawn(async move { bridge.read_to_end(&mut Vec::new()).await });
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        });
        drop(runtime);
        let _ = done.send(());
    });
    let ended = ended.recv_timeout(Duration::from_secs(5));
    assert!(ended.is_ok(), "the runtime waited on its bridges");
}

/// Connects to the server in the WSL distribution `REXY_TEST_WSL` names, and only
/// reads its welcome.
#[cfg(windows)]
#[test]
#[ignore = "needs Rex running in WSL; set REXY_TEST_WSL to the distribution"]
fn a_server_in_wsl_welcomes_this_client() {
    let distro = std::env::var("REXY_TEST_WSL").expect("REXY_TEST_WSL names a distribution");
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let endpoint = format!("wsl://{distro}");
        let control = wire::Control::connect(&endpoint, None, None).await.unwrap();
        let Target::Wsl { distro: reached, socket: Some(socket) } = &control.target else {
            panic!("reached {:?}", control.target);
        };
        assert!(reached == &distro && socket.starts_with('/'), "{socket}");
        assert!(!control.client_id.is_empty());
        // The socket found is the one streams go to, and is reached as named.
        let named = format!("wsl://{distro}{socket}");
        let again = wire::Control::connect(&named, None, Some(&control.target)).await.unwrap();
        assert_eq!(again.target, control.target);
    });
}

#[test]
fn the_place_for_sockets_is_where_wsl_made_it() {
    let said = "wsl: a warning\r\nrexy-dir:/run/user/1000/rexy\nrexy-socket:ssh-4242-0.sock\nrexy-socket:ssh-77-3.sock\n";
    let (directory, sockets) = placed(said).unwrap();
    assert_eq!(directory, "/run/user/1000/rexy");
    assert_eq!(sockets, ["ssh-4242-0.sock", "ssh-77-3.sock"]);
    // Where there is no runtime directory, it is this user's own in /tmp.
    let none = placed("rexy-dir:/tmp/rexy-1000\r\n").unwrap();
    assert_eq!(none, ("/tmp/rexy-1000".into(), Vec::new()));
    // What could not be passed back to WSL, or is no path, is no place.
    assert_eq!(placed("rexy-dir:/run/us\"er/rexy\n"), None);
    assert_eq!(placed("rexy-dir:run/rexy\n"), None);
    assert_eq!(placed("rexy: /tmp/rexy-1000 is not a directory of this user's own\n"), None);
    // The script finds the directory as the Unix client does.
    assert!(PLACE.contains("XDG_RUNTIME_DIR") && PLACE.contains("/tmp/rexy-$>"));
    assert!(PLACE.contains("mkdir $d,0700") && PLACE.contains("$s[4]==$>"));
}

#[test]
fn the_drives_are_mounted_where_wslpath_says_after_any_warning() {
    assert_eq!(mount_root("wsl: a warning\r\n/mnt/c/\n").as_deref(), Some("/mnt/"));
}

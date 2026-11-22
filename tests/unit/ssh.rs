use super::*;

#[test]
fn an_endpoint_splits_into_a_destination_and_an_optional_socket() {
    assert_eq!(parse("ssh://studio"), ("ssh://studio", None));
    assert_eq!(parse("ssh://me@studio:2222"), ("ssh://me@studio:2222", None));
    assert_eq!(
        parse("ssh://me@studio/run/rex/server.sock"),
        ("ssh://me@studio", Some("/run/rex/server.sock"))
    );
}

#[test]
fn the_reason_is_the_last_thing_ssh_said() {
    let stderr = b"Warning: something\nme@studio: Permission denied (publickey).\n\n";
    assert_eq!(reason(stderr), "me@studio: Permission denied (publickey).");
    assert_eq!(reason(b""), "ssh gave no reason");
}

#[test]
fn the_servers_socket_is_where_its_own_rex_found_it() {
    use serde_json::json;
    let (first, second) = ("/run/user/1000/rex/server.sock", "/run/rex/server.sock");
    let listeners = json!([
        {"endpoint": "127.0.0.1:0", "addresses": ["127.0.0.1:52061"], "transport": "tcp"},
        {"endpoint": "unix:///run/rex/server.sock", "addresses": [second],
         "transport": "unix", "primary": true},
    ]);
    let socket = |status: Value| remote_socket(&status);
    let both = json!({"endpoint": first, "listeners": listeners});
    assert_eq!(socket(both).as_deref(), Some(first));
    // Rex writes a socket as a URL, its path percent-encoded.
    let mac = "unix:///Users/me/Library/Application%20Support/rex/server.sock";
    let path = "/Users/me/Library/Application Support/rex/server.sock";
    assert_eq!(socket(json!({"endpoint": mac})).as_deref(), Some(path));
    // An endpoint that is no socket leaves it to the list.
    let listed = json!({"endpoint": "127.0.0.1:7297", "listeners": listeners});
    assert_eq!(socket(listed).as_deref(), Some(second));
    let unnamed = json!({"listeners": listeners});
    assert_eq!(socket(unnamed).as_deref(), Some(second));
    assert_eq!(socket(json!({"endpoint": "127.0.0.1:7297"})), None);
    assert_eq!(socket(json!({"endpoint": "unix:here.sock"})), None);
    assert_eq!(socket(json!({"pid": 4242})), None);
}

#[tokio::test]
async fn what_ssh_says_is_read_to_its_end_and_the_last_of_it_kept() {
    let mut said = b"Warning: something\n".repeat(1000);
    said.extend(b"Timeout, server studio not responding.\n");
    let kept = drain(&said[..]).await;
    assert_eq!(kept.len(), KEPT);
    assert_eq!(reason(&kept), "Timeout, server studio not responding.");
    assert_eq!(drain(&b""[..]).await, b"");
}

#[test]
#[cfg(unix)]
fn the_directory_for_sockets_is_this_users_own() {
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no arguments and cannot fail.
    let user = unsafe { libc::geteuid() };
    let name = format!("rexy-test-{}", std::process::id());
    let root = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&root);
    let (own, link, file) = (root.join("own"), root.join("link"), root.join("file"));
    private(&own, user).unwrap();
    let mode = std::fs::metadata(&own).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o700);
    // There already, it will do.
    private(&own, user).unwrap();
    // But not if it is another's, or only a link to a directory, or no directory.
    assert!(private(&own, user + 1).is_err());
    std::os::unix::fs::symlink(&own, &link).unwrap();
    assert!(private(&link, user).is_err());
    std::fs::write(&file, "").unwrap();
    assert!(private(&file, user).is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

/// What `wsl.exe` would not pass on as it is.
fn unpassable(arg: &str) -> bool {
    arg.contains(['\\', '"'])
}

#[test]
fn what_is_run_through_wsl_survives_the_way_windows_passes_it_on() {
    assert!(!unpassable(STATUS_WSL) && !unpassable(READY));
    // It asks as `STATUS` does, but for the quotes.
    assert_eq!(STATUS_WSL, STATUS.replace('"', ""));
    let status = status_in_wsl("ssh://me@studio:2222").unwrap();
    assert!(!status.iter().any(|arg| unpassable(arg)));
    assert_eq!(status[0], "ssh");
    assert_eq!(status[status.len() - 3..], ["--", "ssh://me@studio:2222", STATUS_WSL]);
    for option in OPTIONS {
        assert!(status.windows(2).any(|pair| pair == ["-o", option]));
    }
    // What would not reach `ssh` as it is written is refused, not changed.
    let error = forward_in_wsl("/run/user/1000/rexy/a.sock", r"C:\rex.sock", "ssh://studio");
    assert!(error.unwrap_err().to_string().contains("backslash"));
    assert!(status_in_wsl("ssh://\"studio\"").is_err());
}

#[test]
fn a_forward_keeps_each_socket_whole_whatever_its_spaces() {
    let local = "/run/user/1000/rexy/ssh-4242-0.sock";
    let remote = "/Users/kyle/Library/Application Support/rex/server.sock";
    let endpoint = format!("ssh://kyle@mac{remote}");
    let (destination, socket) = parse(&endpoint);
    assert_eq!((destination, socket), ("ssh://kyle@mac", Some(remote)));
    let args = forward_in_wsl(local, socket.unwrap(), destination).unwrap();
    let forward = format!("{local}:{remote}");
    let tail = [
        "-o",
        "ExitOnForwardFailure=yes",
        "-o",
        "StreamLocalBindUnlink=yes",
        "-o",
        "StreamLocalBindMask=0177",
        "-L",
        &forward,
        "--",
        "ssh://kyle@mac",
        "cat",
    ];
    assert_eq!(args[args.len() - tail.len()..], tail);
    assert_eq!(args.len(), 1 + 2 * OPTIONS.len() + 1 + tail.len());
    assert_eq!(args[2 * OPTIONS.len() + 1], "-T");
}

#[test]
fn a_forward_in_wsl_is_reached_through_wsl() {
    assert_eq!(socket_name(4242, 0), "ssh-4242-0.sock");
    assert_eq!(socket_name(4242, 7), "ssh-4242-7.sock");
    let path = "/run/user/1000/rexy/ssh-4242-0.sock";
    let endpoint = wsl_endpoint("Ubuntu", path);
    assert_eq!(endpoint, "wsl://Ubuntu/run/user/1000/rexy/ssh-4242-0.sock");
    let socket = Some(path.to_owned());
    let target = wire::Target::Wsl { distro: "Ubuntu".into(), socket };
    assert_eq!(wire::targets_from(&endpoint, None).unwrap(), [target]);
}

#[test]
fn the_forwards_socket_is_never_taken_for_a_switch_of_perls() {
    let local = "/run/user/1000/rexy/ssh-4242-0.sock";
    assert_eq!(ready_in_wsl(local), ["perl", "-e", READY, "--", local]);
}

#[test]
fn only_sockets_of_processes_that_are_gone_are_cleared_out() {
    let gone = |name: &str| left_behind(name, |pid| pid == 7);
    assert!(gone("ssh-4242-0.sock"));
    assert!(gone("ssh-1-12.sock"));
    // Those of a process still running, this one among them, are kept.
    assert!(!gone("ssh-7-0.sock"));
    assert!(!gone(&socket_name(std::process::id(), 3)));
    // As is what was not named so.
    for other in ["ssh-4242-0.sock.bak", "ssh-4242.sock", "ssh--0.sock", "ssh-a-0.sock"] {
        assert!(!gone(other), "{other}");
    }
    for other in ["ssh-4242-0-1.sock", "ssh-+1-0.sock", "x-ssh-1-0.sock", "ssh-99999999999-0.sock"]
    {
        assert!(!gone(other), "{other}");
    }
}

#[test]
fn without_a_distribution_in_wsl_there_is_nowhere_to_forward_to() {
    assert_eq!(ssh_distro(Some("Ubuntu".into())).unwrap(), "Ubuntu");
    let error = ssh_distro(None).unwrap_err().to_string();
    assert!(error.starts_with("ssh:// needs a distribution in WSL"), "{error}");
}

//! Background application handling.

use crate::utils::logging::{log_debug, log_info};
use std::collections::HashSet;
use std::process::Command;

/// Force-stop every package that is not visible in any window.
pub fn clear_background_apps() {
    // Run dumpsys window displays
    let output = Command::new("dumpsys").args(["window", "displays"]).output();
    let stdout = match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => return,
    };

    let mut visible_pkgs = HashSet::new();
    let mut invisible_pkgs = HashSet::new();

    // Read the output line by line (same as grep "Task{")
    for line in stdout.lines() {
        if line.contains("Task{") {
            // Replicates the sed logic: 's/.*A=[0-9]*:\([^ ]*\).*/\1/p'
            if let Some(a_idx) = line.find("A=") {
                let part = &line[a_idx..];
                if let Some(colon_idx) = part.find(':') {
                    if let Some(space_idx) = part.find(' ') {
                        if colon_idx < space_idx {
                            let pkg = &part[colon_idx + 1..space_idx];

                            if line.contains("visible=true") {
                                visible_pkgs.insert(pkg.to_string());
                            } else if line.contains("visible=false") {
                                invisible_pkgs.insert(pkg.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Packages that must never be force-stopped by the background
    // cleaner. Matched EXACTLY: the old substring match both over-matched
    // (any package merely containing "system"/"android") and under-matched
    // the apps users actually keep running in the background.
    let exclude = [
        "com.android.systemui",
        "com.android.settings",
        "android",
        "system",
        // Music / media players: killing these mid-playback is the
        // reported "game + music" breakage. Playback state is also
        // double-checked against media_session below.
        "com.spotify.music",
        "com.google.android.apps.youtube.music",
        "com.google.android.youtube",
        "com.apple.android.music",
        "com.deezer.android.app",
        "com.aspiro.tidal",
        "com.soundcloud.android",
        "com.jio.media.jiosaavn",
    ];

    // Any package holding an active media session is playing (or ready
    // to play) audio/video: never force-stop it, whatever it is.
    let media_session_dump = Command::new("dumpsys")
        .args(["media_session"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    for pkg in invisible_pkgs {
        if visible_pkgs.contains(&pkg) {
            continue;
        }

        let is_excluded = exclude.iter().any(|&ex| pkg == ex);
        let has_media_session = !media_session_dump.is_empty() && media_session_dump.contains(&pkg);

        if !is_excluded && !has_media_session {
            let _ = Command::new("am")
                .args(["force-stop", &pkg])
                .status();

            log_debug("apps", &format!("stopped app pkg={}", pkg));
        }
    }

    log_info("apps", "cleared background apps");
}

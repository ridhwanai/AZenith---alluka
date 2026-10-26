//! CPU frequency reads/writes (plain sysfs and the MediaTek PPM path).

use crate::utils::logging::log_debug;
use crate::utils::props::{get_curprofile, get_limiter, get_litemode};
use crate::utils::sysfs::{chmod, write_lock, write_unlock};
use std::fs;
use std::path::Path;

pub fn read_freqs(path: &str) -> Vec<u64> {
    let mut freqs: Vec<u64> = fs::read_to_string(path)
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|s: &str| s.parse().ok())
        .collect();
    freqs.sort_unstable();
    freqs
}

/// cpufreq `policy*` paths sorted by numeric policy id (policy0, policy6, ...).
/// Glob iteration order is filesystem order, and the PPM writers below map
/// the iteration index onto the kernel's cluster index — an unsorted walk
/// can write the big cluster's limits to cluster 0 and vice versa.
fn sorted_policy_paths(pattern: &str) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = match glob::glob(pattern) {
        Ok(g) => g.flatten().collect(),
        Err(_) => Vec::new(),
    };
    paths.sort_by_key(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| {
                n.trim_start_matches(|c: char| !c.is_ascii_digit())
                    .parse::<u32>()
                    .ok()
            })
            .unwrap_or(u32::MAX)
    });
    paths
}

pub fn which_maxfreq(path: &str) -> Option<u64> {
    read_freqs(path).into_iter().max()
}

pub fn which_minfreq(path: &str) -> Option<u64> {
    read_freqs(path).into_iter().min()
}

pub fn which_midfreq(path: &str) -> Option<u64> {
    let freqs = read_freqs(path);
    if freqs.is_empty() {
        return None;
    }
    // Equivalent to the awk middle-value logic
    Some(freqs[freqs.len() / 2])
}

/// Find the frequency in `file` that is closest to `target`.
pub fn setfreqs(file: &str, target: u64) -> u64 {
    let freqs = read_freqs(file);
    if freqs.is_empty() {
        return target;
    }
    *freqs
        .iter()
        .min_by_key(|&&f| (f as i64 - target as i64).abs())
        .unwrap_or(&target)
}

/// Maximum GPU frequency parsed from the MediaTek OPP dump.
pub fn get_mtk_gpu_max_freq() -> Option<u64> {
    let content = fs::read_to_string("/proc/gpufreq/gpufreq_opp_dump").unwrap_or_default();
    content.lines()
        .filter(|line: &&str| line.contains("freq = "))
        .filter_map(|line: &str| line.split("freq = ").nth(1)?.split_whitespace().next()?.parse::<u64>().ok())
        .max()
}

/// Re-apply the balanced frequency at runtime
pub fn applyfreqbalance() {
    if Path::new("/proc/ppm").exists() {
        setfreqppm();
    } else {
        setfreq();
    }
}

/// Re-apply the performance frequency at runtime.
pub fn applyfreqgame() {
    if Path::new("/proc/ppm").exists() {
        setgamefreqppm();
    } else {
        setgamefreq();
    }
}

pub fn setfreqppm() {
    if !Path::new("/proc/ppm").exists() {
        return;
    }

    let limiter = get_limiter();
    let curprofile = get_curprofile();
    let mut cluster = 0;

    for path in sorted_policy_paths("/sys/devices/system/cpu/cpufreq/policy*") {
        let Some(p_str) = path.to_str() else { continue };
        {
            let policy_name = path.file_name().unwrap_or_default().to_string_lossy();

            let cpu_maxfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_max_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);
            let cpu_minfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_min_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);

            let new_max_target = cpu_maxfreq * limiter / 100;
            let avail_file = format!("{}/scaling_available_frequencies", p_str);
            let mut new_maxfreq = setfreqs(&avail_file, new_max_target);

            if curprofile == "3" {
                new_maxfreq = which_midfreq(&avail_file).unwrap_or(new_maxfreq);
            }

            write_unlock(&format!("{} {}", cluster, new_maxfreq), "/proc/ppm/policy/hard_userlimit_max_cpu_freq");
            write_unlock(&format!("{} {}", cluster, cpu_minfreq), "/proc/ppm/policy/hard_userlimit_min_cpu_freq");

            log_debug("cpu", &format!("set policy={} maxfreq={} minfreq={}", policy_name, new_maxfreq, cpu_minfreq));
            cluster += 1;
        }
    }
}

pub fn setfreq() {
    let limiter = get_limiter();
    let curprofile = get_curprofile();

    if let Ok(paths) = glob::glob("/sys/devices/system/cpu/*/cpufreq") {
        for path in paths.flatten() {
            let Some(p_str) = path.to_str() else { continue };

            let policy_name = path.parent()
                .and_then(|p: &Path| p.file_name())
                .unwrap_or_default()
                .to_string_lossy();

            let cpu_maxfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_max_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);
            let cpu_minfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_min_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);

            let new_max_target = cpu_maxfreq * limiter / 100;
            let avail_file = format!("{}/scaling_available_frequencies", p_str);
            let mut new_maxfreq = setfreqs(&avail_file, new_max_target);

            if curprofile == "3" {
                new_maxfreq = which_midfreq(&avail_file).unwrap_or(new_maxfreq);
            }

            write_unlock(&new_maxfreq.to_string(), &format!("{}/scaling_max_freq", p_str));
            write_unlock(&cpu_minfreq.to_string(), &format!("{}/scaling_min_freq", p_str));

            log_debug("cpu", &format!("set policy={} maxfreq={} minfreq={}", policy_name, new_maxfreq, cpu_minfreq));
        }
    }

    if let Ok(sc_paths) = glob::glob("/sys/devices/system/cpu/cpufreq/policy*/scaling_*_freq") {
        for sp in sc_paths.flatten() {
            if let Some(sp_str) = sp.to_str() {
                chmod(sp_str, 0o644);
            }
        }
    }
}

pub fn setgamefreqppm() {
    if !Path::new("/proc/ppm").exists() {
        return;
    }

    let litemode = get_litemode();
    let mut cluster = 0;

    for path in sorted_policy_paths("/sys/devices/system/cpu/cpufreq/policy*") {
        let Some(p_str) = path.to_str() else { continue };
        {
            let policy_name = path.file_name().unwrap_or_default().to_string_lossy();

            let cpu_maxfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_max_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);
            let avail_file = format!("{}/scaling_available_frequencies", p_str);

            // Encore: lite mode => max stays at max, min goes to mid freq
            let cpu_minfreq = if litemode {
                which_midfreq(&avail_file).unwrap_or(cpu_maxfreq)
            } else {
                cpu_maxfreq
            };

            write_unlock(&format!("{} {}", cluster, cpu_maxfreq), "/proc/ppm/policy/hard_userlimit_max_cpu_freq");
            write_unlock(&format!("{} {}", cluster, cpu_minfreq), "/proc/ppm/policy/hard_userlimit_min_cpu_freq");

            log_debug("cpu", &format!("set policy={} maxfreq={} minfreq={}", policy_name, cpu_maxfreq, cpu_minfreq));
            cluster += 1;
        }
    }
}

pub fn setgamefreq() {
    let litemode = get_litemode();

    if let Ok(paths) = glob::glob("/sys/devices/system/cpu/*/cpufreq") {
        for path in paths.flatten() {
            let Some(p_str) = path.to_str() else { continue };
            let policy_name = path.parent()
                .and_then(|p: &Path| p.file_name())
                .unwrap_or_default()
                .to_string_lossy();

            let cpu_maxfreq: u64 = fs::read_to_string(format!("{}/cpuinfo_max_freq", p_str))
                .unwrap_or_default().trim().parse().unwrap_or(0);
            let avail_file = format!("{}/scaling_available_frequencies", p_str);

            let cpu_minfreq = if litemode {
                which_midfreq(&avail_file).unwrap_or(cpu_maxfreq)
            } else {
                cpu_maxfreq
            };

            write_lock(&cpu_maxfreq.to_string(), &format!("{}/scaling_max_freq", p_str));
            write_lock(&cpu_minfreq.to_string(), &format!("{}/scaling_min_freq", p_str));

            log_debug("cpu", &format!("set policy={} maxfreq={} minfreq={}", policy_name, cpu_maxfreq, cpu_minfreq));
        }
    }

    if let Ok(sc_paths) = glob::glob("/sys/devices/system/cpu/cpufreq/policy*/scaling_*_freq") {
        for sp in sc_paths.flatten() {
            if let Some(sp_str) = sp.to_str() {
                chmod(sp_str, 0o444);
            }
        }
    }
}
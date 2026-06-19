use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, Seek, SeekFrom, Write};
use std::net::IpAddr;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chrono::Local;
use serde::{Deserialize, Serialize};

const VERSION: &str = "3.0.0";
const CHECK_INTERVAL_SECS: u64 = 10;
const SSH_LOG: &str = "/var/log/secure";
const WEB_LOG: &str = "/var/log/nginx/access.log";
const STATE_FILE: &str = "/var/lib/epicode-guard/state.json";
const LOG_FILE: &str = "/var/log/epicode-guard/guard.log";
const PID_FILE: &str = "/var/run/epicode-guard.pid";
const EPICODE_API: &str = "http://127.0.0.1:9111";

fn get_epicode_key() -> String {
    std::env::var("EPICODE_API_KEY").unwrap_or_default()
}

const NFT_TABLE: &str = "epicode_guard";
const NFT_SET: &str = "ban";

const SSH_MAX_FAIL: u32 = 5;
const WEB_ATTACK_SCORE: u32 = 5;
const WEB_SCAN_SCORE: u32 = 1;
const HONEYPOT_SCORE: u32 = 100;
const FLOOD_THRESHOLD: usize = 30;
const BAN_THRESHOLD: u32 = 10;
const BAN_TIMEOUT_SECS: u64 = 86400;
const DECAY_INTERVAL_SECS: u64 = 300;
const DECAY_AMOUNT: u32 = 2;
const FILE_CHECK_INTERVAL_SECS: u64 = 300;

fn get_honeypot_ports() -> Vec<u16> {
    std::env::var("GUARD_HONEYPOT_PORTS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

const WHITELIST: &[&str] = &["127.0.0.1", "::1", "0.0.0.0", "::"];

const MONITORED_FILES: &[&str] = &[
    "/etc/nginx/conf.d/default.conf",
    "/etc/ssh/sshd_config",
    "/etc/systemd/system/epicode.service",
    "/usr/local/bin/epicode-cloud",
    "/etc/sysctl.d/99-hardening.conf",
    "/etc/modprobe.d/hardening.conf",
    "/etc/passwd",
    "/etc/shadow",
];

const EXPECTED_PORTS: &[u16] = &[22, 80, 443];

const WEB_ATTACK_PATTERNS: &[&str] = &[
    "union select",
    "or 1=1",
    "' or ",
    "\" or ",
    "; drop ",
    "information_schema",
    "sleep(",
    "benchmark(",
    "<script",
    "javascript:",
    "onerror=",
    "onload=",
    "alert(",
    "document.cookie",
    "../",
    "..%2f",
    "..%5c",
    "/etc/passwd",
    "/proc/self",
    "/etc/shadow",
    ".env",
    ".git/",
    ".git/config",
    "wp-admin",
    "wp-login",
    "phpmyadmin",
    "wp-config",
    ".ds_store",
    "backup.sql",
    ".htaccess",
    "/debug",
    "/console",
    "/actuator",
    "/phpinfo",
    "/server-status",
    "/.svn",
    "/.hg",
    ";ls",
    ";cat ",
    ";id",
    ";whoami",
    ";uname",
    "%0d%0a",
    "$(",
    "`id`",
    "eval(",
    "base64_decode",
    "file_get_contents",
    "/bin/sh",
    "/bin/bash",
    "/usr/bin/",
    "cmd.exe",
    "powershell",
    "config.json",
    "docker.env",
    ".env.dev",
    ".env.backup",
    ".env.old",
    ".env.bak",
    ".env.local",
    ".env.production",
];

#[derive(Serialize, Deserialize, Clone)]
struct IpEntry {
    score: u32,
    last_fail: i64,
    banned_until: i64,
    ssh_fails: u32,
    web_attacks: u32,
    web_scans: u32,
    honeypot_hits: u32,
}

impl Default for IpEntry {
    fn default() -> Self {
        Self {
            score: 0,
            last_fail: 0,
            banned_until: 0,
            ssh_fails: 0,
            web_attacks: 0,
            web_scans: 0,
            honeypot_hits: 0,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct GuardState {
    ssh_offset: u64,
    web_offset: u64,
    ips: HashMap<String, IpEntry>,
    file_hashes: HashMap<String, String>,
    last_decay: i64,
    last_file_check: i64,
    total_bans: u64,
    total_attacks: u64,
    total_honeypot: u64,
    start_time: i64,
    last_ports: Vec<u16>,
}

impl Default for GuardState {
    fn default() -> Self {
        Self {
            ssh_offset: 0,
            web_offset: 0,
            ips: HashMap::new(),
            file_hashes: HashMap::new(),
            last_decay: 0,
            last_file_check: 0,
            total_bans: 0,
            total_attacks: 0,
            total_honeypot: 0,
            start_time: now_ts(),
            last_ports: Vec::new(),
        }
    }
}

impl GuardState {
    fn load() -> Self {
        match fs::read_to_string(STATE_FILE) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn save(&self) {
        if let Ok(data) = serde_json::to_string_pretty(self) {
            if let Some(parent) = std::path::Path::new(STATE_FILE).parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(STATE_FILE, data);
        }
    }

    fn record(&mut self, ip: &str, score: u32, category: &str, now: i64) {
        if is_whitelisted(ip) {
            return;
        }
        let entry = self.ips.entry(ip.to_string()).or_default();
        entry.score += score;
        entry.last_fail = now;
        match category {
            "ssh" => entry.ssh_fails += 1,
            "web_attack" => entry.web_attacks += 1,
            "web_scan" => entry.web_scans += 1,
            "honeypot" => entry.honeypot_hits += 1,
            "flood" => entry.web_scans += 1,
            _ => {}
        }
        if score > 0 {
            self.total_attacks += 1;
        }
    }

    fn decay(&mut self, now: i64) {
        let mut to_remove = Vec::new();
        for (ip, entry) in self.ips.iter_mut() {
            if entry.banned_until > 0 {
                continue;
            }
            if now - entry.last_fail > DECAY_INTERVAL_SECS as i64 {
                if entry.score > DECAY_AMOUNT {
                    entry.score -= DECAY_AMOUNT;
                } else {
                    to_remove.push(ip.clone());
                }
            }
        }
        for ip in to_remove {
            self.ips.remove(&ip);
        }
    }

    fn process_bans(&mut self, now: i64) {
        let mut to_ban: Vec<(String, u64)> = Vec::new();
        let mut to_clean: Vec<String> = Vec::new();
        for (ip, entry) in self.ips.iter() {
            if entry.banned_until > 0 {
                if now > entry.banned_until {
                    to_clean.push(ip.clone());
                }
                continue;
            }
            if entry.score >= BAN_THRESHOLD {
                let timeout = if entry.ssh_fails >= SSH_MAX_FAIL || entry.honeypot_hits > 0 {
                    BAN_TIMEOUT_SECS * 7
                } else if entry.web_attacks > 0 {
                    BAN_TIMEOUT_SECS * 2
                } else {
                    BAN_TIMEOUT_SECS
                };
                to_ban.push((ip.clone(), timeout));
                self.total_bans += 1;
                log_msg(&format!(
                    "BANNED {} (score={} ssh={} web_atk={} web_scan={} honeypot={}) for {}h",
                    ip,
                    entry.score,
                    entry.ssh_fails,
                    entry.web_attacks,
                    entry.web_scans,
                    entry.honeypot_hits,
                    timeout / 3600
                ));
            }
        }
        for (ip, timeout) in &to_ban {
            nft_ban(ip, *timeout);
            if let Some(e) = self.ips.get_mut(ip) {
                e.banned_until = now + *timeout as i64;
                epicode_remember_ban(ip, e, *timeout);
            }
        }
        for ip in to_clean {
            self.ips.remove(&ip);
        }
    }

    fn reapply_bans(&mut self, now: i64) {
        let mut count = 0u64;
        for (ip, entry) in self.ips.iter() {
            if entry.banned_until > now {
                let remaining = (entry.banned_until - now) as u64;
                nft_ban(ip, remaining);
                count += 1;
            }
        }
        if count > 0 {
            log_msg(&format!("Re-applied {} bans from state", count));
        }
        let mut expired = 0u64;
        self.ips.retain(|_, entry| {
            if entry.banned_until > 0 && entry.banned_until <= now {
                expired += 1;
                false
            } else {
                true
            }
        });
        if expired > 0 {
            log_msg(&format!("Cleaned {} expired bans from state", expired));
        }
    }
}

fn epicode_remember(content: &str, labels: &[&str]) {
    let labels_str = labels
        .iter()
        .map(|l| format!("\"{}\"", l))
        .collect::<Vec<_>>()
        .join(",");
    let body = format!(
        r#"{{"content":"{}","labels":[{}]}}"#,
        content
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', " ")
            .replace('\r', ""),
        labels_str
    );
    let success = Command::new("curl")
        .args(&[
            "-s",
            "-X",
            "POST",
            &format!("{}/v1/remember", EPICODE_API),
            "-H",
            &format!("X-API-Key: {}", get_epicode_key()),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
        ])
        .output()
        .map(|o| {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("success")
        })
        .unwrap_or(false);
    if !success {
        log_msg("Failed to write attack memory to Epicode");
    }
}

fn epicode_remember_ban(ip: &str, entry: &IpEntry, timeout: u64) {
    let attack_type = if entry.honeypot_hits > 0 {
        "honeypot"
    } else if entry.web_attacks > 0 && entry.ssh_fails > 0 {
        "ssh+web"
    } else if entry.web_attacks > 0 {
        "web-attack"
    } else {
        "ssh-bruteforce"
    };
    let content = format!(
        "Security ban: IP {} | type={} | score={} | ssh_fails={} | web_attacks={} | honeypot={} | duration={}h | source=epicode-guard-v{}",
        ip, attack_type, entry.score, entry.ssh_fails, entry.web_attacks, entry.honeypot_hits, timeout / 3600, VERSION
    );
    let labels = &["security", "auto-banned", attack_type, "guard-memory"];
    epicode_remember(&content, labels);
}

fn epicode_remember_honeypot(ip: &str, port: u16) {
    let content = format!(
        "Honeypot capture: IP {} connected to decoy port {} | instant ban | source=epicode-guard-v{}",
        ip, port, VERSION
    );
    let labels = &["security", "honeypot", "decoy", "guard-memory"];
    epicode_remember(&content, labels);
}

fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

fn log_msg(msg: &str) {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{}] {}", ts, msg);
    println!("{}", line);
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_FILE)
    {
        let _ = writeln!(f, "{}", line);
    }
}

fn is_whitelisted(ip: &str) -> bool {
    WHITELIST.contains(&ip)
        || ip.starts_with("10.")
        || ip.starts_with("172.16.")
        || ip.starts_with("192.168.")
        || ip == "::1"
}

fn run_cmd(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_cmd_output(cmd: &str, args: &[&str]) -> Option<String> {
    Command::new(cmd).args(args).output().ok().and_then(|o| {
        if o.status.success() {
            Some(String::from_utf8_lossy(&o.stdout).to_string())
        } else {
            None
        }
    })
}

fn nft_init() {
    migrate_v1_rules();
    let _ = Command::new("nft")
        .args(&["delete", "table", "inet", NFT_TABLE])
        .output();
    run_cmd("nft", &["add", "table", "inet", NFT_TABLE]);
    run_cmd(
        "nft",
        &[
            "add",
            "set",
            "inet",
            NFT_TABLE,
            NFT_SET,
            "{ type ipv4_addr; flags timeout; }",
        ],
    );
    run_cmd(
        "nft",
        &[
            "add",
            "chain",
            "inet",
            NFT_TABLE,
            "input",
            "{ type filter hook input priority 0; policy accept; }",
        ],
    );
    run_cmd(
        "nft",
        &[
            "add", "rule", "inet", NFT_TABLE, "input", "ip", "saddr", "@ban", "drop",
        ],
    );
    log_msg("nft firewall initialized (independent of firewalld)");
}

fn nft_ban(ip: &str, timeout_secs: u64) {
    let hours = timeout_secs / 3600;
    let timeout_str = if hours > 0 {
        format!("{}h", hours)
    } else {
        format!("{}s", timeout_secs)
    };
    let element = format!("{{ {} timeout {} }}", ip, timeout_str);
    run_cmd(
        "nft",
        &["add", "element", "inet", NFT_TABLE, NFT_SET, &element],
    );
}

fn nft_unban(ip: &str) {
    let element = format!("{{ {} }}", ip);
    run_cmd(
        "nft",
        &["delete", "element", "inet", NFT_TABLE, NFT_SET, &element],
    );
}

fn nft_list_banned() -> Vec<String> {
    let output = run_cmd_output("nft", &["list", "set", "inet", NFT_TABLE, NFT_SET]);
    match output {
        Some(o) => {
            let mut ips = Vec::new();
            for line in o.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("elements = {") || trimmed.contains("timeout") {
                    for part in trimmed.split(',') {
                        let token = part
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .trim_end_matches('{')
                            .trim();
                        if token.parse::<IpAddr>().is_ok() && !is_whitelisted(token) {
                            ips.push(token.to_string());
                        }
                    }
                }
            }
            ips.sort();
            ips.dedup();
            ips
        }
        None => Vec::new(),
    }
}

fn nft_banned_count() -> usize {
    nft_list_banned().len()
}

fn migrate_v1_rules() {
    if let Some(rules) = run_cmd_output("firewall-cmd", &["--list-rich-rules"]) {
        let mut count = 0u32;
        for line in rules.lines() {
            let trimmed = line.trim();
            if trimmed.contains("rule family=\"ipv4\" source address=\"")
                && trimmed.ends_with("\" drop")
            {
                run_cmd("firewall-cmd", &["--remove-rich-rule", trimmed]);
                count += 1;
            }
        }
        if count > 0 {
            log_msg(&format!(
                "Migrated: removed {} old firewall-cmd rules",
                count
            ));
        }
    }
}

fn open_honeypot_ports() {
    let ports = get_honeypot_ports();
    for &port in &ports {
        run_cmd("firewall-cmd", &["--add-port", &format!("{}/tcp", port)]);
    }
    log_msg(&format!("Honeypot ports opened: {:?}", ports));
}

fn start_honeypot(tx: &mpsc::Sender<String>) {
    let ports = get_honeypot_ports();
    for port in ports {
        let tx = tx.clone();
        let _ = thread::Builder::new()
            .name(format!("honeypot-{}", port))
            .spawn(
                move || match std::net::TcpListener::bind(("0.0.0.0", port)) {
                    Ok(listener) => {
                        listener.set_nonblocking(true).ok();
                        loop {
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    if let Ok(addr) = stream.peer_addr() {
                                        let ip = addr.ip().to_string();
                                        if !is_whitelisted(&ip) {
                                            let _ = tx.send(ip);
                                        }
                                    }
                                    drop(stream);
                                }
                                Err(_) => {
                                    thread::sleep(Duration::from_millis(100));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::AddrInUse {
                            log_msg(&format!("Honeypot port {} bind failed: {}", port, e));
                        }
                    }
                },
            );
    }
}

fn tail_log(path: &str, offset: &mut u64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return lines,
    };
    let len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return lines,
    };
    if len < *offset {
        *offset = 0;
    }
    if *offset >= len {
        return lines;
    }
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return lines;
    }
    let mut reader = std::io::BufReader::new(file);
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = buf.trim().to_string();
                if !trimmed.is_empty() {
                    lines.push(trimmed);
                }
            }
            Err(_) => break,
        }
    }
    if let Ok(pos) = reader.stream_position() {
        *offset = pos;
    }
    lines
}

fn analyze_ssh(line: &str) -> Option<String> {
    if !line.contains("Failed password")
        && !line.contains("Invalid user")
        && !line.contains("authentication failure")
    {
        return None;
    }
    let from_pos = line.rfind(" from ")?;
    let rest = &line[from_pos + 6..];
    let end = rest.find(' ').unwrap_or(rest.len());
    let ip_str = &rest[..end];
    if ip_str.parse::<IpAddr>().is_ok() && !is_whitelisted(ip_str) {
        Some(ip_str.to_string())
    } else {
        None
    }
}

fn parse_nginx_line(line: &str) -> Option<(String, String, u16)> {
    let ip = line.split(' ').next()?;
    if ip.parse::<IpAddr>().is_err() {
        return None;
    }
    let first_quote = line.find('"')?;
    let rest = &line[first_quote + 1..];
    let second_quote = rest.find('"')?;
    let request = &rest[..second_quote];
    let after = &rest[second_quote + 1..];
    let status_str = after.trim_start().split(' ').next()?;
    let status: u16 = status_str.parse().ok()?;
    let req_parts: Vec<&str> = request.splitn(3, ' ').collect();
    let path = if req_parts.len() >= 2 {
        req_parts[1].to_string()
    } else {
        request.to_string()
    };
    Some((ip.to_string(), path, status))
}

fn is_attack_request(path: &str) -> bool {
    let lower = path.to_lowercase();
    WEB_ATTACK_PATTERNS.iter().any(|p| lower.contains(p))
}

fn hash_file(path: &str) -> Option<String> {
    run_cmd_output("sha256sum", &[path])
        .and_then(|out| out.split_whitespace().next().map(|s| s.to_string()))
}

fn check_file_integrity(state: &mut GuardState) {
    let first_run = state.file_hashes.is_empty();
    for &path in MONITORED_FILES {
        let current = match hash_file(path) {
            Some(h) => h,
            None => continue,
        };
        if first_run {
            state.file_hashes.insert(path.to_string(), current);
        } else if let Some(previous) = state.file_hashes.get(path) {
            if &current != previous {
                log_msg(&format!("FILE INTEGRITY ALERT: {} changed!", path));
                state.file_hashes.insert(path.to_string(), current);
            }
        } else {
            state.file_hashes.insert(path.to_string(), current);
        }
    }
}

fn check_unexpected_ports() -> Vec<u16> {
    let output = run_cmd_output("ss", &["-tlnp"]);
    match output {
        Some(o) => {
            let mut ports = Vec::new();
            for line in o.lines().skip(1) {
                let parts: Vec<&str> = line.splitn(6, ' ').collect();
                if let Some(local) = parts.get(4) {
                    if let Some(port_str) = local.rsplit(':').next() {
                        if let Ok(port) = port_str.parse::<u16>() {
                            let honeypot = get_honeypot_ports();
                            let is_ok = EXPECTED_PORTS.contains(&port)
                                || port == 9111
                                || honeypot.contains(&port);
                            if !is_ok {
                                let is_local =
                                    local.starts_with("127.0.0.1") || local.starts_with("[::1]");
                                if !is_local {
                                    ports.push(port);
                                }
                            }
                        }
                    }
                }
            }
            ports
        }
        None => Vec::new(),
    }
}

fn check_connection_flood() -> Vec<(String, usize)> {
    let output = run_cmd_output("ss", &["-tn"]);
    let mut counts: HashMap<String, usize> = HashMap::new();
    if let Some(o) = output {
        for line in o.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                if let Some(peer) = parts.get(4) {
                    if let Some(colon_pos) = peer.rfind(':') {
                        let ip = &peer[..colon_pos];
                        if !is_whitelisted(ip) && ip.parse::<IpAddr>().is_ok() {
                            *counts.entry(ip.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c > FLOOD_THRESHOLD)
        .collect()
}

fn run_daemon() {
    if let Ok(pid) = fs::read_to_string(PID_FILE) {
        if let Ok(old_pid) = pid.trim().parse::<u32>() {
            if run_cmd("kill", &["-0", &old_pid.to_string()]) {
                eprintln!("epicode-guard already running (pid {})", old_pid);
                std::process::exit(1);
            }
        }
    }
    let _ = fs::create_dir_all("/var/lib/epicode-guard");
    let _ = fs::create_dir_all("/var/log/epicode-guard");
    let _ = fs::write(PID_FILE, std::process::id().to_string());

    log_msg(&format!("=== epicode-guard v{} starting ===", VERSION));
    nft_init();
    open_honeypot_ports();

    let (tx, rx) = mpsc::channel();
    start_honeypot(&tx);

    let mut state = GuardState::load();
    let now = now_ts();
    state.reapply_bans(now);
    log_msg(&format!(
        "State loaded: {} tracked IPs, {} historical bans",
        state.ips.len(),
        state.total_bans
    ));

    let mut cycle = 0u64;
    loop {
        let now = now_ts();

        while let Ok(ip) = rx.try_recv() {
            state.record(&ip, HONEYPOT_SCORE, "honeypot", now);
            state.total_honeypot += 1;
            log_msg(&format!("HONEYPOT HIT: {} on decoy port", ip));
            epicode_remember_honeypot(&ip, 0);
        }

        let ssh_lines = tail_log(SSH_LOG, &mut state.ssh_offset);
        let mut ssh_count = 0u32;
        for line in &ssh_lines {
            if let Some(ip) = analyze_ssh(line) {
                state.record(&ip, 3, "ssh", now);
                ssh_count += 1;
            }
        }
        if ssh_count > 0 {
            log_msg(&format!("SSH: {} new failed auth attempts", ssh_count));
        }

        let web_lines = tail_log(WEB_LOG, &mut state.web_offset);
        let mut web_attacks = 0u32;
        let mut web_scans = 0u32;
        for line in &web_lines {
            if let Some((ip, path, status)) = parse_nginx_line(line) {
                if is_whitelisted(&ip) {
                    continue;
                }
                if is_attack_request(&path) {
                    state.record(&ip, WEB_ATTACK_SCORE, "web_attack", now);
                    web_attacks += 1;
                } else if status == 404 || status == 403 {
                    state.record(&ip, WEB_SCAN_SCORE, "web_scan", now);
                    web_scans += 1;
                }
            }
        }
        if web_attacks > 0 || web_scans > 50 {
            log_msg(&format!(
                "WEB: {} attacks, {} scan-404s this cycle",
                web_attacks, web_scans
            ));
        }

        if cycle % (FILE_CHECK_INTERVAL_SECS / CHECK_INTERVAL_SECS) == 0 {
            check_file_integrity(&mut state);
            state.last_file_check = now;
        }

        if cycle % 30 == 0 {
            let unexpected = check_unexpected_ports();
            if !unexpected.is_empty() && unexpected != state.last_ports {
                log_msg(&format!(
                    "PORT ALERT: unexpected listening: {:?}",
                    unexpected
                ));
            }
            state.last_ports = unexpected;
        }

        if cycle % 6 == 0 {
            let floods = check_connection_flood();
            for (ip, count) in &floods {
                let entry = state.ips.entry(ip.clone()).or_default();
                if entry.banned_until == 0 {
                    log_msg(&format!(
                        "FLOOD: {} has {} concurrent connections",
                        ip, count
                    ));
                    state.record(ip, 5, "flood", now);
                }
            }
        }

        if now - state.last_decay > DECAY_INTERVAL_SECS as i64 {
            state.decay(now);
            state.last_decay = now;
        }

        state.process_bans(now);

        state.save();

        if cycle % 60 == 0 && cycle > 0 {
            log_msg(&format!(
                "Stats: tracked={} nft_banned={} total_bans={} attacks={} honeypot={}",
                state.ips.len(),
                nft_banned_count(),
                state.total_bans,
                state.total_attacks,
                state.total_honeypot
            ));
        }

        cycle += 1;
        std::thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
    }
}

fn cmd_status() {
    println!("=== epicode-guard v{} Status ===\n", VERSION);
    let state = GuardState::load();
    let banned = nft_banned_count();
    let now = now_ts();

    let uptime = if state.start_time > 0 {
        let secs = now - state.start_time;
        format!(
            "{}d {}h {}m",
            secs / 86400,
            (secs % 86400) / 3600,
            (secs % 3600) / 60
        )
    } else {
        "unknown".to_string()
    };

    println!("Uptime:            {}", uptime);
    println!("Tracked IPs:       {}", state.ips.len());
    println!("Currently Banned:  {}", banned);
    println!("Total Bans:        {}", state.total_bans);
    println!("Total Attacks:     {}", state.total_attacks);
    println!("Honeypot Hits:     {}", state.total_honeypot);
    println!();

    let active: Vec<_> = state
        .ips
        .iter()
        .filter(|(_, e)| e.banned_until > now)
        .collect();
    if !active.is_empty() {
        println!("--- Active Bans (top 20 by score) ---");
        let mut sorted = active;
        sorted.sort_by(|a, b| b.1.score.cmp(&a.1.score));
        for (ip, entry) in sorted.iter().take(20) {
            let remain = (entry.banned_until - now) / 60;
            let tags = if entry.honeypot_hits > 0 {
                "HONEYPOT"
            } else if entry.web_attacks > 0 {
                "WEB"
            } else {
                "SSH"
            };
            println!(
                "  {:<18} score={:<4} {} remain={}m",
                ip, entry.score, tags, remain
            );
        }
    }

    let mut suspects: Vec<_> = state
        .ips
        .iter()
        .filter(|(_, e)| e.banned_until == 0 && e.score > 3)
        .collect();
    suspects.sort_by(|a, b| b.1.score.cmp(&a.1.score));
    if !suspects.is_empty() {
        println!("\n--- Suspicious IPs (score > 3, top 10) ---");
        for (ip, entry) in suspects.iter().take(10) {
            println!(
                "  {:<18} score={} ssh={} web={}",
                ip, entry.score, entry.ssh_fails, entry.web_attacks
            );
        }
    }
    println!();
}

fn cmd_ban(ip: &str) {
    if ip.parse::<IpAddr>().is_err() {
        eprintln!("Invalid IP: {}", ip);
        return;
    }
    nft_ban(ip, BAN_TIMEOUT_SECS);
    let mut state = GuardState::load();
    let entry = state.ips.entry(ip.to_string()).or_default();
    entry.banned_until = now_ts() + BAN_TIMEOUT_SECS as i64;
    entry.score = BAN_THRESHOLD;
    state.total_bans += 1;
    state.save();
    log_msg(&format!(
        "Manual ban: {} for {}h",
        ip,
        BAN_TIMEOUT_SECS / 3600
    ));
    println!("Banned {} for {} hours", ip, BAN_TIMEOUT_SECS / 3600);
}

fn cmd_unban(ip: &str) {
    nft_unban(ip);
    let mut state = GuardState::load();
    state.ips.remove(ip);
    state.save();
    log_msg(&format!("Manual unban: {}", ip));
    println!("Unbanned {}", ip);
}

fn cmd_check() {
    println!("=== File Integrity Check ===\n");
    let mut state = GuardState::load();
    let first_run = state.file_hashes.is_empty();
    for &path in MONITORED_FILES {
        match hash_file(path) {
            Some(current) => {
                if first_run {
                    println!("  {} -> {} (baseline)", path, &current[..16]);
                    state.file_hashes.insert(path.to_string(), current);
                } else if let Some(previous) = state.file_hashes.get(path) {
                    if &current == previous {
                        println!("  {} -> OK", path);
                    } else {
                        println!("  {} -> CHANGED!", path);
                    }
                } else {
                    println!("  {} -> {} (new)", path, &current[..16]);
                    state.file_hashes.insert(path.to_string(), current);
                }
            }
            None => println!("  {} -> NOT FOUND", path),
        }
    }
    state.save();
    println!();
    let unexpected = check_unexpected_ports();
    println!(
        "Unexpected ports: {}",
        if unexpected.is_empty() {
            "none".to_string()
        } else {
            format!("{:?}", unexpected)
        }
    );
    let floods = check_connection_flood();
    if floods.is_empty() {
        println!("Connection flood: none");
    } else {
        println!("Connection flood:");
        for (ip, count) in floods {
            println!("  {} -> {} connections", ip, count);
        }
    }
    println!();
}

fn cmd_nft() {
    println!("=== nft Table: {} ===\n", NFT_TABLE);
    if let Some(output) = run_cmd_output("nft", &["list", "table", "inet", NFT_TABLE]) {
        println!("{}", output);
    } else {
        println!("Table not found");
    }
}

fn cmd_help() {
    println!("epicode-guard v{} - Server Auto-Defense Daemon", VERSION);
    println!();
    println!("Usage:");
    println!("  epicode-guard            Run as daemon");
    println!("  epicode-guard status     Show security status");
    println!("  epicode-guard ban <ip>   Manual ban IP");
    println!("  epicode-guard unban <ip> Manual unban IP");
    println!("  epicode-guard check      File integrity + ports + flood");
    println!("  epicode-guard nft        Show nft table");
    println!("  epicode-guard help       Show this help");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("status") => cmd_status(),
        Some("ban") => {
            if let Some(ip) = args.get(2) {
                cmd_ban(ip);
            } else {
                eprintln!("Usage: epicode-guard ban <ip>");
            }
        }
        Some("unban") => {
            if let Some(ip) = args.get(2) {
                cmd_unban(ip);
            } else {
                eprintln!("Usage: epicode-guard unban <ip>");
            }
        }
        Some("check") => cmd_check(),
        Some("nft") => cmd_nft(),
        Some("help") | Some("--help") | Some("-h") => cmd_help(),
        None | Some("daemon") => run_daemon(),
        _ => cmd_help(),
    }
}

#[cfg(windows)]
mod about;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod auth;
mod autostart;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod history;
#[cfg(windows)]
mod net;
#[cfg(windows)]
mod server;
#[cfg(windows)]
mod splash;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("serve");

    match cmd {
        "--help" | "-h" | "help" => banner(),
        "--install-autostart" => run_install_autostart(),
        "--uninstall-autostart" => run_uninstall_autostart(),
        "serve" => {
            let no_open = args.iter().skip(2).any(|a| a == "--no-open");
            run_serve(no_open);
        }
        "list" => run_list(),
        "set" => run_set(args.get(2).map(String::as_str)),
        "share" => run_share(),
        "token" => run_token(&args),
        "setup" | "--setup" => run_setup(),
        // Bare `audioremote` → serve, honor --no-open if given as first arg
        // (e.g. `audioremote --no-open`).
        _ if cmd == "serve" || cmd.starts_with("--") || cmd.is_empty() => {
            let no_open = args.iter().skip(1).any(|a| a == "--no-open");
            run_serve(no_open);
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            eprintln!();
            banner();
            std::process::exit(2);
        }
    }
}

fn banner() {
    println!(
        "audioremote v{} (Windows 11 host agent)",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Subcommands:");
    println!("  audioremote               start the HTTP server (default)");
    println!("  audioremote serve         same as above (explicit)");
    println!("  audioremote serve --no-open   skip auto-opening the browser (for autostart)");
    println!("  audioremote setup         interactive config wizard (bind / token / sort)");
    println!("  audioremote list          list playback endpoints + current defaults");
    println!("  audioremote set <id>      switch default (Console/Multimedia/Communications)");
    println!("  audioremote share         print the LAN URLs with the token in full");
    println!(
        "  audioremote token ...     manage LAN tokens (list [--show] / add <name> / revoke <name|token>)"
    );
    println!("  audioremote --install-autostart     start server at logon (HKCU Run)");
    println!("  audioremote --uninstall-autostart  remove the AudioRemote logon entry");
    println!("  audioremote --help        show this help");
    println!();
    println!("Repository: {}", env!("CARGO_PKG_REPOSITORY"));
}

fn run_install_autostart() {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("[fatal] cannot determine current executable: {e}");
            std::process::exit(1);
        }
    };
    let command = match autostart::install(&exe) {
        Ok(command) => command,
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("[unsupported] {e}");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("[fatal] cannot install autostart: {e}");
            std::process::exit(1);
        }
    };

    println!("AudioRemote autostart installed.");
    println!(r"  registry: HKCU\{}", autostart::RUN_KEY_PATH);
    println!("  value:    {}", autostart::VALUE_NAME);
    println!("  command:  {command}");
    println!("  note:     run this command again if the exe is moved.");
}

fn run_uninstall_autostart() {
    match autostart::uninstall() {
        Ok(()) => println!("AudioRemote autostart removed (if it was registered)."),
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            eprintln!("[unsupported] {e}");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("[fatal] cannot uninstall autostart: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn run_serve(no_open: bool) {
    use std::sync::Arc;

    let config_path = config::default_config_path();
    let history_path = history::default_history_path();

    let (cfg, generated) = match config::load_or_init(&config_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!(
                "[fatal] cannot load config at {}: {}",
                config_path.display(),
                e
            );
            std::process::exit(1);
        }
    };

    let history_state = match history::load(&history_path) {
        Ok(h) => h,
        Err(e) => {
            eprintln!(
                "[warn] cannot load history at {}: {} (starting empty)",
                history_path.display(),
                e
            );
            history::History::default()
        }
    };

    // Resolve bind + port before the runtime starts: a hand-edited `bind = "::1"`
    // or `port = 0` used to surface as an opaque parse failure (or, worse, an
    // ephemeral port the banner and share URLs then reported wrong).
    let addr = match cfg.socket_addr() {
        Ok(addr) => addr,
        Err(reason) => {
            eprintln!(
                "[fatal] invalid [server] settings in {}: {reason}",
                config_path.display()
            );
            std::process::exit(1);
        }
    };

    let host_url = net::build_host_url(&cfg);
    let share_entries = net::build_share_entries(&cfg, cfg.share_token());
    print_startup_banner(
        &cfg,
        &config_path,
        generated,
        &host_url,
        &share_entries,
        no_open,
    );

    // Auto-open the loopback URL in the default browser on every manual `serve`.
    // Suppress with `--no-open` (used by the autostart entry so logon doesn't
    // pop a browser tab).
    if !no_open {
        open_url(&host_url);
    }

    let allowed_hosts = net::build_allowed_hosts(&cfg);
    // Tokens are the one setting that must not be a startup snapshot: the CLI
    // edits config.toml from another process, and a revoke has to take effect
    // without a restart.
    let auth_state = auth::AuthState::new(config_path.clone(), cfg.auth.clone());
    let state = server::AppState {
        config: Arc::new(cfg),
        auth: Arc::new(auth_state),
        audio: server::AudioGate::new(),
        history: Arc::new(tokio::sync::Mutex::new(history_state)),
        history_path: Arc::new(history_path),
        allowed_hosts: Arc::new(allowed_hosts),
    };

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("[fatal] tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    if let Err(e) = rt.block_on(server::serve(state, addr)) {
        eprintln!("[fatal] server: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn print_startup_banner(
    cfg: &config::Config,
    config_path: &std::path::Path,
    generated: bool,
    host_url: &str,
    share_entries: &[net::ShareEntry],
    no_open: bool,
) {
    // Splash art (ANSI colored). Silently downgrades to plain when NO_COLOR is set.
    print!("{}", splash::render());
    println!();
    println!("  config      {}", config_path.display());
    println!("  bind        {}:{}", cfg.server.bind, cfg.server.port);
    if generated {
        if let Some(tok) = cfg.share_token() {
            println!("  token       {tok}   <-- first-run token (saved; not shown again)");
        }
    } else {
        let active = cfg.auth.active_count();
        println!("  token       {active} active   (audioremote token list to view/manage)");
    }
    println!(
        "  device_sort {}",
        match cfg.audio.device_sort {
            config::SortPolicy::State => "state",
            config::SortPolicy::Name => "name",
            config::SortPolicy::Recent => "recent",
        }
    );

    let invalid_networks = cfg.invalid_networks();
    if !invalid_networks.is_empty() {
        println!();
        println!("  [WARNING] allowed_networks contains entries that are not valid networks:");
        for entry in &invalid_networks {
            println!("              {entry:?}");
        }
        println!("            They match nothing, so every LAN client outside the remaining");
        println!("            entries is refused. Use CIDR (\"203.0.113.0/24\") or a bare");
        println!("            address (\"203.0.113.20\").");
    }

    if cfg.lan_exposed() {
        if !cfg.auth.require_token {
            println!();
            println!("  [WARNING] require_token = false while bound to the LAN — the API is");
            println!("            OPEN to every machine that can reach this host. Set");
            println!("            require_token = true (or bind = 127.0.0.1) unless intentional.");
        }
        if generated {
            println!();
            println!("  [firewall] Windows may prompt to allow audioremote on the LAN — pick");
            println!("             \"Private networks\" the first time.");
        }
    } else {
        println!();
        println!(
            "  [note] LAN is DISABLED (bind = {}). This host will not answer other",
            cfg.server.bind
        );
        println!("         machines. Run `audioremote setup` to re-enable LAN.");
    }

    println!();
    println!("  Open on this host           :");
    println!("    {host_url}");

    if !share_entries.is_empty() {
        println!();
        println!("  Open on your other machine (guest Win11 / phone / VM) - token embedded:");
        for e in share_entries {
            let tag = if e.virtual_iface {
                "  [virtual switch]"
            } else {
                ""
            };
            // The token is masked on every start except the very first one. This
            // console is long-lived: it survives in scrollback, screen shares,
            // recordings and redirected logs, and it is re-printed at every logon
            // by the autostart entry. `audioremote share` prints it in full when
            // it is actually needed.
            let url = if generated {
                e.url.clone()
            } else {
                mask_share_url(&e.url)
            };
            println!("    {}   ({}){}", url, e.interface, tag);
        }
        if !generated {
            println!();
            println!("    (token masked - run `audioremote share` for the full URLs)");
        }
    }

    if !no_open {
        println!();
        println!("  (Opening the host URL in your browser now …)");
    }

    println!();
    println!("  Ctrl+C to stop.");
    println!();
}

/// Shorten a token for display: `ar_live_1a2b…9f8e`. Enough to tell two entries
/// apart in `token list`, not enough to authenticate with.
#[cfg(windows)]
fn mask_token(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 16 {
        // Too short to reveal a prefix and a suffix without giving away most of
        // it — a hand-added token can be any length.
        return "…".to_string();
    }
    let head: String = chars[..12].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

/// Mask the `#t=<token>` fragment of a share URL, leaving the address readable.
#[cfg(windows)]
fn mask_share_url(url: &str) -> String {
    match url.split_once("#t=") {
        Some((base, token)) => format!("{base}#t={}", mask_token(token)),
        None => url.to_string(),
    }
}

/// Print the LAN URLs with the token in full. Explicit command = explicit
/// consent to put the token on screen, which the startup banner no longer does.
#[cfg(windows)]
fn run_share() {
    let path = config::default_config_path();
    let (cfg, _) = match config::load_or_init(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot load config: {e}");
            std::process::exit(1);
        }
    };

    if !cfg.lan_exposed() {
        println!(
            "LAN is disabled (bind = {}). Run `audioremote setup` to enable it.",
            cfg.server.bind
        );
        return;
    }
    let entries = net::build_share_entries(&cfg, cfg.share_token());
    if entries.is_empty() {
        println!("No LAN address found on this host, so there is nothing to share yet.");
        return;
    }

    println!("Open on your other machine (guest Win11 / phone / VM).");
    println!("These URLs contain a live token - treat them like a password:");
    println!();
    for e in entries {
        let tag = if e.virtual_iface {
            "  [virtual switch]"
        } else {
            ""
        };
        println!("  {}   ({}){}", e.url, e.interface, tag);
    }
}

/// Fire-and-forget open of `url` in the default browser (Windows only).
#[cfg(windows)]
fn open_url(url: &str) {
    // Strip trailing "  (label)" the printer adds for readability.
    let clean = url.split_whitespace().next().unwrap_or(url);
    // `cmd /C start "" <url>` picks the OS default handler for http:// URLs.
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", clean])
        .spawn();
}

#[cfg(windows)]
fn run_setup() {
    use std::io::Write;

    let path = config::default_config_path();
    let (mut cfg, _) = match config::load_or_init(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot load config: {e}");
            std::process::exit(1);
        }
    };

    println!("audioremote setup — current config: {}", path.display());
    println!();

    // 1. LAN mode
    let currently_lan = cfg.lan_exposed();
    println!(
        "1) Network mode  (currently: {})",
        if currently_lan {
            "LAN exposed (0.0.0.0)"
        } else {
            "host-only (127.0.0.1)"
        }
    );
    print!(
        "   Switch to {}? [y/N]: ",
        if currently_lan {
            "host-only"
        } else {
            "LAN exposed"
        }
    );
    std::io::stdout().flush().ok();
    if yes() {
        cfg.server.bind = if currently_lan {
            "127.0.0.1".to_string()
        } else {
            "0.0.0.0".to_string()
        };
        println!("   -> bind = {}", cfg.server.bind);
    }
    println!();

    // 2. Reissue token (revokes ALL current tokens, issues one fresh "default")
    println!("2) Reissue auth token? (revokes ALL current tokens)");
    print!("   Old clients will need the new URL. [y/N]: ");
    std::io::stdout().flush().ok();
    if yes() {
        for t in &mut cfg.auth.tokens {
            t.revoked = true;
        }
        let token = config::add_named_token(&mut cfg, "default");
        println!("   -> new token: {token}");
    }
    println!();

    // 3. Sort policy
    println!("3) Device sort");
    println!("   1) state (active first)      2) name          3) recent (most recently used)");
    print!(
        "   Choose [current: {}] (enter to keep): ",
        match cfg.audio.device_sort {
            config::SortPolicy::State => "1) state",
            config::SortPolicy::Name => "2) name",
            config::SortPolicy::Recent => "3) recent",
        }
    );
    std::io::stdout().flush().ok();
    let line = read_line();
    match line.trim() {
        "1" => cfg.audio.device_sort = config::SortPolicy::State,
        "2" => cfg.audio.device_sort = config::SortPolicy::Name,
        "3" => cfg.audio.device_sort = config::SortPolicy::Recent,
        _ => {}
    }
    println!("   -> device_sort = {:?}", cfg.audio.device_sort);
    println!();

    // Port
    println!("4) Port  (currently: {})", cfg.server.port);
    print!("   Change? [y/N]: ");
    std::io::stdout().flush().ok();
    if yes() {
        print!("   New port (1024–65535): ");
        std::io::stdout().flush().ok();
        if let Ok(n) = read_line().trim().parse::<u16>() {
            if n >= 1024 {
                cfg.server.port = n;
                println!("   -> port = {n}");
            } else {
                println!("   port must be >= 1024; keeping {}", cfg.server.port);
            }
        }
    }
    println!();

    if let Err(e) = config::save(&path, &cfg) {
        eprintln!("failed to save config: {e}");
        std::process::exit(1);
    }
    println!("Saved. Restart audioremote for changes to take effect.");
}

#[cfg(windows)]
fn yes() -> bool {
    matches!(
        read_line().trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    )
}

#[cfg(windows)]
fn read_line() -> String {
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s
}

#[cfg(windows)]
fn run_list() {
    match audio::list_devices() {
        Ok(devs) => {
            println!("{:<3} {:<9} {:<3} {:<40} id", "#", "state", "def", "name");
            println!("{}", "-".repeat(120));
            for (i, d) in devs.iter().enumerate() {
                let def = default_marker(d);
                println!(
                    "{:<3} {:<9} {:<3} {:<40} {}",
                    i,
                    d.state.as_str(),
                    def,
                    truncate(&d.name, 40),
                    d.id
                );
            }
        }
        Err(e) => {
            eprintln!("list failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn default_marker(d: &audio::AudioDevice) -> String {
    let mut s = String::new();
    if d.is_default_console {
        s.push('C');
    }
    if d.is_default_multimedia {
        s.push('M');
    }
    if d.is_default_communications {
        s.push('X');
    }
    if s.is_empty() {
        s.push('-');
    }
    s
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(windows)]
fn run_set(id: Option<&str>) {
    let Some(id) = id else {
        eprintln!("usage: audioremote set <device_id>");
        std::process::exit(2);
    };
    match audio::set_default(id) {
        Ok(()) => println!("OK: default endpoint switched to {id}"),
        Err(e) => {
            eprintln!("set failed: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(windows)]
fn run_token(args: &[String]) {
    let sub = args.get(2).map(String::as_str).unwrap_or("list");
    let path = config::default_config_path();
    let (mut cfg, _) = match config::load_or_init(&path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("cannot load config: {e}");
            std::process::exit(1);
        }
    };

    match sub {
        "list" => {
            // Masked by default: this listing is the one command people run over
            // a shared screen, and the values are long-lived credentials.
            let reveal = args.iter().skip(3).any(|a| a == "--show");
            println!("{:<20} {:<8} {:<12} token", "name", "state", "created");
            println!("{}", "-".repeat(96));
            for t in &cfg.auth.tokens {
                let st = if t.revoked { "revoked" } else { "active" };
                let created = if t.created_at == 0 {
                    "-".to_string()
                } else {
                    t.created_at.to_string()
                };
                let shown = if reveal {
                    t.token.clone()
                } else {
                    mask_token(&t.token)
                };
                println!(
                    "{:<20} {:<8} {:<12} {}",
                    truncate(&t.name, 20),
                    st,
                    created,
                    shown
                );
            }
            if !reveal {
                println!();
                println!("tokens are masked - `audioremote token list --show` prints them in full");
            }
        }
        "add" => {
            let Some(name) = args.get(3) else {
                eprintln!("usage: audioremote token add <name>");
                std::process::exit(2);
            };
            let token = config::add_named_token(&mut cfg, name);
            if let Err(e) = config::save(&path, &cfg) {
                eprintln!("failed to save config: {e}");
                std::process::exit(1);
            }
            println!("added token '{name}':");
            println!("  {token}");
            println!();
            println!("A running server picks this up within a second - no restart needed.");
        }
        "revoke" => {
            let Some(target) = args.get(3) else {
                eprintln!("usage: audioremote token revoke <name|token>");
                std::process::exit(2);
            };
            let n = config::revoke_token(&mut cfg, target);
            if n == 0 {
                eprintln!("no active token matched '{target}'");
                std::process::exit(1);
            }
            // Never leave the server with zero usable tokens: reissue if the
            // last active token was just revoked.
            let reissued = if !cfg.auth.tokens.iter().any(|t| !t.revoked) {
                Some(config::add_named_token(&mut cfg, "default"))
            } else {
                None
            };
            // Report only after the write succeeds: "revoked" printed ahead of a
            // failed save is exactly the lie that makes a leaked token look dead.
            if let Err(e) = config::save(&path, &cfg) {
                eprintln!("failed to save config: {e}");
                eprintln!("nothing was revoked - the token(s) are still valid");
                std::process::exit(1);
            }
            match reissued {
                Some(token) => {
                    println!(
                        "revoked {n}; that was the last active token — issued a new 'default':"
                    );
                    println!("  {token}");
                }
                None => println!("revoked {n} token(s) matching '{target}'"),
            }
            println!();
            println!("A running server stops accepting them within a second - no restart needed.");
        }
        other => {
            eprintln!("unknown token subcommand: {other}");
            eprintln!(
                "usage: audioremote token [list [--show] | add <name> | revoke <name|token>]"
            );
            std::process::exit(2);
        }
    }
}

// Non-Windows stubs. Signatures must match the calls in `main` exactly — the
// build for these targets is not covered by CI, so a mismatch here is only
// discovered by whoever tries to `cargo check` on macOS or Linux.
#[cfg(not(windows))]
fn run_serve(_no_open: bool) {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_list() {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_share() {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_set(_: Option<&str>) {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_setup() {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_token(_: &[String]) {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}

#[cfg(all(test, windows))]
mod tests {
    use super::{mask_share_url, mask_token, truncate};

    #[test]
    fn mask_token_keeps_the_prefix_and_hides_the_secret() {
        let token = "ar_live_0123456789abcdef0123456789abcdef0123456789abcdef";
        let masked = mask_token(token);
        assert_eq!(masked, "ar_live_0123…cdef");
        assert!(!masked.contains("456789abcdef0123"));
        assert!(masked.len() < token.len());
    }

    #[test]
    fn mask_token_refuses_to_reveal_short_values() {
        for short in ["", "ar_live_", "ar_live_abcd", "0123456789abcdef"] {
            assert_eq!(mask_token(short), "…", "{short:?}");
        }
    }

    #[test]
    fn mask_share_url_hides_only_the_fragment() {
        let url = "http://203.0.113.5:17650/#t=ar_live_0123456789abcdef0123456789abcdef";
        let masked = mask_share_url(url);
        assert!(masked.starts_with("http://203.0.113.5:17650/#t=ar_live_0123…"));
        assert!(!masked.contains("89abcdef0123456789"));
        // A URL without a token fragment is passed through untouched.
        assert_eq!(
            mask_share_url("http://203.0.113.5:17650/"),
            "http://203.0.113.5:17650/"
        );
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        assert_eq!(truncate("スピーカー", 40), "スピーカー");
        assert_eq!(truncate("スピーカー", 3), "スピ…");
        assert_eq!(truncate("abc", 3), "abc");
    }
}

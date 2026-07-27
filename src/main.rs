#[cfg(windows)]
mod about;
mod autostart;
#[cfg(windows)]
mod audio;
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
    println!("audioremote v{} (Windows 11 host agent)", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Subcommands:");
    println!("  audioremote               start the HTTP server (default)");
    println!("  audioremote serve         same as above (explicit)");
    println!("  audioremote serve --no-open   skip auto-opening the browser (for autostart)");
    println!("  audioremote setup         interactive config wizard (bind / token / sort)");
    println!("  audioremote list          list playback endpoints + current defaults");
    println!("  audioremote set <id>      switch default (Console/Multimedia/Communications)");
    println!("  audioremote token ...     manage LAN tokens (list / add <name> / revoke <name|token>)");
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
            eprintln!("[fatal] cannot load config at {}: {}", config_path.display(), e);
            std::process::exit(1);
        }
    };

    let history_state = match history::load(&history_path) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[warn] cannot load history at {}: {} (starting empty)", history_path.display(), e);
            history::History::default()
        }
    };

    let host_url = net::build_host_url(&cfg);
    let share_entries = net::build_share_entries(&cfg);
    print_startup_banner(&cfg, &config_path, generated, &host_url, &share_entries, no_open);

    // Auto-open the loopback URL in the default browser on every manual `serve`.
    // Suppress with `--no-open` (used by the autostart entry so logon doesn't
    // pop a browser tab).
    if !no_open {
        open_url(&host_url);
    }

    let allowed_hosts = net::build_allowed_hosts(&cfg);
    let state = server::AppState {
        config: Arc::new(cfg),
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

    if let Err(e) = rt.block_on(server::serve(state)) {
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
        let active = cfg.auth.tokens.iter().filter(|t| !t.revoked).count();
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
        println!("  [note] LAN is DISABLED (bind = {}). This host will not answer other", cfg.server.bind);
        println!("         machines. Run `audioremote setup` to re-enable LAN.");
    }

    println!();
    println!("  Open on this host           :");
    println!("    {host_url}");

    if !share_entries.is_empty() {
        println!();
        println!("  Open on your other machine (guest Win11 / phone / VM) — token embedded:");
        for e in share_entries {
            let tag = if e.virtual_iface { "  [virtual switch]" } else { "" };
            println!("    {}   ({}){}", e.url, e.interface, tag);
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
        if currently_lan { "LAN exposed (0.0.0.0)" } else { "host-only (127.0.0.1)" }
    );
    print!("   Switch to {}? [y/N]: ", if currently_lan { "host-only" } else { "LAN exposed" });
    std::io::stdout().flush().ok();
    if yes() {
        cfg.server.bind = if currently_lan { "127.0.0.1".to_string() } else { "0.0.0.0".to_string() };
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
    print!("   Choose [current: {}] (enter to keep): ", match cfg.audio.device_sort {
        config::SortPolicy::State => "1) state",
        config::SortPolicy::Name => "2) name",
        config::SortPolicy::Recent => "3) recent",
    });
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
    matches!(read_line().trim().to_ascii_lowercase().as_str(), "y" | "yes")
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
            println!("{:<3} {:<9} {:<3} {:<40} {}", "#", "state", "def", "name", "id");
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
    if d.is_default_console { s.push('C'); }
    if d.is_default_multimedia { s.push('M'); }
    if d.is_default_communications { s.push('X'); }
    if s.is_empty() { s.push('-'); }
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
            println!("{:<20} {:<8} {:<12} {}", "name", "state", "created", "token");
            println!("{}", "-".repeat(96));
            for t in &cfg.auth.tokens {
                let st = if t.revoked { "revoked" } else { "active" };
                let created = if t.created_at == 0 {
                    "-".to_string()
                } else {
                    t.created_at.to_string()
                };
                println!(
                    "{:<20} {:<8} {:<12} {}",
                    truncate(&t.name, 20),
                    st,
                    created,
                    t.token
                );
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
            if !cfg.auth.tokens.iter().any(|t| !t.revoked) {
                let token = config::add_named_token(&mut cfg, "default");
                println!("revoked {n}; that was the last active token — issued a new 'default':");
                println!("  {token}");
            } else {
                println!("revoked {n} token(s) matching '{target}'");
            }
            if let Err(e) = config::save(&path, &cfg) {
                eprintln!("failed to save config: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown token subcommand: {other}");
            eprintln!("usage: audioremote token [list | add <name> | revoke <name|token>]");
            std::process::exit(2);
        }
    }
}

#[cfg(not(windows))]
fn run_serve() {
    eprintln!("audioremote only runs on Windows.");
    std::process::exit(1);
}
#[cfg(not(windows))]
fn run_list() {
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

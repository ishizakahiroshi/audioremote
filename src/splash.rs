//! Terminal startup splash. ASCII art of the app icon (speaker + waves +
//! wordmark) with ANSI colors matching the Web UI palette. Respects the
//! `NO_COLOR` environment variable (https://no-color.org/) — set it to any
//! value to suppress escape sequences.

pub fn render() -> String {
    let use_color = std::env::var_os("NO_COLOR").is_none();

    // ANSI 256-color approximations of the app palette.
    //   ink        -> 235 (dark grey / near-black)
    //   accent     -> 208 (orange)
    //   ink-soft   -> 244 (mid grey)
    let ink = code(use_color, "\x1b[38;5;235m");
    let orange = code(use_color, "\x1b[38;5;208m");
    let soft = code(use_color, "\x1b[38;5;244m");
    let bold = code(use_color, "\x1b[1m");
    let reset = code(use_color, "\x1b[0m");

    let version = env!("CARGO_PKG_VERSION");
    let build_id = env!("AUDIOREMOTE_BUILD_ID");

    format!(
        concat!(
            "\n",
            "  {bold}{ink}┏━━━━━━━━━┓{reset}\n",
            "  {bold}{ink}┃  ▓▓▓▓▓  ┃{reset}   {orange}⟩ ⟩ ⟩{reset}\n",
            "  {bold}{ink}┃  ▓ {orange}●●{ink} ▓  ┃{reset}   {orange}⟩ ⟩{reset}\n",
            "  {bold}{ink}┃  ▓▓▓▓▓  ┃{reset}   {orange}⟩{reset}\n",
            "  {bold}{ink}┗━━━━━━━━━┛{reset}\n",
            "\n",
            "  {bold}Audio{orange}Remote{reset}  {soft}·{reset}  v{version} ({build_id})  {soft}·  Windows 11 host agent{reset}\n",
            "  {soft}────────────────────────────────────────────────{reset}\n",
        ),
        bold = bold,
        ink = ink,
        orange = orange,
        soft = soft,
        reset = reset,
        version = version,
        build_id = build_id,
    )
}

fn code(use_color: bool, seq: &'static str) -> &'static str {
    if use_color {
        seq
    } else {
        ""
    }
}

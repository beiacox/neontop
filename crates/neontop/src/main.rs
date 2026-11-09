//! NeonTop: monitor de recursos del sistema con estética neón (interfaz en GPUI).
//!
//! Argumentos:
//!   --helper …     modo ayudante como root (lo lanza pkexec; sin interfaz)
//!   --demo         datos simulados (revisar temas y maquetación sin datos reales)
//!   --autostart    arranque con la sesión (puede quedarse en la bandeja)
//!   --tab <id>     pestaña inicial;  --theme <id>  tema (solo para esta ejecución)
//!   --lang <código> idioma (es, en, pt, fr, de) solo para esta ejecución

mod app;
mod autostart;
mod charts;
mod notify;
mod settings;
mod single;
mod state;
mod tabs;
mod tray;
mod ui;
mod web;

use gpui::Application;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `neontop --helper …`: modo ayudante lanzado por pkexec (sin interfaz)
    if args.first().map(String::as_str) == Some("--helper") {
        std::process::exit(neontop_core::helper::run(&args[1..]));
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("neontop {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let demo = args.iter().any(|a| a == "--demo");
    // una sola instancia (la demo puede convivir con la normal)
    if !demo && !single::claim() {
        return;
    }
    if !demo {
        autostart::migrate_legacy();
    }
    let autostarted = args.iter().any(|a| a == autostart::ARG);
    let tab = arg_value(&args, "--tab");
    let theme_override = arg_value(&args, "--theme");
    let lang_override = arg_value(&args, "--lang");

    Application::new().run(move |cx| {
        ui::theme::load_fonts(cx);
        ui::input::bind_keys(cx);
        let (st, tx) = state::init(cx, demo);
        {
            let s = st.read(cx);
            neontop_core::i18n::init(lang_override.as_deref().or(s.ui.lang.as_deref()));
            let theme = theme_override.clone().unwrap_or_else(|| s.ui.theme.clone());
            ui::theme::apply(cx, &theme, s.ui.glow, s.ui.scanlines);
        }
        if let Some(t) = tab.clone() {
            st.update(cx, |s, _| s.ui.tab = t);
        }
        let tray_ok = !demo && tray::create(tx.clone());
        if !demo {
            single::listen(tx.clone());
        }
        cx.on_window_closed(app::on_window_closed).detach();
        let start_hidden = autostarted && st.read(cx).cfg.get().start_hidden && tray_ok;
        if !start_hidden {
            app::show_main_window(cx);
        }
    });
}

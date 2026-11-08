// GUI app: no console window (stderr still works when redirected).
#![windows_subsystem = "windows"]
mod api;
mod assets;
mod auth;
mod backdrop;
mod anim;
mod home;
mod user;
mod http;
mod images;
mod library;
mod playlist;
mod artist;
mod login;
mod shell;
mod spotify;
mod spotify_assets;
mod theme;
mod titlebar;

use gpui::{
    App, AppContext, Application, Bounds, Context, Entity, IntoElement, ParentElement, Render,
    Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, size,
};

use assets::Assets;
use login::{LoggedIn, LoginView};
use shell::Shell;

fn load_fonts(cx: &mut App) {
    cx.text_system()
        .add_fonts(Assets::fonts())
        .expect("failed to load bundled fonts");

    // Prefer the real UI fonts from the user's local Spotify install.
    match spotify_assets::load_spotify_fonts() {
        Ok(fonts) => {
            let data = fonts.data.into_iter().map(Into::into).collect();
            match cx.text_system().add_fonts(data) {
                Ok(()) => theme::set_fonts(fonts.ui, fonts.title),
                Err(e) => eprintln!("could not register Spotify fonts: {e:#}"),
            }
        }
        Err(e) => eprintln!("Spotify fonts unavailable, using {}: {e:#}", theme::FALLBACK_FONT),
    }
}

enum Screen {
    Login(Entity<LoginView>),
    Main(Entity<Shell>),
}

struct Root {
    screen: Screen,
}

impl Root {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `SPOTIRUST_PREVIEW_SHELL=1` shows the logged-in layout without data (layout checks).
        if std::env::var_os("SPOTIRUST_PREVIEW_SHELL").is_some() {
            return Self { screen: Screen::Main(cx.new(|cx| Shell::new(None, cx))) };
        }
        if let Some(token) = auth::load_token() {
            let api = api::Api::new(token);
            return Self { screen: Screen::Main(cx.new(|cx| Shell::new(Some(api), cx))) };
        }
        let login = cx.new(|_| LoginView::new());
        cx.subscribe_in(&login, window, |this, _, ev: &LoggedIn, _window, cx| {
            let api = api::Api::new(ev.0.clone());
            this.screen = Screen::Main(cx.new(|cx| Shell::new(Some(api), cx)));
            cx.notify();
        })
        .detach();
        Self { screen: Screen::Login(login) }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Test harness hook: report the real render viewport so captures can match the reference.
        if let Some(path) = std::env::var_os("SPOTIRUST_VIEWPORT_FILE") {
            let vp = window.viewport_size();
            let _ = std::fs::write(
                path,
                format!("{} {} {}", f32::from(vp.width), f32::from(vp.height), window.scale_factor()),
            );
        }
        let child = match &self.screen {
            Screen::Login(v) => v.clone().into_any_element(),
            Screen::Main(v) => v.clone().into_any_element(),
        };
        div().size_full().child(child)
    }
}

/// Prints GPUI's own warnings/errors to stderr (it logs through the `log` crate).
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("[{}] {}: {}", r.level(), r.target(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    static LOGGER: StderrLog = StderrLog;
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Warn));
    Application::new().with_assets(Assets).run(|cx: &mut App| {
        load_fonts(cx);
        images::init(cx);

        let bounds = Bounds::centered(None, size(px(1551.2), px(831.2)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Spotirust".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                window_min_size: Some(size(px(800.), px(600.))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Root::new(window, cx)),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}

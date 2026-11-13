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
mod search;
mod audio_out;
mod player;
mod artist;
mod login;
mod shell;
mod spotify;
mod spotify_assets;
mod spotify_cache;
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
            return Self { screen: Self::main_screen(api::Api::new(token), window, cx) };
        }
        Self { screen: Self::login_screen(window, cx) }
    }

    /// The signed-in app; logging out swaps back to the login screen.
    fn main_screen(api: std::sync::Arc<api::Api>, window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let shell = cx.new(|cx| Shell::new(Some(api), cx));
        cx.subscribe_in(&shell, window, |this, _, _: &shell::LoggedOut, window, cx| {
            this.screen = Self::login_screen(window, cx);
            cx.notify();
        })
        .detach();
        Screen::Main(shell)
    }

    fn login_screen(window: &mut Window, cx: &mut Context<Self>) -> Screen {
        let login = cx.new(|_| LoginView::new());
        cx.subscribe_in(&login, window, |this, _, ev: &LoggedIn, window, cx| {
            this.screen = Self::main_screen(api::Api::new(ev.0.clone()), window, cx);
            cx.notify();
        })
        .detach();
        Screen::Login(login)
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
        spotify_cache::warm_up();

        // The reference client's default size, shrunk to fit the primary display (at 125% a
        // 1920x1080 screen is only 1536x864 logical) so the window always opens fully on screen.
        let mut want = size(px(1551.2), px(831.2));
        if let Some(display) = cx.primary_display() {
            let screen = display.bounds().size;
            want.width = want.width.min(screen.width - px(32.));
            want.height = want.height.min(screen.height - px(80.));
        }
        let bounds = Bounds::centered(None, want, cx);
        // Test hook: SPOTIRUST_OFFSCREEN=1 (test copies, moved off screen once open) doesn't bring
        // the app to the front.
        let offscreen = std::env::var_os("SPOTIRUST_OFFSCREEN").is_some();
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
        if !offscreen {
            cx.activate(true);
        }
    });
}

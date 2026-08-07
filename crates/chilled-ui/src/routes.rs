//! Client-side route map. Server-side, any extensionless /ui path serves
//! index.html, so every route here deep-links.

use dioxus::prelude::*;

use crate::ui::home::Home;
use crate::ui::login::Login;
use crate::ui::logs::Logs;
use crate::ui::mount::Mount;
use crate::ui::not_found::NotFound;
use crate::ui::profile::Profile;
use crate::ui::server_config::ServerConfig;
use crate::ui::setup::Setup;
use crate::ui::shell::Shell;
use crate::ui::users::Users;

#[derive(Routable, Clone, PartialEq)]
pub enum Route {
    #[layout(Shell)]
    #[route("/")]
    Home {},
    #[route("/mount/:name")]
    Mount { name: String },
    #[route("/login")]
    Login {},
    #[route("/setup")]
    Setup {},
    #[route("/profile")]
    Profile {},
    #[route("/server-config")]
    ServerConfig {},
    #[route("/users")]
    Users {},
    #[route("/logs")]
    Logs {},
    #[end_layout]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

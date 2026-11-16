//! Pages pushed on the navigation stack.

pub mod album;
pub mod browse;
pub mod group;
pub mod search;
pub mod track;
pub mod tracks;

use relm4::adw;

use crate::ctx::Route;

pub trait Page {
    fn nav(&self) -> &adw::NavigationPage;
    fn route(&self) -> &Route;
    /// Re-read shared state (current track, ratings, preferred versions…).
    fn refresh(&self) {}
    /// Search pages only: replace the query.
    fn set_query(&self, _q: &str) {}
}

/// Unique navigation tag for a route.
pub fn tag(route: &Route) -> String {
    match route {
        Route::Album(f) => format!("album:{f}"),
        Route::Track(f) => format!("track:{f}"),
        Route::Artist(n) => format!("artist:{n}"),
        Route::Label(n) => format!("label:{n}"),
        Route::Composer(n) => format!("composer:{n}"),
        Route::Search(_) => "search".into(),
    }
}

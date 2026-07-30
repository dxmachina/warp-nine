use warpui::async_assert_eq;
use warpui::integration::AssertionCallback;

use crate::integration_testing::view_getters::single_terminal_view;

pub fn assert_secret_tooltip_open(open: bool) -> AssertionCallback {
    Box::new(move |app, window_id| {
        let terminal_view = single_terminal_view(app, window_id);
        let error_message = if open {
            "The secret tooltip should be open"
        } else {
            "The secret tooltip should not be open"
        };
        terminal_view.read(app, |view, _ctx| {
            async_assert_eq!(view.is_secret_tooltip_open(), open, "{}", error_message)
        })
    })
}

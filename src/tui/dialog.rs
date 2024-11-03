use cursive::theme::{BorderStyle, Effect, Style, Theme};
use cursive::utils::markup::StyledString;
use cursive::view::{IntoBoxedView, Nameable, Resizable};
use cursive::views::*;
use cursive::Cursive;
use cursive::View;
use std::sync::Arc;

use super::theme::get_borderless_layout;

/// A factory function to display an overlay dialog with a fullscreen background cover.
///
/// # Arguments
/// * `siv` - The Cursive instance to add the layers to.
/// * `dialog` - A pre-configured Dialog that will be wrapped in the overlay.
pub fn show_overlay_view<V>(siv: &mut Cursive, content: V)
where
    V: View + 'static,
{
    // Create a StackView to layer the background and dialog
    let mut stack = StackView::new();

    // Add the fullscreen background overlay as the bottom layer in the stack
    stack.add_fullscreen_layer(TextView::new("").full_screen());

    let mut message = StyledString::new();
    message.append_plain("Press ");
    message.append_styled("ESC", Style::from(Effect::Bold));
    message.append_plain(" to go back.");

    // Create a layout with the content at the top and the ESC message at the bottom
    let layout = LinearLayout::vertical()
        .child(content) // Add the main content at the top
        .child(TextView::new(message).fixed_height(1)); // Add the ESC message at the bottom

    // Wrap the layout with custom borders if needed
    let dialog_with_esc_text = get_borderless_layout(siv, layout, None).full_screen();

    // Add the layout as the top layer in the stack
    stack.add_fullscreen_layer(dialog_with_esc_text);

    // Add the StackView as a single fullscreen layer with a name
    siv.add_layer(stack.with_name("overlay_dialog").full_screen());
}

pub fn center_layout_view<V>(siv: &mut Cursive, content: V, theme: Option<Theme>) -> LinearLayout
where
    V: View + 'static,
{
    let theme = theme.unwrap_or_else(|| siv.current_theme().clone());

    // Create the vertically-centered layout
    let vertical_centered_layout = LinearLayout::vertical()
        .child(ResizedView::with_full_height(TextView::new(""))) // Top dummy
        .child(ThemedView::new(theme, content)) // Centered content
        .child(ResizedView::with_full_height(TextView::new(""))); // Bottom dummy

    // Wrap in a horizontally-centered layout with left and right dummies
    LinearLayout::horizontal()
        .child(ResizedView::with_full_width(TextView::new(""))) // Left dummy
        .child(vertical_centered_layout) // Centered content
        .child(ResizedView::with_full_width(TextView::new(""))) // Right dummy
}

pub fn confirm<F>(siv: &mut Cursive, question: &str, callback: F)
where
    F: Fn(&mut Cursive) + Send + Sync + 'static,
{
    let question_text = question.to_string();
    let callback = Arc::new(callback); // Use Arc for thread-safe reference counting

    // Clone the current theme and set simple borders
    let mut theme = siv.current_theme().clone();
    theme.borders = BorderStyle::Simple;

    let dialog = Dialog::around(
        LinearLayout::vertical()
            .child(DummyView) // Spacer line
            .child(TextView::new(question_text).center()),
    )
    .title("Confirmation")
    .button("No", |s| {
        s.pop_layer(); // Close dialog without calling callback
    })
    .button("Yes", {
        let callback = Arc::clone(&callback);
        move |s| {
            callback(s);
            s.pop_layer(); // Close dialog after calling callback
        }
    })
    .fixed_width(75);

    let view = center_layout_view(siv, dialog, Some(theme));

    // Show the centered dialog as an overlay
    show_overlay_view(siv, view);
}

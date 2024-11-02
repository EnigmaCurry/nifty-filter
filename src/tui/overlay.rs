use cursive::theme::{Effect, Style};
use cursive::utils::markup::StyledString;
use cursive::view::{Nameable, Resizable};
use cursive::views::*;
use cursive::Cursive;

use super::theme::get_borderless_layout;

/// A factory function to display an overlay dialog with a fullscreen background cover.
///
/// # Arguments
/// * `siv` - The Cursive instance to add the layers to.
/// * `dialog` - A pre-configured Dialog that will be wrapped in the overlay.
pub fn show_overlay_dialog(siv: &mut Cursive, dialog: ThemedView<Dialog>) {
    // Create a StackView to layer the background and dialog
    let mut stack = StackView::new();

    // Add the fullscreen background overlay as the bottom layer in the stack
    stack.add_fullscreen_layer(TextView::new("").full_screen());

    let mut message = StyledString::new();
    message.append_plain("Press ");
    message.append_styled("ESC", Style::from(Effect::Bold));
    message.append(" to go back.");
    // Create a new layout with the original content at the top and the ESC text at the bottom
    let layout = LinearLayout::vertical()
        .child(dialog) // Add the original dialog content at the top
        .child(TextView::new(message).fixed_height(1)); // Add the text message

    // Create a new dialog with the updated content
    let dialog_with_esc_text = get_borderless_layout(siv, layout, None).full_screen();

    // Add the dialog with ESC as the top layer in the stack
    stack.add_fullscreen_layer(dialog_with_esc_text);

    // Add the StackView as a single layer, with a name for easy management
    siv.add_layer(stack.with_name("overlay_dialog").full_screen());
}

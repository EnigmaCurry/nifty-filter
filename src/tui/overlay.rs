use cursive::view::{Nameable, Resizable};
use cursive::views::*;
use cursive::Cursive;

/// A factory function to display an overlay dialog with a fullscreen background cover.
///
/// # Arguments
/// * `siv` - The Cursive instance to add the layers to.
/// * `dialog` - A pre-configured Dialog that will be wrapped in the overlay.
pub fn show_overlay_dialog(siv: &mut Cursive, dialog: Dialog) {
    // Create a StackView to layer the background and dialog
    let mut stack = StackView::new();

    // Add the fullscreen background overlay as the bottom layer in the stack
    //stack.add_fullscreen_layer(TextView::new("").full_screen());

    // Create a new layout with the original content at the top and the ESC text at the bottom
    let layout = LinearLayout::vertical()
        .child(dialog) // Add the original dialog content at the top
        .child(DummyView.full_screen())
        .child(TextView::new("Press ESC to go back").fixed_height(1)); // Add the text message

    // Create a new dialog with the updated content
    let dialog_with_esc_text = Dialog::around(layout).full_screen();

    // Add the dialog with ESC as the top layer in the stack
    stack.add_fullscreen_layer(dialog_with_esc_text);

    // Add the StackView as a single layer, with a name for easy management
    siv.add_layer(stack.with_name("overlay_dialog").full_screen());
}

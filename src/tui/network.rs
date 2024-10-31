use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;

use super::overlay::show_overlay_dialog;

pub fn main(siv: &mut Cursive) {
    let dialog = Dialog::new()
        .title("Network Settings")
        .content(TextView::new("This overlay hides the main layer."));

    show_overlay_dialog(siv, dialog);
}

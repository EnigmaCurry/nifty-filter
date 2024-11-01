use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;

use super::overlay::show_overlay_dialog;

pub fn main(siv: &mut Cursive) -> LinearLayout {
    let dialog = Dialog::new().content(TextView::new("This overlay hides the main layer."));

    LinearLayout::vertical().child(Dialog::new().content(dialog))
}

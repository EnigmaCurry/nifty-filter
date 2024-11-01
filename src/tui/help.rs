use super::overlay::show_overlay_dialog;
use cursive::view::Resizable;
use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;
use strum::{AsRefStr, Display, EnumIter, EnumString, IntoEnumIterator};

#[derive(EnumIter, AsRefStr, EnumString, Debug, Clone, Display)]
enum MenuItem {
    #[strum(serialize = "Netfilter / Nftables")]
    Nftables,
}

pub fn main(_siv: &mut Cursive) -> LinearLayout {
    let app_name = env!("CARGO_PKG_NAME");
    let version = env!("CARGO_PKG_VERSION");
    let author = env!("CARGO_PKG_AUTHORS");
    let repository = env!("CARGO_PKG_REPOSITORY");
    let help_text = format!(
        "{} v{}\n\
        Author: {}\n\
        Repository: {}\n",
        app_name, version, author, repository
    );
    let about = Dialog::new()
        .title("About")
        .content(TextView::new(help_text));

    let mut menu = SelectView::<MenuItem>::new()
        .on_submit(|siv: &mut Cursive, choice: &MenuItem| menu_action(siv, choice));
    for item in MenuItem::iter() {
        menu.add_item(item.as_ref(), item.clone());
    }

    // Wrap the menu in a Dialog to give it a title and a Quit button
    let dialog = Dialog::new().content(menu).full_screen();

    LinearLayout::vertical()
        .child(dialog.full_screen())
        .child(about)
}

fn nftables(siv: &mut Cursive) {
    let dialog = Dialog::new()
        .title("Nftables")
        .content(TextView::new("TODO"));
    show_overlay_dialog(siv, dialog);
}

// Function to handle menu actions
fn menu_action(siv: &mut Cursive, choice: &MenuItem) {
    match choice {
        MenuItem::Nftables => nftables(siv),
    }
}

use cursive::event::{Event, Key};
use cursive::traits::*;
use cursive::views::*;
use cursive::Cursive;
use cursive::CursiveExt;
use strum::IntoEnumIterator;
use strum::{AsRefStr, Display, EnumIter, EnumString};

use self::overlay::show_overlay_dialog;

mod dhcp;
mod dns;
mod firewall;
mod help;
mod interface;
pub mod overlay;
mod theme;

#[derive(EnumIter, AsRefStr, EnumString, Debug, Clone, Display)]
enum MenuItem {
    #[strum(serialize = "Network Interfaces")]
    Interfaces,
    #[strum(serialize = "Firewall and Router")]
    Firewall,
    #[strum(serialize = "DHCP")]
    DHCP,
    #[strum(serialize = "DNS")]
    DNS,
    #[strum(serialize = "Help")]
    Help,
}

pub fn main() {
    let mut siv = Cursive::default();

    // Set theme:
    siv.set_theme(theme::theme1());

    // Title for the entire screen
    let title = TextView::new(format!(
        "{} v{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    ))
    .h_align(cursive::align::HAlign::Center) // Center-align the title
    .fixed_height(1); // Limit title height to 1 line

    // Define the SelectView for the main menu and set an action on submit
    let mut menu = SelectView::new()
        .on_submit(|siv: &mut Cursive, choice: &MenuItem| menu_action(siv, choice));

    // Populate the menu with items using the enum
    for item in MenuItem::iter() {
        menu.add_item(item.as_ref(), item.clone());
    }

    let menu = LinearLayout::vertical()
        .child(Dialog::new().content(menu).full_height())
        .child(TextView::new("Press ESC to quit").fixed_height(1))
        .child(DummyView.fixed_height(1));

    // Wrap the SelectView in a Dialog to give it a title and a Quit button
    let dialog = Dialog::new().title("Main Menu").content(menu).full_height();

    // Use a vertical layout with the title at the top
    let layout = LinearLayout::vertical()
        .child(title) // Add the screen title
        .child(DummyView.fixed_height(1)) // Add some padding
        .child(dialog.full_screen());

    // Add the dialog to the cursive root
    siv.add_fullscreen_layer(layout);

    // Add global ESC key callback
    siv.add_global_callback(Key::Esc, |s| {
        if s.screen().len() == 1 {
            s.quit();
        } else {
            s.pop_layer();
        }
    });

    // Add keyboard shortcuts:
    // Emacs :
    siv.add_global_callback(Event::CtrlChar('n'), |s| {
        // Simulate pressing down arrow
        s.on_event(Event::Key(Key::Down));
    });
    siv.add_global_callback(Event::CtrlChar('p'), |s| {
        // Simulate pressing up arrow
        s.on_event(Event::Key(Key::Up));
    });
    siv.add_global_callback(Event::CtrlChar('j'), |s| {
        // Simulate pressing Enter
        s.on_event(Event::Key(Key::Enter));
    });
    siv.add_global_callback(Event::CtrlChar('g'), |s| {
        // Simulate pressing Enter
        s.on_event(Event::Key(Key::Esc));
    });

    // Vim :
    siv.add_global_callback('j', |s| {
        // Simulate pressing down arrow
        s.on_event(Event::Key(Key::Down));
    });
    siv.add_global_callback('k', |s| {
        // Simulate pressing up arrow
        s.on_event(Event::Key(Key::Up));
    });

    // Start the TUI
    siv.run();
}

// Function to handle menu actions
fn menu_action(siv: &mut Cursive, choice: &MenuItem) {
    let content: LinearLayout = match choice {
        MenuItem::Interfaces => interface::main(siv),
        MenuItem::Firewall => firewall::main(siv),
        MenuItem::DHCP => dhcp::main(siv),
        MenuItem::DNS => dns::main(siv),
        MenuItem::Help => help::main(siv),
    };
    let dialog = Dialog::new().title(choice.to_string()).content(content);
    show_overlay_dialog(siv, dialog);
}

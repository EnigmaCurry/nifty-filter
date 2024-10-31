use cursive::event::Key;
use cursive::traits::*;
use cursive::views::*;
use cursive::Cursive;
use cursive::CursiveExt;

mod dhcp;
mod dns;
mod firewall;
mod help;
mod network;
pub mod overlay;
mod theme;

pub fn main() {
    let mut siv = Cursive::default();

    // Set theme:
    siv.set_theme(theme::theme1());

    // Title for the entire screen
    let title = TextView::new("nifty-filter")
        .h_align(cursive::align::HAlign::Center) // Center-align the title
        .fixed_height(1); // Limit title height to 1 line

    // Define the SelectView for the main menu and set an action on submit
    let mut menu =
        SelectView::new().on_submit(|siv: &mut Cursive, choice: &String| menu_action(siv, choice));

    // Populate the menu with items
    menu.add_item("Network", "Network".to_string());
    menu.add_item("Firewall", "Firewall".to_string());
    menu.add_item("DHCP", "DHCP".to_string());
    menu.add_item("DNS", "DNS".to_string());
    menu.add_item("Help", "Help".to_string());

    // Wrap the SelectView in a Dialog to give it a title and a Quit button
    let dialog = Dialog::new()
        .title("Main Menu")
        .content(menu)
        .button("ESC", |s| s.quit())
        .full_height();

    // Use a vertical layout with the title at the top
    let layout = LinearLayout::vertical()
        .child(title) // Add the screen title
        .child(DummyView.fixed_height(1)) // Add some padding
        .child(dialog.full_screen()); // Center the main content

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

    // Start the TUI
    siv.run();
}

// Function to handle menu actions
fn menu_action(siv: &mut Cursive, choice: &str) {
    match choice {
        "Network" => network::main(siv),
        "Firewall" => firewall::main(siv),
        "DHCP" => dhcp::main(siv),
        "DNS" => dns::main(siv),
        "Help" => help::main(siv),
        _ => {}
    }
}

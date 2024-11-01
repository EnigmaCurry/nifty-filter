use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;

use super::overlay::show_overlay_dialog;

pub fn main(siv: &mut Cursive) {
    let mut menu =
        SelectView::new().on_submit(|siv: &mut Cursive, choice: &String| menu_action(siv, choice));

    // Populate the menu with items
    menu.add_item("Rename Interfaces", "Rename Interfaces".to_string());
    menu.add_item("Assign Interfaces", "Assign Interfaces".to_string());
    menu.add_item("Set IP Addresses", "Set IP Addresses".to_string());

    let dialog = Dialog::new().title("Network Settings").content(menu);

    show_overlay_dialog(siv, dialog);
}

// Function to handle menu actions
fn menu_action(siv: &mut Cursive, choice: &str) {
    match choice {
        "Rename Interfaces" => rename_interfaces(siv),
        "Assign Interfaces" => assign_interfaces(siv),
        "Set IP Addresses" => set_ip_addresses(siv),
        _ => {}
    }
}

pub fn rename_interfaces(siv: &mut Cursive) {
    let dialog = Dialog::new()
        .title("Rename interfaces")
        .content(TextView::new("This overlay hides the main layer."));

    show_overlay_dialog(siv, dialog);
}

pub fn assign_interfaces(siv: &mut Cursive) {
    let dialog = Dialog::new()
        .title("Assign interfaces")
        .content(TextView::new("This overlay hides the main layer."));

    show_overlay_dialog(siv, dialog);
}

pub fn set_ip_addresses(siv: &mut Cursive) {
    let dialog = Dialog::new()
        .title("Set IP addresses")
        .content(TextView::new("This overlay hides the main layer."));

    show_overlay_dialog(siv, dialog);
}

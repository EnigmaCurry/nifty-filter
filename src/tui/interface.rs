//use super::overlay::show_overlay_dialog;
use crate::info::network::{get_interfaces, InterfaceInfo};
use crate::systemd::check_service_status;
use cursive::view::Nameable;
use cursive::view::Resizable;
use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;
use strum::{AsRefStr, Display, EnumIter, EnumString, IntoEnumIterator};

use super::overlay::show_overlay_dialog;

fn format_interface_info(info: &InterfaceInfo) -> String {
    format!(
        "Name: {}\nMAC: {}\nStatus: {}\nIPv4: {}\n\nPress Enter to configure this interface.",
        info.name,
        info.mac_address
            .clone()
            .unwrap_or_else(|| "N/A".to_string()),
        info.status,
        info.ip_addresses
            .iter()
            .find(|ip| ip.contains('.'))
            .unwrap_or(&"none".to_string())
    )
}

fn rename_interface(siv: &mut Cursive, interface_name: String) {}

pub fn configure_interface(siv: &mut Cursive, interface_name: String) {
    #[derive(EnumIter, AsRefStr, EnumString, Debug, Clone, Display)]
    enum MenuItem {
        #[strum(serialize = "Rename Interface")]
        RenameInterface,
        #[strum(serialize = "Change IPv4 address")]
        ChangeIpv4Address,
        #[strum(serialize = "Change Gateway address")]
        ChangeGateway,
        #[strum(serialize = "Change DNS addresses")]
        ChangeDNS,
    }
    let ifname = interface_name.clone();
    let mut menu =
        SelectView::<MenuItem>::new().on_submit(move |siv: &mut Cursive, choice: &MenuItem| {
            match choice {
                &MenuItem::RenameInterface => rename_interface(siv, ifname.clone()),
                _ => panic!(),
            }
        });
    for item in MenuItem::iter() {
        menu.add_item(item.as_ref(), item.clone());
    }
    let interface_name = interface_name.to_string();
    let dialog = Dialog::around(menu).title(format!("Rename Interface : {interface_name}"));
    show_overlay_dialog(siv, dialog);
}

pub fn main(siv: &mut Cursive) -> LinearLayout {
    let interfaces = get_interfaces();

    // Check services and create warnings if necessary
    let mut warnings = String::new();
    for service in [
        "NetworkManager",
        "cloud-init",
        "wicd",
        "connman",
        "dhclient",
        "isc-dhcp-client",
        "ifupdown",
        "netplan",
    ]
    .iter()
    {
        if check_service_status(service) {
            warnings.push_str(&format!(
                "Warning: service {} is active which may interfere with network config.\n",
                service
            ));
        }
    }

    // Create the SelectView for the interface names
    let mut menu = SelectView::<String>::new()
        .on_select({
            let interfaces = interfaces.clone();
            move |siv, selected_name| {
                // Find the selected interface and update the info box
                let interface_info = interfaces.iter().find(|i| &i.name == selected_name);
                if let Some(info) = interface_info {
                    let details = format_interface_info(info);
                    siv.call_on_name("info_box", |view: &mut TextView| {
                        view.set_content(details);
                    });
                }
            }
        })
        .on_submit({
            move |siv: &mut Cursive, selected_name: &String| {
                configure_interface(siv, selected_name.clone());
            }
        });

    for interface_info in &interfaces {
        menu.add_item(interface_info.name.clone(), interface_info.name.clone());
    }

    // Create an initial info box with details of the first interface if available
    let initial_info = interfaces
        .first()
        .map_or("No interfaces available".to_string(), |info| {
            format_interface_info(info)
        });

    let info_box = TextView::new(initial_info).with_name("info_box");

    // Conditionally create the warnings box
    let warnings_box = if !warnings.is_empty() {
        Some(Dialog::around(TextView::new(warnings)).title("Warnings"))
    } else {
        None
    };

    // Build the layout with the menu, warnings (if any), and info box
    let mut layout = LinearLayout::vertical().child(Dialog::around(menu));

    if let Some(warnings_dialog) = warnings_box {
        layout.add_child(warnings_dialog.padding_top(1));
    }

    layout.child(Dialog::around(info_box).title("Interface Details"))
}

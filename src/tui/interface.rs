//use super::overlay::show_overlay_dialog;
use crate::info::interfaces::{
    get_interfaces, InterfaceInfo, InterfaceType, MANAGED_INTERFACE_TYPES,
};
use crate::systemd::check_service_status;
use cursive::theme::{BaseColor, Color, Effect, PaletteColor, Style};
use cursive::utils::markup::StyledString;
use cursive::view::Nameable;
use cursive::views::*;
use cursive::Cursive;
#[allow(unused_imports)]
use cursive::CursiveExt;
use strum::{AsRefStr, Display, EnumIter, EnumString, IntoEnumIterator};

use super::overlay::show_overlay_dialog;
use super::theme::{self, set_highlight_disabled, theme1};

fn format_interface_info(info: &InterfaceInfo) -> String {
    let interface_type = match info.interface_type {
        InterfaceType::Loopback => "Loopback",
        InterfaceType::Bridge => "Bridge",
        InterfaceType::PhysicalEthernet => "Physical Ethernet",
        InterfaceType::PhysicalWifi => "Physical WiFi",
        InterfaceType::Virtual => "Virtual Ethernet",
        InterfaceType::Tap => "TAP device",
        InterfaceType::Unknown => "Unknown",
    };
    let message;
    if MANAGED_INTERFACE_TYPES.contains(&info.interface_type) {
        message = "Press Enter to configure this interface.";
    } else {
        message = "This interface cannot be configured by this tool."
    }

    format!(
        "Name: {}\nType: {}\nMAC: {}\nStatus: {}\nIPv4: {}\nHardware: {}\n\n{}",
        info.name,
        interface_type,
        info.mac_address
            .clone()
            .unwrap_or_else(|| "N/A".to_string()),
        info.status,
        info.ip_addresses
            .iter()
            .find(|ip| ip.contains('.'))
            .unwrap_or(&"none".to_string()),
        info.pci_info,
        message
    )
}

fn rename_interface(_siv: &mut Cursive, interface_name: String) {}

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
                _ => {}
            }
        });
    for item in MenuItem::iter() {
        menu.add_item(item.as_ref(), item.clone());
    }

    let layout = LinearLayout::vertical().child(menu);

    let dialog = Dialog::around(layout)
        .title(format!("Configure Interface"))
        .padding_top(1);
    show_overlay_dialog(siv, dialog);
}

pub fn main(siv: &mut Cursive) -> LinearLayout {
    let interfaces: Vec<InterfaceInfo> = get_interfaces();

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
                // Find the selected interface
                let interface_info = interfaces.iter().find(|i| &i.name == selected_name);

                // Update the info box with the selected interface's details
                if let Some(info) = interface_info {
                    let details = format_interface_info(info);
                    siv.call_on_name("info_box", |view: &mut TextView| {
                        view.set_content(details);
                    });

                    // Change the highlight color to show ounmanaged devices as grey:
                    if !MANAGED_INTERFACE_TYPES.contains(&info.interface_type) {
                        // Update the theme for the highlight color dynamically
                        set_highlight_disabled(siv);
                    } else {
                        siv.set_theme(theme1());
                    }
                }
            }
        })
        .on_submit({
            let interfaces = interfaces.clone();
            move |siv: &mut Cursive, selected_name: &String| {
                let interface_info = interfaces.iter().find(|i| &i.name == selected_name);
                if let Some(info) = interface_info {
                    if MANAGED_INTERFACE_TYPES.contains(&info.interface_type) {
                        configure_interface(siv, selected_name.clone());
                    }
                }
            }
        });

    for interface_info in &interfaces {
        if MANAGED_INTERFACE_TYPES.contains(&interface_info.interface_type) {
            menu.add_item(interface_info.name.clone(), interface_info.name.clone());
        } else {
            menu.add_item(
                StyledString::styled(interface_info.name.clone(), Effect::Italic),
                interface_info.name.clone(),
            );
        }
    }

    // Disable highlight if first interface is unmanageable:
    match interfaces.first() {
        Some(interface) => {
            if !MANAGED_INTERFACE_TYPES.contains(&interface.interface_type) {
                set_highlight_disabled(siv);
            }
        }
        None => {}
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

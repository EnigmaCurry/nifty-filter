use colored_json::ToColoredJson;
use serde_json::Value;
pub mod network;

pub fn pretty_print_json(json: Value) {
    let json_string = serde_json::to_string_pretty(&json).expect("Invalid JSON");
    // Print with colors if outputting to a terminal
    if atty::is(atty::Stream::Stdout) {
        println!(
            "{}",
            json_string.to_colored_json_auto().expect("Invalid JSON")
        );
    } else {
        println!("{}", json_string);
    }
}

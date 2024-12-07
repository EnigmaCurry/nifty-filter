// use askama::Template;
// use glob::glob;
// use std::fs;
// use std::io::Write;

// #[derive(Template)]
// #[template(path = "systemd-network.link.txt")]

// struct LinkTemplate {
//     description: String,
//     mac_address: String,
//     name: String,
// }

// pub fn configure_link(name: &str, mac_address: &str, priority: usize, description: &str) {
//     let template = LinkTemplate {
//         description: description.to_string(),
//         mac_address: mac_address.to_string(),
//         name: name.to_string(),
//     };

//     // Render the template to a string
//     let rendered = template.render().unwrap();

//     // Define the output path using the priority and name
//     let path = format!("/etc/systemd/network/{}-{}.link", priority, name);

//     // Remove any existing file with the same name regardless of priority
//     let glob_pattern = format!("/etc/systemd/network/*-{}.link", name);
//     for entry in glob(&glob_pattern).expect("Failed to read glob pattern") {
//         if let Ok(existing_path) = entry {
//             fs::remove_file(&existing_path).expect("Failed to remove existing file");
//         }
//     }

//     // Create the new file and write the rendered content
//     let mut file = fs::File::create(&path).expect("Unable to create file");
//     file.write_all(rendered.as_bytes())
//         .expect("Unable to write data");
// }

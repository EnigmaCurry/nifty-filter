use std::process::Command;

pub fn check_service_status(service_name: &str) -> bool {
    let output = Command::new("systemctl")
        .arg("is-active")
        .arg(service_name)
        .output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

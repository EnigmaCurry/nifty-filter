use std::str::FromStr;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ChainPolicy {
    Accept,
    Drop,
    Reject,
    Continue,
    Return,
    Queue,
    Log,
}

impl ChainPolicy {
    pub fn new(input: &str) -> Result<Self, String> {
        input.parse::<ChainPolicy>()
    }

    fn variants() -> Vec<String> {
        vec![
            "accept".to_string(),
            "drop".to_string(),
            "reject".to_string(),
            "continue".to_string(),
            "return".to_string(),
            "queue".to_string(),
            "log".to_string(),
        ]
    }
}

impl FromStr for ChainPolicy {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.to_lowercase().as_str() {
            "accept" => Ok(ChainPolicy::Accept),
            "drop" => Ok(ChainPolicy::Drop),
            "reject" => Ok(ChainPolicy::Reject),
            "continue" => Ok(ChainPolicy::Continue),
            "return" => Ok(ChainPolicy::Return),
            "queue" => Ok(ChainPolicy::Queue),
            "log" => Ok(ChainPolicy::Log),
            _ => Err(format!(
                "Invalid chain policy: '{}'. Acceptable values are: {}",
                input,
                ChainPolicy::variants().join(", ")
            )),
        }
    }
}

impl std::fmt::Display for ChainPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ChainPolicy::Accept => "accept",
                ChainPolicy::Drop => "drop",
                ChainPolicy::Reject => "reject",
                ChainPolicy::Continue => "continue",
                ChainPolicy::Return => "return",
                ChainPolicy::Queue => "queue",
                ChainPolicy::Log => "log",
            }
        )
    }
}

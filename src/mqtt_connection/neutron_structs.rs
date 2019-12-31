use serde_json::to_string;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum CommandType {
    Online,                 // Sends to own topic
    Offline,                // Sends to own topic

    UpdateInstall,          // Received on own topic
    RemoteManagement,       // Received on own topic

    MQTTServerCA                // <UNIMPLEMENTED> Received on global topic
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Command {
    pub command: CommandType,
    pub data: String,
}

impl Command {
    pub fn new(command: CommandType, data: &str) -> Self {
        Self {
            command,
            data: data.to_owned(),
        }
    }

    /**
     * Converts the `Command` struct to a JSON formatted string.
     * If the conversion fails, an error message is printed and `None` is returned.
     */
    pub fn to_string(&self) -> Option<String> {
        match to_string(self) {
            Ok(res) => return Some(res),
            Err(e) => error!("Could not convert command to string. Command: {:?} | Err: {}", self.command, e),
        }
        None
    }
}

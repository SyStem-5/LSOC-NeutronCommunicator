use serde_json::to_string;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum CommandType {
    RefreshUpdateManifest,         // Received on ROOT_NECO_TOPIC
    StartUpdateDownloadAndInstall, // Received on <self> NECO topic
    Changelogs,                    // Sends to ROOT_EXTERNAL_INTERFACE
    UpdateStarted,                 // Sends to ROOT_EXTERNAL_INTERFACE
    State,                         // Sends to ROOT_EXTERNAL_INTERFACE

    ComponentStates, // Sends to ROOT_EXTERNAL_INTERFACE, received on ROOT_NECO_TOPIC
    ComponentLog,    // Sends to ROOT_EXTERNAL_INTERFACE, received on <self> NECO topic

    // This is not needed right now
    // Probably going to be used for communication between NECOs
    //CertRenewal,                  // Sends to ROOT_NECO_TOPIC
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

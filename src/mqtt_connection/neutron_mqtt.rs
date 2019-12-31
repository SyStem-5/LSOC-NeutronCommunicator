use crate::mqtt::{message, AsyncClient, Message};
use serde_json::from_str as from_json;

use super::neutron_structs::{Command, CommandType};
use crate::remote_management::start_ssh_server;

// This topic is read-only (subscribe only)
const ROOT_TOPIC: &str = "LSOC/communicators";
const RECONNECT_TIMEOUT: u64 = 2500;

/**
 * `OnMessage` mqtt callback
 */
pub fn payload_callback(cli: &AsyncClient, msg: Option<message::Message>) {
    if let Some(msg) = msg {

        let mqtt_cli = cli.clone();
        match from_json(&msg.payload_str()) {
            Ok(result) => process_command(&mqtt_cli, &result),
            Err(e) => {
                error!("Could not parse command struct.");
                debug!("{}", e);
            }
        }
    }
}

/**
 * `OnConnectionSuccess` mqtt callback.
 */
pub fn connection_success(cli: &AsyncClient, _msgid: u16) {
    info!("Neutron Server connection succeeded.");

    cli.subscribe(ROOT_TOPIC, 1);

    cli.subscribe(
        own_topic(cli.inner.client_id.to_str().unwrap_or_default()),
        1,
    );

    cli.publish(send_state(
        true,
        cli.inner.client_id.to_str().unwrap_or_default(),
    ));
}

/**
 * `OnConnectionFail` mqtt callback.
 */
pub fn connection_failure(cli: &AsyncClient, _msgid: u16, rc: i32) {
    debug!("Connection attempt failed with error code {}.", rc);

    std::thread::sleep(std::time::Duration::from_millis(RECONNECT_TIMEOUT));
    cli.reconnect_with_callbacks(connection_success, connection_failure);
}

/**
 * `OnConnectionLost` mqtt callback.
 */
pub fn connection_lost(cli: &AsyncClient) {
    error!("Connection lost. Reconnecting...");

    std::thread::sleep(std::time::Duration::from_millis(RECONNECT_TIMEOUT));
    cli.reconnect_with_callbacks(connection_success, connection_failure);
}

/**
 * Executes the command type the main node issued to us and passes the data of the command to the matched function.
 */
fn process_command(mqtt_client: &AsyncClient, cmd: &Command) {
    match cmd.command {
        CommandType::RemoteManagement => start_ssh_server(mqtt_client, &cmd.data),
        CommandType::UpdateInstall => {
            //TODO
            // Fetch the Update Manifest
            // Start UpdateDownloadAndInstall
        },
        CommandType::MQTTServerCA => {},
        _ => {}
    }
}

/**
 * Returns the state command in relation to the `state` parameter.
 * The `client_id` parameter is required to create the topic path.
 */
pub fn send_state(state: bool, client_id: &str) -> Message {
    let cmd_type = if state {
        CommandType::Online
    } else {
        CommandType::Offline
    };

    Message::new(
        own_topic_out(client_id),
        Command::new(cmd_type, "").to_string().unwrap_or_default(),
        1,
    )
}

/**
 * ```This topic is read-only (subscribe only).```
 *
 * Concatenates the root topic and the client id to form the clients own topic.
 */
fn own_topic(client_id: &str) -> String {
    [ROOT_TOPIC, "/", client_id].concat()
}

/**
 * ```This has to be used if we're publishing!```
 *
 * Concatenates the own topic and the '/out' part to form the write-only topic.
 */
pub fn own_topic_out(client_id: &str) -> String {
    [&own_topic(client_id), "/out"].concat()
}

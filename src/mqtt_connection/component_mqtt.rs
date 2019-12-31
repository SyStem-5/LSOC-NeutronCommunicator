//use crate::encryption_certificates::structs::CertRenewal;
use crate::mqtt::{message, AsyncClient, Message};
use crate::version_control::{
    get_component_log, get_component_states, request_update_manifest, update_download_and_install,
};
// use crate::COMPONENT_MQTT_OWN_TOPIC;
use serde_json::from_str as from_json;

use super::component_structs::{Command, CommandType};

const RECONNECT_TIMEOUT: u64 = 2500;
const ROOT_EXTERNAL_INTERFACE_TOPIC: &str = "external_interface";
pub const ROOT_NECO_TOPIC: &str = "neutron_communicators";
// const ROOT_TOPIC_ALL: &str = "neutron_communicators/#";

/**
 * `OnMessage` mqtt callback
 */
pub fn payload_callback(cli: &AsyncClient, msg: Option<message::Message>) {
    if let Some(msg) = msg {
        //let topic = msg.topic().split('/');
        //let topic_split: Vec<&str> = topic.collect();
        //let payload_str = msg.payload_str();

        /*dbg!();
        warn!("Topic: {}, Payload: {}", msg.topic(), payload_str);*/

        //if topic_split.len() == 1 && topic_split[0] == ROOT_TOPIC {
        let mqtt_cli = cli.clone();
        match from_json(&msg.payload_str()) {
            Ok(result) => process_command(&mqtt_cli, &result),
            Err(e) => {
                error!("Could not parse command struct.");
                debug!("{}", e);
            }
        }
        // std::thread::spawn(move || {

        //     match serde_json::from_str(&msg.payload_str()) {
        //         Ok(result) => process_command(&mqtt_cli, &result),
        //         Err(e) => {
        //             error!("Could not parse command from component mqtt root topic.");
        //             debug!("{}", e);
        //         }
        //     }
        // });
        //}
    }
}

/**
 * `OnConnectionSuccess` mqtt callback.
 */
pub fn connection_success(cli: &AsyncClient, _msgid: u16) {
    info!("Backhaul broker connection succeeded.");

    cli.subscribe(ROOT_NECO_TOPIC, 1);

    cli.subscribe(
        [
            ROOT_NECO_TOPIC,
            "/",
            cli.inner.client_id.to_str().unwrap_or_default(),
        ]
        .concat(),
        1,
    );

    send_component_states(cli);
    // cli.subscribe(ROOT_TOPIC_ALL, 1);
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
        CommandType::RefreshUpdateManifest => request_update_manifest(&mqtt_client),
        CommandType::StartUpdateDownloadAndInstall => {
            send_update_started(&mqtt_client);
            update_download_and_install(&mqtt_client);
        }
        CommandType::ComponentStates => send_component_states(mqtt_client),
        CommandType::ComponentLog => send_component_log(mqtt_client, &cmd.data),
        _ => {}
    }
}

/**
 * Responds to the `External Interface` topic.
 * We publish a payload containing a list of components (& their states) that this NECO is in charge for.
 */
fn send_component_states(client: &AsyncClient) {
    match get_component_states() {
        Ok(json) => {
            if let Some(command) = Command::new(CommandType::ComponentStates, &json).to_string() {
                let msg = Message::new(ROOT_EXTERNAL_INTERFACE_TOPIC, command, 1);
                client.publish(msg);
            }
        }
        Err(e) => error!("Could not send component states. {}", e),
    }
}

/**
 * Responds to the `External Interface` topic.
 * Returns the component log (can be a service or a container component).
 */
fn send_component_log(client: &AsyncClient, data: &str) {
    match get_component_log(data) {
        Ok(json) => {
            if let Some(command) = Command::new(CommandType::ComponentLog, &json).to_string() {
                let msg = Message::new(ROOT_EXTERNAL_INTERFACE_TOPIC, command, 1);
                client.publish(msg);
            }
        }
        Err(e) => error!("Could not send component log. {}", e),
    }
}

/**
 * Publishes the state to the `External Interface` topic.
 */
pub fn send_state(client: &AsyncClient, state: &str) {
    if let Some(command) = Command::new(CommandType::State, state).to_string() {
        let msg = Message::new(ROOT_EXTERNAL_INTERFACE_TOPIC, command, 1);
        client.publish(msg);
    }
}

/**
 * Publishes the concatenated changelogs to the `External Interface` topic.
 */
pub fn send_changelogs(client: &AsyncClient, changelogs: &str) {
    if let Some(command) = Command::new(CommandType::Changelogs, changelogs).to_string() {
        let msg = Message::new(ROOT_EXTERNAL_INTERFACE_TOPIC, command, 1);
        client.publish(msg);
    }
}

/**
 * Sends a command telling the WebInterface that the updating procedure has started.
 */
fn send_update_started(client: &AsyncClient) {
    if let Some(command) = Command::new(CommandType::UpdateStarted, "").to_string() {
        let msg = Message::new(ROOT_EXTERNAL_INTERFACE_TOPIC, command, 1);
        client.publish(msg);
    }
}

/*pub fn send_cert_key(client: &AsyncClient, cert_key: CertificateKeyPair) {
    if let Ok(own_topic) = COMPONENT_MQTT_OWN_TOPIC.lock() {
        /*let msg = Message::new(
            own_topic.to_owned(),
            serde_json::to_string(&new_command(
                CommandType::CertKeyUpdate,
                &serde_json::to_string(&cert_key).unwrap(),
            ))
            .unwrap(),
            1,
        );*/
        let msg = Message::new(
            own_topic.to_owned(),
            serde_json::to_string(&Command::new(
                CommandType::CertRenewal,
                &serde_json::to_string(&cert_key).unwrap(),
            ))
            .unwrap(),
            1,
        );
        client.publish(msg);
    } else {
        error!("Couldn't lock mutex COMPONENT_MQTT_OWN_TOPIC, cannot send CertificateKeyPair.");
    }
}*/

use crate::mqtt::{AsyncClient, ConnectOptionsBuilder, SslOptionsBuilder, MQTT_VERSION_3_1_1};

use crate::settings::structs::{ComponentMqttClient, NeutronMqttClient};

use crate::NEUTRON_SERVER_IP;

pub mod component_mqtt;
mod component_structs;

mod neutron_mqtt;
// We only export this
pub use neutron_mqtt::own_topic_out;
pub mod neutron_structs;

/**
 * Initiates the connection to the component backhaul network MQTT broker
 * If connection is successful; returns `Some<AsyncClient>`
 * If we fail to instantiate `AsyncClient`; returns `None`
 */
pub fn init_component_mqtt(mqtt_config: &ComponentMqttClient) -> Option<AsyncClient> {
    info!("Connecting to component backhaul...");
    let mqtt_address = format!("ssl://{}:{}", mqtt_config.ip, mqtt_config.port);

    match AsyncClient::new((mqtt_address.as_str(), mqtt_config.username.as_str() /*Clientid*/)) {
        Ok(mut client) => {
            client.set_connection_lost_callback(component_mqtt::connection_lost);
            client.set_message_callback(component_mqtt::payload_callback);

            let ssl = SslOptionsBuilder::new()
                .trust_store(&mqtt_config.cafile)
                .finalize();

            let conn_opts = ConnectOptionsBuilder::new()
                .keep_alive_interval(std::time::Duration::from_secs(30))
                .mqtt_version(MQTT_VERSION_3_1_1)
                .clean_session(true)
                .ssl_options(ssl)
                .user_name(mqtt_config.username.to_owned())
                .password(mqtt_config.password.to_owned())
                //.will_message(web_interface::wi_announce_blackbox(&cli, false))
                .finalize();

            // Make the connection to the broker
            client.connect_with_callbacks(
                conn_opts,
                component_mqtt::connection_success,
                component_mqtt::connection_failure,
            );

            Some(client)
        }
        Err(e) => {
            error!("Could not create a component mqtt connection. {}", e);

            None
        }
    }
}

/**
 *
 */
pub fn init_neutron_mqtt(mqtt_config: &NeutronMqttClient) -> Option<AsyncClient> {
    info!("Connecting to neutron server...");

    #[cfg(feature = "INSECURE")]
    warn!("Using an insecure Neutron MQTT port!");
    #[cfg(feature = "INSECURE")]
    let mqtt_address = format!("tcp://{}:1883", NEUTRON_SERVER_IP);

    #[cfg(feature = "SECURE")]
    let mqtt_address = format!("ssl://{}:1883", NEUTRON_SERVER_IP);

    match AsyncClient::new((&*mqtt_address, &*mqtt_config.username /*Clientid*/)) {
        Ok(mut client) => {
            client.set_connection_lost_callback(neutron_mqtt::connection_lost);
            client.set_message_callback(neutron_mqtt::payload_callback);

            /* let ssl = SslOptionsBuilder::new()
                .trust_store(&mqtt_config.cafile)
                .finalize(); */

            let conn_opts = ConnectOptionsBuilder::new()
                .keep_alive_interval(std::time::Duration::from_secs(30))
                .mqtt_version(MQTT_VERSION_3_1_1)
                .clean_session(true)
                // .ssl_options(ssl)
                .user_name(mqtt_config.username.to_owned())
                .password(mqtt_config.password.to_owned())
                .will_message(neutron_mqtt::send_state(false, client.inner.client_id.to_str().unwrap_or_default()))
                .finalize();

            // Make the connection to the broker
            client.connect_with_callbacks(
                conn_opts,
                neutron_mqtt::connection_success,
                neutron_mqtt::connection_failure,
            );

            Some(client)
        }
        Err(e) => {
            error!("Could not create a component mqtt connection. {}", e);

            None
        }
    }
}

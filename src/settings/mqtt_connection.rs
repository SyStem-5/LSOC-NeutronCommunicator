use std::io::Error;

use super::{save_to_file, structs};

/**
 * Sets the Neutron account settings and saves them to file.
 */
pub fn save_neutron_creds(
    mut settings: structs::Settings,
    neutron_user: &str,
    username: &str,
    password: &str,
) -> Result<(), Error> {
    settings.neutron_account_username = neutron_user.to_owned();
    settings.neutron_mqtt_client.username = username.to_owned();
    settings.neutron_mqtt_client.password = password.to_owned();

    save_to_file(settings)
}

/**
 * Sets the component backhaul server credentials and saves them to file.
 */
pub fn save_component_creds(
    mut settings: structs::Settings,
    ip: &str,
    port: &str,
    username: &str,
    password: &str,
    ca_path: &str,
) -> Result<(), Error> {
    settings.component_mqtt_client.ip = ip.to_owned();
    settings.component_mqtt_client.port = port.to_owned();
    settings.component_mqtt_client.username = username.to_owned();
    settings.component_mqtt_client.password = password.to_owned();
    settings.component_mqtt_client.cafile = ca_path.to_owned();

    save_to_file(settings)
}

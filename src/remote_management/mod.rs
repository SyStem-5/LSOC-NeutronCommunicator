use std::fs::File;
use std::io::{Error, ErrorKind, Write};
use std::process::Command;

use crate::mqtt::{AsyncClient, Message};
use crate::mqtt_connection::neutron_structs::{Command as NeutronCommand, CommandType};
use crate::mqtt_connection::own_topic_out;

const SSH_FOLDER_PATH: &str = "/root/.ssh";
const AUTHORIZED_KEY_FILE: &str = "authorized_keys";
const CMD_SSH_SERVICE_RESTART: &str = "systemctl restart sshd";

/** WHEN THIS GETS STABILIZED -> REMOVE THE AUTOMATIC KEY IMPLEMENTATION FROM THE INSTALLATION **/

/** This should be called on NEUS to generate the key pair: 'ssh-keygen -a 100 -t ed25519' **/

/**
 *
 */
pub fn start_ssh_server(mqtt: &AsyncClient, pub_key: &str) {
    match get_wan_ip() {
        Ok(ip) => {
            let cmd = NeutronCommand::new(CommandType::RemoteManagement, &ip)
                .to_string()
                .unwrap_or_default();
            let ip_msg = Message::new(
                own_topic_out(mqtt.inner.client_id.to_str().unwrap_or_default()),
                cmd,
                1,
            );

            match set_pub_key(pub_key) {
                Ok(_) => {
                    if let Err(e) = restart_ssh_service() {
                        error!("Failed to restart the SSH service. {}", e);
                    } else {
                        mqtt.publish(ip_msg);
                    }
                }
                Err(e) => error!("Failed to set public SSH key. {}", e),
            }
        }
        Err(e) => error!("Could not get WAN IP address. {}", e),
    }
}

/**
 *
 */
fn set_pub_key(pub_key: &str) -> Result<(), Error> {
    let auth_file_path = [SSH_FOLDER_PATH, "/", AUTHORIZED_KEY_FILE].concat();
    match File::create(&auth_file_path) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(pub_key.as_bytes()) {
                return Err(e);
            }
        }
        Err(e) => return Err(e),
    }

    // Set permissions
    let cmd = format!(
        "chmod 700 {} && chmod 600 {}",
        SSH_FOLDER_PATH, &auth_file_path
    );
    match Command::new("sh").arg("-c").arg(cmd).output() {
        Ok(res) => {
            if !res.stderr.is_empty() {
                return Err(Error::new(
                    ErrorKind::Other,
                    String::from_utf8_lossy(&res.stderr),
                ));
            }
        }
        Err(e) => return Err(e),
    }

    Ok(())
}

/**
 *
 */
fn restart_ssh_service() -> Result<(), Error> {
    match Command::new("sh")
        .arg("-c")
        .arg(CMD_SSH_SERVICE_RESTART)
        .output()
    {
        Ok(res) => {
            if !res.stderr.is_empty() {
                return Err(Error::new(
                    ErrorKind::Other,
                    String::from_utf8_lossy(&res.stderr),
                ));
            }
        }
        Err(e) => return Err(e),
    }

    Ok(())
}

/**
 *
 */
fn get_wan_ip() -> Result<String, Error> {
    match Command::new("sh")
        .arg("-c")
        .arg("curl https://api.ipify.org")
        .output()
    {
        Ok(res) => {
            if !res.stderr.is_empty() {
                return Err(Error::new(
                    ErrorKind::Other,
                    String::from_utf8_lossy(&res.stderr),
                ));
            }

            Ok(String::from_utf8_lossy(&res.stdout).into())
        }
        Err(e) => Err(e),
    }
}

use std::{
    fs::{create_dir_all, File},
    io::prelude::Read,
    io::Error,
    io::Write,
    path::Path,
};

use serde_json::from_str;

const BASE_DIRECTORY: &str = "/etc/LSOCNeutronUpdateClient/";
const SETTINGS_FILE_LOCATION: &str = "/etc/LSOCNeutronUpdateClient/settings.json";

const SETTINGS_DEFAULT: &str = r#"{
    "mosquitto_client": {
        "username": "",
        "password": ""
    },
    "blackbox_uds_path": "/etc/BlackBox/uds",
    "app_name": "lsoc",
    "update_branch": "stable",
	"update_components": {
		"blackbox": "/etc/BlackBox/blackbox_version.json",
        "webinterface": "/etc/BlackBox/webinterface_version.json"
    }
}"#;

/**
 * Checks if the settings file exists.
 * If it exists, try to load and return it.
 * If it exists, but fails to load, log error message and exit.
 * If it doesn't exist return Err to main.
 */
pub fn init() -> Result<serde_json::Value, ()> {
    if !Path::new(SETTINGS_FILE_LOCATION).exists() {
        error!("Settings file not found.");
        info!("Run 'sudo lsoc_neutron_communicator commands' to get the command for generating a settings file.");

        Err(())
    } else {
        match load_settings() {
            Ok(settings) => {
                info!("Settings loaded successfully.");

                Ok(settings)
            }
            Err(e) => {
                error!("Failed to load settings file. {}", e);
                Err(())
            }
        }
    }
}

/**
 * Creates a settings file and saves the default settings in it.
 * Returns Settings object if successfull.
 */
pub fn write_default_settings() -> Result<(), Error> {
    info!("Generating default settings file...");

    create_dir_all(BASE_DIRECTORY)?;

    let mut file = File::create(SETTINGS_FILE_LOCATION)?;
    file.write_all(SETTINGS_DEFAULT.as_bytes())?;

    info!(
        "Default settings file generated. Only root can modify the file. Location: {}",
        SETTINGS_FILE_LOCATION
    );

    Ok(())
}

/**
 * Tries to load the settings file.
 * Returns Settings object if successfull.
 */
fn load_settings() -> Result<serde_json::Value, Error> {
    info!("Loading settings file: '{}'", SETTINGS_FILE_LOCATION);

    let mut contents = String::new();

    let mut file = File::open(SETTINGS_FILE_LOCATION)?;

    file.read_to_string(&mut contents)?;

    let settings: serde_json::Value = from_str(&contents)?;

    Ok(settings)
}

// /**
//  * Saves the settings file.
//  */
// fn save_settings_file(new_settings: &str) -> Result<(), Error> {
//     let mut contents = String::new();

//     let mut file = File::open(SETTINGS_FILE_LOCATION)?;

//     file.read_to_string(&mut contents)?;

//     let mut settings: serde_json::Value = from_str(&contents)?;

//     //settings.blackbox_mqtt_client.mqtt_password = String::from(password);

//     let mut file = File::create(SETTINGS_FILE_LOCATION)?;
//     file.write_all(&to_string(&settings)?.as_bytes())?;

//     Ok(())
// }

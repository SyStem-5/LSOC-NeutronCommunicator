use std::{fs::File, io::prelude::Read, io::Error, io::ErrorKind, io::Write, path::Path};

use serde_json::from_str;

use crate::{APP_NAME, BASE_DIRECTORY};

pub mod encryption_certificates;
pub mod mqtt_connection;
pub mod update_components;
pub mod structs;

const SETTINGS_FILE: &str = "settings.json";

/**
 * Checks if the settings file exists.
 * If it exists, try to load and return return `Ok(structs::Settings)`.
 * If it exists but fails to load, return `Err()`.
 * If it doesn't exist return `Err()`.
 */
pub fn init() -> Result<structs::Settings, ()> {
    if Path::new(&get_settings_location()).exists() {
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
    } else {
        error!("Settings file not found.");
        info!("Run with '--help' to get the command for generating a settings file.");

        Err(())
    }
}

/**
 * Converts the settings struct default() output to JSON and saves it to disk.
 * If the file already exits it is truncated.
 * If the path is invalid it returns an error.
 * Returns settings file path if successful.
 */
pub fn write_default() -> Result<String, Error> {
    info!("Generating default settings file...");

    if let Err(e) = save_to_file(structs::Settings::default()) {
        return Err(e);
    }

    Ok(get_settings_location())
}

/**
 * Tries to load the JSON settings file from the `get_settings_location()` function and parse it.
 * If we're successful at parsing the file, we then add NECO to the `update_components` array in the
 *     settings struct so that we can include ourselves when searching for updates.
 *
 * Returns `Ok(structs::Settings)` if successful.
 */
fn load_settings() -> Result<structs::Settings, Error> {
    let settings_loc = get_settings_location();

    info!("Loading settings file: '{}'", settings_loc);

    let mut contents = String::new();

    match File::open(settings_loc) {
        Ok(mut file) => {
            if let Err(e) = file.read_to_string(&mut contents) {
                return Err(e);
            }
        }
        Err(e) => return Err(e),
    }

    if let Ok(json) = from_str(&contents) {
        let mut settings: structs::Settings = json;

        settings.update_components.push(structs::UpdateComponent {
            name: APP_NAME.to_owned(),
            version_file_path: String::new(),
            permission_user: "root".to_owned(),
            permission_group: "root".to_owned(),
            file_permissions: "700".to_owned(),
            container_name: None,
            service_name: Some(String::from("neutroncommunicator.service")),
            restart_command: String::new(),
        });

        return Ok(settings);
    }

    Err(Error::new(
        std::io::ErrorKind::Other,
        "Failed to convert JSON file to settings type.",
    ))
}

/**
 * Converts the struct `structs::Settings` to JSON and then saves the data to the path given by `get_settings_location()`.
 *
 * This function also removes the `NECO` entry in the `update_components` vector as it is added on startup and there is no need for it to be saved.
 */
fn save_to_file(mut settings: structs::Settings) -> Result<(), Error> {
    let settings_loc = get_settings_location();

    // Remove the NeutronCommunicator from update component vec as it is added at startup and never saved to file
    if let Some(index) = settings
        .update_components
        .iter()
        .position(|x| x.name == APP_NAME)
    {
        settings.update_components.remove(index);
    }

    // Convert to json
    let json_settings;
    match serde_json::to_string_pretty(&settings) {
        Ok(json) => json_settings = json,
        Err(e) => return Err(Error::new(ErrorKind::Other, e)),
    }

    // Save to file
    match File::create(settings_loc) {
        Ok(mut file) => {
            if let Err(e) = file.write_all(&json_settings.as_bytes()) {
                return Err(e);
            }
        }
        Err(e) => return Err(e),
    }

    Ok(())
}

/**
 * Concatenates the `BASE_DIRECTORY` `SETTINGS_FILE` to create the path of the settings file.
 */
fn get_settings_location() -> String {
    [BASE_DIRECTORY, SETTINGS_FILE].concat()
}

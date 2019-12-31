#![allow(clippy::bool_comparison)]

use std::collections::BTreeMap;
use std::fs::{create_dir, create_dir_all, remove_dir_all, remove_file, File};
use std::io::{copy, Error, ErrorKind, Read, Write};
use std::process::Command;

use serde_json;
use serde_json::json;

use crate::mqtt::AsyncClient;

use crate::mqtt_connection::component_mqtt::{send_changelogs, send_state};
use crate::settings::structs::UpdateComponent;

use crate::{
    APP_NAME, APP_VERSION, BASE_DIRECTORY, COMPONENT_VERSIONS,
    NEUTRON_SERVER_IP, NEUTRON_SERVER_PORT, NEUTRON_SERVER_PROTOCOL,
    SETTINGS, UPDATE_COMPONENTS, UPDATE_MANIFEST,
};

mod recipe_processor;
mod security;
pub mod structs;

const TEMP_UPDATE_FOLDER: &str = ".vc-temp/version_control/";
//const ABS_TEMP_UPDATE_FOLDER: &'static str = format!("{}{}", BASE_DIRECTORY, TEMP_UPDATE_FOLDER);
const LEFTOVER_UPDATES_FILE: &str = "unfinished_updates.json";
const RECIPE_FILENAME: &str = "recipe.json";

/**
 * Goes through the components list and opens each version file, the contents of the
 * version file is then saved into a `BTreeMap` alongside the component name.
 * `BTreeMap` always contains NECOs version.
 */
pub fn init_component_versions(components: &[UpdateComponent]) -> BTreeMap<String, String> {
    let mut versions: BTreeMap<String, String> = BTreeMap::new();

    // The updater is always present in the versions BTreeMap
    versions.insert(APP_NAME.to_owned(), APP_VERSION.to_owned());

    info!("Initializing component versions...");

    for component in components {
        // This will prevent trying to fetch version file for NECO
        // This is needed because we're inserting permission data into the 'UpdateComponent' vector
        if component.name == APP_NAME {
            continue;
        }

        match File::open(&component.version_file_path) {
            Ok(mut file) => {
                let mut version = String::new();
                match file.read_to_string(&mut version) {
                    Ok(_) => {
                        versions.insert(component.name.to_owned(), version.trim().to_owned());
                    }
                    Err(e) => {
                        warn!(
                            "Failed to load version for component: '{}'",
                            &component.name
                        );
                        debug!("{}", e);
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Could not find/open version file for component: '{}'",
                    &component.name
                );
                debug!("{}", e);
            }
        }
    }

    info!("Loaded versions: {:?}", versions);
    info!("Component versions loaded.");

    versions
}

/**
 * Requests the update manifest from `Neutron Update Server` for the configured components.
 * When update manifest is received it is then parsed. If we succeed at parsing, the parsed
 *     update manifest is set by locking a mutex.
 *
 * NOTICE: Sends the changelogs through the component backhaul if there were update found.
 * NOTICE: Sends state updates through the component backhaul.
 *
 * Mutexes `SETTINGS`, `COMPONENT_VERSIONS`, `UPDATE_MANIFEST` are locked momentarily.
 */
pub fn request_update_manifest(
    mqtt_client: &AsyncClient, /*neutron_acc_user: &str,
                               mosquitto_client_user: &str,
                               mosquitto_client_pass: &str,
                               app_name: &str,
                               update_branch: &str,*/
) /*-> Option<structs::UpdateManifest>*/
{
    debug!("Requesting update manifest...");

    send_state(mqtt_client, "Looking for updates...");

    // Get variables from Settings struct
    let neutron_acc_user;
    let mosquitto_client_user;
    let mosquitto_client_pass;
    let app_name;
    let update_branch;
    if let Ok(settings) = SETTINGS.lock() {
        neutron_acc_user = settings.neutron_account_username.to_owned();
        mosquitto_client_user = settings.neutron_mqtt_client.username.to_owned();
        mosquitto_client_pass = settings.neutron_mqtt_client.password.to_owned();
        app_name = settings.application_name.to_owned();
        update_branch = settings.update_branch.to_owned();
    } else {
        error!("Could not lock SETTINGS mutex.");
        return;
    }

    // Get component names from Vec<Settings::UpdateComponent> Settings struct
    // Get component versions from Vec<Settings::UpdateComponent> Settings struct
    let components: Vec<String>;
    let versions: Vec<String>;
    if let Ok(comp_ver) = COMPONENT_VERSIONS.lock() {
        components = comp_ver.keys().cloned().collect();
        versions = comp_ver.values().cloned().collect();
    } else {
        error!("Could not acquire COMPONENT_VERSIONS mutex.");
        return;
    }

    if components.is_empty() || versions.is_empty() {
        warn!("Could not request update manifest with no components/versions loaded.");

        if let Ok(mut manifest) = UPDATE_MANIFEST.lock() {
            *manifest = None;
            return;
        }
    }

    let url = format!(
        "{protocol}{host}{port}/api/versioncontrol?neutronuser={neutron_username}&username={mqtt_username}&password={mqtt_password}&application={app}&branch={branch}&components={component_list}&versions={version_list}",
        protocol = NEUTRON_SERVER_PROTOCOL,
        host = NEUTRON_SERVER_IP,
        port = NEUTRON_SERVER_PORT,
        neutron_username = neutron_acc_user,
        mqtt_username = mosquitto_client_user,
        mqtt_password = mosquitto_client_pass,
        app = app_name,
        branch = update_branch,
        component_list = components.join(","),
        version_list = versions.join(",")
    );

    match reqwest::get(&url) {
        Ok(mut req) => {
            if let Ok(txt) = req.text() {
                let response: serde_json::Value = serde_json::from_str(&txt).unwrap_or_default();

                if response["result"] == true {
                    if response["msg"]["manifest"] != json!({})
                        && response["msg"]["manifest"] != serde_json::Value::Null
                    {
                        // Acquire the mutex lock, set the update manifest and exit the function
                        if let Ok(mut manifest) = UPDATE_MANIFEST.lock() {
                            *manifest =
                                serde_json::from_value(response["msg"]["manifest"].to_owned()).ok();

                            send_state(mqtt_client, "Found updates.");

                            // Prepare the changelogs and send them
                            let upds: Vec<structs::Update> = manifest
                                .clone()
                                .unwrap()
                                .list
                                .values()
                                .cloned()
                                .flatten()
                                //.flat_map(|updates| updates)
                                .collect();
                            let changelogs: String = upds
                                .iter()
                                .map(|update| {
                                    [update.changelog.to_owned(), "\r\n\r\n".to_owned()].concat()
                                })
                                .rev()
                                .collect();

                            send_changelogs(mqtt_client, &changelogs);

                            return;
                        } else {
                            error!("Couldn't lock and set UPDATE_MANIFEST mutex.");
                        }

                    //return serde_json::from_value(response["msg"]["manifest"].to_owned()).ok();
                    } else {
                        send_state(mqtt_client, "No updates were found.");
                    }
                } else if response["msg"] == serde_json::Value::Null {
                    error!("Update manifest response empty.");

                    send_state(mqtt_client, "Update manifest response empty.");
                } else {
                    error!("Server -> {}", response["msg"].as_str().unwrap_or_default());
                }
            }
        }
        Err(e) => {
            send_state(mqtt_client, "Could not reach Neutron server.");
            warn!("Could not reach Neutron server.");
            debug!("{}", e);
        }
    }

    if let Ok(mut manifest) = UPDATE_MANIFEST.lock() {
        *manifest = None;
    }

    //None
}

/**
 * This function calls `dload_and_verify_updates()`, `unpack_updates` then it
 *     checks if there are any NECO updates, if there are, install them
 *     (call to `get_recipes()` and `recipe_processor::cook()`) first and add
 *     others to the leftover update file.
 *
 * NOTICE: Sends state updates through the component backhaul.
 * NOTICE: The `update manifest` has to be correctly version sorted for this function to do its job correctly.
 * NOTICE: At the end of the function, we set the `UPDATE_MANIFEST` to `None` to prevent installation of already-installed updates.
 *
 * Mutexes `UPDATE_MANIFEST`, `SETTINGS`, `UPDATE_COMPONENTS` are locked momentarily.
 */
pub fn update_download_and_install(mqtt_client: &AsyncClient) {
    // info!("Starting update download & install.");
    // info!("UM: {:?}", &update_manifest.list);

    // Get update manifest
    let update_manifest: structs::UpdateManifest;
    if let Ok(manifest_option) = UPDATE_MANIFEST.lock() {
        if let Some(manifest) = manifest_option.clone() {
            update_manifest = manifest;
        } else {
            warn!("Cannot download and install - update manifest is empty.");
            return;
        }
    } else {
        error!("Could not lock UPDATE_MANIFEST mutex.");
        return;
    }

    // Set variables from the Settings struct
    let neutron_acc_user;
    let mosquitto_client_user;
    let mosquitto_client_pass;
    let app_name;
    let update_branch;
    if let Ok(settings) = SETTINGS.lock() {
        neutron_acc_user = settings.neutron_account_username.to_owned();
        mosquitto_client_user = settings.neutron_mqtt_client.username.to_owned();
        mosquitto_client_pass = settings.neutron_mqtt_client.password.to_owned();
        app_name = settings.application_name.to_owned();
        update_branch = settings.update_branch.to_owned();
    } else {
        error!("Could not lock SETTINGS mutex.");
        return;
    }

    // Get permission presets from Settings::UpdateComponents struct
    let permission_presets: Vec<UpdateComponent>;
    if let Ok(permissions) = UPDATE_COMPONENTS.lock() {
        permission_presets = permissions.clone();
    } else {
        error!("Could not lock UPDATE_COMPONENTS mutex.");
        return;
    }

    // Start downloading and verifying

    send_state(mqtt_client, "Starting update download & install.");

    // Contains path to the update archive and a server-side calculated checksum for the archive
    let verified_updates: BTreeMap<String, Vec<String>> = dload_and_verify_updates(
        update_manifest,
        &neutron_acc_user,
        &mosquitto_client_user,
        &mosquitto_client_pass,
        &app_name,
        &update_branch,
    );

    // info!("VERIFIED: {:?}", &verified_updates);

    // If downloading updates fail, just return, we don't need to waste cpu cycles on an empty list
    if verified_updates.is_empty() {
        return;
    }

    send_state(mqtt_client, "Updates downloaded and verified. Unpacking...");

    info!("Unpacking updates...");

    // Returns component name with a vector of file paths that have been extracted
    let mut inflated_updates: BTreeMap<String, Vec<String>> = unpack_updates(verified_updates);
    // info!("INFLATED: {:?}", inflated_updates);

    // NOTICE: THIS WILL SKIP UPDATING NECO IF WE'RE DEBUGGING
    // if cfg!(debug_assertions) {
    //     inflated_updates.remove(APP_NAME);
    // }

    let cookbook: Vec<serde_json::Value> = if inflated_updates.contains_key(APP_NAME) {
        send_state(mqtt_client, "Upgrading updater...");
        info!("Starting NECO upgrade...");

        let mut neco_updates: BTreeMap<String, Vec<String>> = BTreeMap::new();
        neco_updates.insert(APP_NAME.to_owned(), inflated_updates[APP_NAME].to_vec());

        // Remove the NECO recipe path from the update list we're going to install
        // after upgrading NECO
        inflated_updates.remove(APP_NAME);

        if !inflated_updates.is_empty() {
            if save_leftover_updates(&inflated_updates).is_err() {
                error!("Failed to save unfinished update list.");
                warn!("Automatic resuming will not happen, start the update search manually after NECO upgrade.");
                send_state(mqtt_client, "Failed to save the unfinished update list. Start the update search manually after the updater upgrade.");
            } else {
                info!("Other updates will be installed after upgrading NECO.");
                send_state(
                    mqtt_client,
                    "Other updates will be installed after updater is upgraded.",
                );
            }
        }

        get_recipes(neco_updates, &permission_presets)
    } else {
        info!("Fetching recipes...");

        get_recipes(inflated_updates, &permission_presets)
    };

    // info!("Cookbook: {:#}", serde_json::to_string(&cookbook).unwrap());

    info!("Updating component(s)...");
    send_state(mqtt_client, "Updating component(s)...");

    // Start cooking
    if recipe_processor::cook(&cookbook) {
        info!("Update download & install complete.");
        send_state(mqtt_client, "Update download & install complete.");
    } else {
        send_state(
            mqtt_client,
            "Some components failed to install. Please contact the support team.",
        );
    }

    // Remove the update manifest so we don't download the same updates again
    if let Ok(mut manifest_option) = UPDATE_MANIFEST.lock() {
        *manifest_option = None;
    }

    //info!("Cleaning up vc-temp folder.");
    //remove_dir_all(TMP_ROOT).is_ok();
}

/**
 * Fetches the recipes from the `update_paths.value()`(Vec) and groups them into
 *     component updates which then becomes a cookbook.
 * The cookbook is an array of components that have updates pending for installation.
 *     Each component has an `updates` key (array).
 *
 * Returns `Vec<>` containing every component that has updates pending.
 */
fn get_recipes(
    update_paths: BTreeMap<String, Vec<String>>,
    permission_presets: &[UpdateComponent],
) -> Vec<serde_json::Value> {
    let mut cookbook: Vec<serde_json::Value> = Vec::new();

    for component in update_paths {
        // Extract the component permissions for this component
        let component_perms: Vec<&UpdateComponent> = permission_presets
            .iter()
            .filter(|x| x.name == component.0)
            .collect();

        let mut component_in_vec: serde_json::Value =
            serde_json::from_str("{}").unwrap_or_default();

        // Values in root of component
        component_in_vec["component"] = serde_json::value::Value::String(component.0.to_owned());
        component_in_vec["restart_command"] =
            serde_json::value::Value::String(component_perms[0].restart_command.to_owned());

        let mut restart_comp = false;

        // Since the updates are ordered by version, from old to new, we can set `final_version` on
        //     every iter and get the last version by the end
        let mut final_version = String::new();
        // This is going to contain all the updates we are able to extract from the paths for that component
        let mut recipes: Vec<serde_json::Value> = Vec::new();

        // For every recipe path in a recipe vector
        for recipe_path in component.1 {
            // Open the recipe at the `recipe_path` and try to parse it
            match File::open([&recipe_path, RECIPE_FILENAME].concat()) {
                Ok(mut file) => {
                    let mut recipe = String::new();
                    match file.read_to_string(&mut recipe) {
                        Ok(_) => {
                            if let Ok(recipe_json) = serde_json::from_str(&recipe) {
                                let parsed_json: Vec<serde_json::Value> = recipe_json;

                                // For every command block in a recipe
                                for mut instruction in parsed_json {
                                    if instruction["restart"] == true {
                                        //|| component.0 == "BlackBox"
                                        restart_comp = true;
                                    }

                                    // Append the temp update folder to the command array
                                    instruction["absolute_update_path"] =
                                        serde_json::value::Value::String(recipe_path.to_string());

                                    // This will work because the recipes are ordered from oldest to newest
                                    if instruction["version"] != serde_json::Value::Null {
                                        final_version = instruction["version"]
                                            .as_str()
                                            .unwrap_or_default()
                                            .to_string();
                                    }

                                    // Check if permission overrides exist for the copy command
                                    // If they don't, insert the ones from settings for that component
                                    if instruction["type"] == "copy" && !component_perms.is_empty()
                                    {
                                        if instruction["permission_user"] == serde_json::Value::Null
                                        {
                                            instruction["permission_user"] =
                                                serde_json::value::Value::String(
                                                    component_perms[0].permission_user.to_owned(),
                                                );
                                        }

                                        if instruction["permission_group"]
                                            == serde_json::Value::Null
                                        {
                                            instruction["permission_group"] =
                                                serde_json::value::Value::String(
                                                    component_perms[0].permission_group.to_owned(),
                                                );
                                        }

                                        if instruction["file_permissions"]
                                            == serde_json::Value::Null
                                        {
                                            instruction["file_permissions"] =
                                                serde_json::value::Value::String(
                                                    component_perms[0].file_permissions.to_owned(),
                                                );
                                        }
                                    }

                                    // Add instruction to recipes
                                    recipes.push(instruction);
                                }
                            } else {
                                warn!("Could not parse recipe.");
                                debug!("{}", recipe);
                            }
                        }
                        Err(e) => {
                            warn!("Failed to load recipe from file. Path: '{}'", &recipe_path);
                            debug!("{}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Could not find/open recipe. Path: '{}'", &recipe_path);
                    debug!("{}", e);
                }
            }
        }

        if final_version.is_empty() {
            error!("Could not find any version numbers in recipes for component: {}. Skipping component...", &component.0);
            continue;
        }

        // Set values in the json array element
        component_in_vec["restart"] = serde_json::Value::Bool(restart_comp);
        component_in_vec["final_version"] = serde_json::value::Value::String(final_version);
        component_in_vec["updates"] = serde_json::Value::Array(recipes);
        // Append everything we just set to the main array
        cookbook.push(component_in_vec);
    }

    cookbook
}

/**
 * Unzipps the downloaded update files so that they can be further processed.
 * Files are unzipped to a folder named `<zipfile-name>-extracted` and if it was
 *     successful, the zip file is removed.
 *
 * NOTICE: The client needs to have `unzip` installed for this function to work.
 *
 * Returns `BTreeMap` with component name as the key and the extracted folder path
 *     as the value if successful.
 */
fn unpack_updates(
    verified_updates: BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Vec<String>> {
    let mut inflated_updates: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for component in verified_updates {
        let mut unzipped_updates: Vec<String> = Vec::new();

        // For every update in the vector of a component
        for update in component.1 {
            let extracted_folder_name = [&update, "-extracted"].concat();

            match Command::new("unzip")
                .arg(&update)
                .arg("-d")
                .arg(&extracted_folder_name)
                .output()
            {
                Ok(res) => {
                    // Check if error output is empty, if it's not, stdout error and skip this loop count
                    if !String::from_utf8_lossy(&res.stderr).is_empty() {
                        error!(
                            "Could not extract update zip-file. {}",
                            String::from_utf8_lossy(&res.stderr)
                        );
                        continue;
                    }
                }
                Err(e) => error!("Could not execute 'unzip'. Is it installed? {}", e),
            }

            // If we're here, that means that we have no critical errors

            if remove_file(&update).ok().is_none() {
                warn!("Could not remove extracted zip file. {}", &update);
            }

            // Push the extracted update path to vec
            unzipped_updates.push([&extracted_folder_name, "/"].concat());
        }
        inflated_updates.insert(component.0, unzipped_updates);
    }

    inflated_updates
}

/**
 * Downloads and hash-checks the update files using the provided update manifest.
 * Removes the version control temporary directory and recreates it, then it goes through
 *     the update manifest requesting the update files.
 * When the download is complete, compare the hash to the one in the update manifest, if
 *     it matches it is considered good. If it's bad, it gets deleted before returning.
 *
 * Returns empty `BTreeMap` if there aren't any good* updates to install.
 * **Good updates - the updates that passed the hash validation.
 *
 * Returns `BTreeMap` with component name as the key and the confirmed update list (`Vec`) as the value.
 */
fn dload_and_verify_updates(
    update_manifest: structs::UpdateManifest,
    neutron_acc_user: &str,
    mosquitto_client_user: &str,
    mosquitto_client_pass: &str,
    app_name: &str,
    update_branch: &str,
) -> BTreeMap<String, Vec<String>> {
    info!("Initiating Update Download and Checksum Validation.");

    let temp_folder = get_temp_folder_path();
    if let Err(e) = remove_dir_all(&temp_folder) {
        warn!("Could not remove root temporary folder. {}", e)
    }

    if create_dir_all(&temp_folder).is_ok() {
        //let mut unverified_updates: BTreeMap<String, String> = BTreeMap::new();
        let mut verified_updates: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut dirty_updates: Vec<String> = Vec::new();

        for component in update_manifest.list {
            let tmp_dir_component_path = [temp_folder.to_owned(), component.0.to_owned()].concat();

            let mut component_updates: Vec<String> = Vec::new();

            // Try to create a temporary component folder
            if create_dir(&tmp_dir_component_path).is_ok() {
                for update in component.1 {
                    // We don't need the .zip extension at the end because 'unzip' command automatically does that
                    let file_path = format!("{}/{}", tmp_dir_component_path, &update.version);

                    let url = format!(
                        "{}{}{}/version_control/download?neutronuser={}&username={}&password={}&application={}&branch={}&component={}&version={}",
                        NEUTRON_SERVER_PROTOCOL,
                        NEUTRON_SERVER_IP,
                        NEUTRON_SERVER_PORT,
                        neutron_acc_user,
                        mosquitto_client_user,
                        mosquitto_client_pass,
                        app_name,
                        &update_branch,
                        &component.0,
                        &update.version
                    );

                    match reqwest::get(&url) {
                        Ok(mut response) => {
                            if let Ok(mut file) = File::create(&file_path) {
                                if copy(&mut response, &mut file).is_ok() {
                                    //info!("{} : {}", &component.0, &update.version);
                                    //info!("UNVF: {:?}", &unverified_updates);
                                    if security::compare_hash(&file_path, &update.checksum).is_ok()
                                    {
                                        component_updates.push(file_path);
                                    } else {
                                        warn!("Update file verification failed. {}", &file_path);
                                        dirty_updates.push(file_path);
                                    }
                                }
                            } else {
                                error!("Could not create file after downloading.");
                            }
                        }
                        Err(e) => {
                            error!(
                                "Could not fetch update package. Component: {}, Version: {}",
                                component.0, update.version
                            );
                            // Error message is written in debug because it contains sensitive information
                            debug!("{}", e);
                        }
                    }
                }

                // If we got some files to install, append them to the component name
                if !component_updates.is_empty() {
                    // 'component.0' is components name
                    verified_updates.insert(component.0, component_updates);
                }
            } else {
                error!("Could not create temporary folder structure.");
            }
        }

        info!("Update Download and Verification Complete.");

        info!("Purging dirty update files...");

        // Purge bad (unverified) update files
        // We're doing this here so there is no chance of executing this file by accident later
        for file_path in dirty_updates {
            if remove_file(&file_path).is_err() {
                warn!("Could not remove dirty update. Path: {}", file_path);
            }
        }

        return verified_updates;
    } else {
        error!("Could not create a new root temporary folder.");
    }

    // If we're here, nothing good happened.
    // Just return an empty list.
    BTreeMap::new()
}

/**
 * Saves the provided update manifest as a leftover update manifest.
 *
 * Returns `Ok(())` if successful.
 */
fn save_leftover_updates(
    update_manifest: &BTreeMap<String, Vec<String>>,
) -> Result<(), std::io::Error> {
    let unfinished_updates_file =
        [get_temp_folder_path(), LEFTOVER_UPDATES_FILE.to_owned()].concat();

    let mut file = File::create(unfinished_updates_file)?;
    file.write_all(&serde_json::to_string(&update_manifest)?.as_bytes())?;

    Ok(())
}

/**
 * Tries to open the unfinished updates update manifest, if the file cannot be
 *     opened (because it doesn't exist or is corrupted) we just return the function.
 * If we find the leftover update manifest, try to parse it and call `install_leftover_updates()` on that manifest.
 */
pub fn find_leftover_updates(permission_presets: &[UpdateComponent]) {
    let unfinished_updates_file =
        [get_temp_folder_path(), LEFTOVER_UPDATES_FILE.to_owned()].concat();

    let mut contents = String::new();

    let mut file: File;
    if let Ok(opened_file) = File::open(unfinished_updates_file) {
        file = opened_file;
    } else {
        warn!("Could not find/open leftover updates file.");
        return;
    }

    if file.read_to_string(&mut contents).is_err() {
        error!("Could not convert read file.");
        return;
    }

    if let Ok(json) = serde_json::from_str(&contents) {
        let update_list: BTreeMap<String, Vec<String>> = json;

        if !update_list.is_empty() {
            info!("Found leftover updates.");
            install_leftover_updates(update_list, permission_presets);
        }
    } else {
        error!("Could not convert leftover update list from JSON.");
    }
}

/**
 * Fetches the recipes from the paths found in the cookbook then cooks
 *     the updates and tries to remove the temporary folder.
 * If removing the base version control temporary folder fails, we
 *     at least try to remove the leftover update manifest so the same updates don't get installed again.
 */
fn install_leftover_updates(
    update_list: BTreeMap<String, Vec<String>>,
    permission_presets: &[UpdateComponent],
) {
    let cookbook = get_recipes(update_list, permission_presets);

    info!("Updating component(s)...");

    // Start cooking
    recipe_processor::cook(&cookbook);

    info!("Update installation complete.");

    debug!("Removing temporary update folder...");
    if remove_dir_all(get_temp_folder_path()).is_err() {
        error!("Could not remove temporary update folder.");

        if remove_file([get_temp_folder_path(), LEFTOVER_UPDATES_FILE.to_owned()].concat()).is_err()
        {
            error!("Could not remove leftover update list. It's possible it will try to install the same updates again.");
        }
    }
}

/**
 * Concatenates the `BASE_DIRECTORY` and `TEMP_UPDATE_FOLDER`.
 */
fn get_temp_folder_path() -> String {
    [BASE_DIRECTORY, TEMP_UPDATE_FOLDER].concat()
}

/**
 * Loops through the `UpdateComponent` vector (obtained by locking the `UPDATE_COMPONENTS` mutex)
 * determines the component states by running commands using the service/container name.
 * The NECO username, used to log into the component network, is used as an ID.
 * The `Main` struct is then converted to a JSON-formatted `String`.
 * Mutexes `SETTINGS`, `COMPONENT_VERSIONS`, `UPDATE_COMPONENTS` are locked momentarily.
 */
pub fn get_component_states() -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct Main {
        id: String,
        components: Vec<Component>,
    }

    #[derive(Serialize)]
    struct Component {
        component: String,
        version: String,
        state: bool,
    }

    let mut neco_components = Main {
        id: String::new(),
        components: Vec::new(),
    };

    if let Ok(settings) = SETTINGS.lock() {
        neco_components.id = settings.component_mqtt_client.username.to_owned();
    } else {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            "Could not lock SETTINGS mutex.",
        )));
    }

    let component_versions;
    if let Ok(versions) = COMPONENT_VERSIONS.lock() {
        component_versions = versions.clone();
    } else {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            "Could not lock COMPONENT_VERSIONS mutex.",
        )));
    }

    let update_components = UPDATE_COMPONENTS.lock().ok();
    if update_components.is_none() {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            "Could not lock UPDATE_COMPONENTS mutex.",
        )));
    }

    for comp in update_components.unwrap().clone() {
        // This way we skip adding NECO to the vector
        if comp.name == APP_NAME {
            continue;
        }

        let ver = component_versions
            .get(&comp.name)
            .unwrap_or(&String::from("Unknown"))
            .to_string();

        if let Some(name) = comp.container_name {
            neco_components.components.push(Component {
                component: [&comp.name, " - Container"].concat(),
                version: ver.to_string(),
                state: fetch_container_state(&name),
            })
        }

        if let Some(name) = comp.service_name {
            neco_components.components.push(Component {
                component: [&comp.name, " - Service"].concat(),
                version: ver.to_string(),
                state: fetch_service_state(&name),
            })
        }
    }

    serde_json::to_string(&neco_components)
}

/**
 * Executes the `systemctl is-active` command and checks if the command returns a non-zero code.
 * Returns false if the command fails to run (also prints out the error), writes to stderr (also prints) or returns a non-zero code.
 * The `name` parameter is  the name of the service (usually including '.service' at the end).
 */
fn fetch_service_state(name: &str) -> bool {
    let command = format!("systemctl is-active {}", name);

    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(res) => {
            if !res.stderr.is_empty() {
                error!(
                    "Failed to get service state. {}",
                    String::from_utf8_lossy(&res.stderr)
                );
                return false;
            }

            return res.status.success();
        }
        Err(e) => error!("Command Digest: Could not execute command. {}", e),
    }

    false
}

/**
 * Executes the `docker ps` command with some arguments that try to get the ID of the container.
 * If the container is UP or PAUSED this function will return `true`.
 * If the command outputs to stderr, the function returns `false` and an error message is printed.
 *
 * Will return true even if the container is paused (techically it is still running).
 * The `name` parameter is the name of the docker container.
 */
fn fetch_container_state(name: &str) -> bool {
    let id_command = format!("docker ps -qf \"name=^{}$\"", name);

    match execute_shell(&id_command) {
        Ok(out) => !out.is_empty(),
        Err(e_res) => {
            error!("Failed to get container ID. >> {}", e_res.trim());
            false
        }
    }
}

/**
 * The input data gets parsed to a struct
 * then we split the `component` field in the `JSONIn` struct so that we can separate the component name from the type.
 * If the split vector doesn't have 2 elements; `Err` is returned.
 * After that, we loop through the `UpdateComponent` vector until we find the component with the matching name.
 * If such component cannot be found, `Err` is returned.
 * Then we compare the component type from the request and fetch the log.
 * The `JSONOut` struct is then converted to a `String`.
 */
pub fn get_component_log(data: &str) -> Result<String, serde_json::Error> {
    #[derive(Serialize)]
    struct JSONOut {
        request: String,
        data: String,
    }

    // {'id': 'test_neco_aio', 'request': '<random id>', 'component': 'BlackBox - Service'}
    #[derive(Deserialize)]
    struct JSONIn {
        request: String,
        component: String,
    }

    // Parse the json to a struct
    let parsed_json: JSONIn;
    match serde_json::from_str(&data.replace("'", "\"")) {
        Ok(result) => parsed_json = result,
        Err(e) => {
            error!("Could not parse get_component_log data. {}", e);
            return Err(e);
        }
    }

    // Split the data.component by ' - ' so that we can separate the component name from the component type
    let component_name;
    let comp_type;
    let split: Vec<&str> = parsed_json.component.split(" - ").collect();
    if split.len() != 2 {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            format!(
                "Failed splitting component, no component type specified. '{}'",
                parsed_json.component
            ),
        )));
    }
    component_name = split[0];
    comp_type = split[1];

    // Lock the UpdateComponents mutex so we can extract the component that matches the component name in the parsed JSON
    let update_components: Vec<UpdateComponent>;
    if let Ok(components) = UPDATE_COMPONENTS.lock() {
        update_components = components
            .clone()
            .into_iter()
            .filter(|x| x.name == component_name)
            .collect();
    } else {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            "Could not lock UPDATE_COMPONENTS mutex.",
        )));
    }

    let mut ret_data = JSONOut {
        request: parsed_json.request,
        data: String::new(),
    };

    // Get the component log - it is either a service or a container, we have a variable for the type
    // Save the stdout/stderr to the main struct
    if let Some(component) = update_components.get(0) {
        match comp_type {
            "Service" => {
                if let Some(n) = &component.service_name {
                    ret_data.data = fetch_service_log(&n);
                }
            }
            "Container" => {
                if let Some(n) = &component.container_name {
                    ret_data.data = fetch_container_log(&n);
                }
            }
            _ => {
                return Err(serde_json::Error::io(Error::new(
                    ErrorKind::Other,
                    format!(
                        "Could not determine the component type. '{}'",
                        component_name
                    ),
                )));
            }
        }

        if ret_data.data.is_empty() {
            return Err(
                serde_json::Error::io(
                    Error::new(
                        ErrorKind::Other,
                        format!("Failed to fetch the log. Component: {} | Type requested: {} | <type>.name == None", &component.name, comp_type)
                    )
                )
            );
        }
    } else {
        return Err(serde_json::Error::io(Error::new(
            ErrorKind::Other,
            format!("Could not find a component named: '{}'", component_name),
        )));
    }

    // Convert the main struct to String
    serde_json::to_string(&ret_data)
}

/**
 * Executes the `journalctl -u` command and returns the output (stdout/stderr).
 * The `name` parameter is  the name of the service (usually including '.service' at the end).
 */
fn fetch_service_log(name: &str) -> String {
    let command = format!("journalctl --no-pager -u {}", name);

    match execute_shell(&command) {
        Ok(res) => res,
        Err(e_res) => format!("Failed to get service log. >> {}", e_res.trim()),
    }
}

/**
 * Executes the `docker logs` command and returns the output (stdout/stderr).
 * The `name` parameter is the name of the docker container.
 */
fn fetch_container_log(name: &str) -> String {
    let command = format!("docker logs -t {}", name);

    match execute_shell(&command) {
        Ok(res) => res,
        Err(e_res) => format!("Failed to get container log. >> {}", e_res.trim()),
    }
}

/**
 * Executes a given command and returns the output (stdout/stderr) as a `Result`.
 */
fn execute_shell(command: &str) -> Result<String, String> {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(res) => {
            return if res.stderr.is_empty() {
                Ok(String::from_utf8_lossy(&res.stdout).into())
            } else {
                Err(String::from_utf8_lossy(&res.stderr).into())
            }
        }
        Err(e) => error!("Command Digest: Could not execute command. {}", e),
    }

    Err(String::from("Internal Error"))
}

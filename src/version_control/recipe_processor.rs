use fs_extra;

use std::fs::copy;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::Ordering;

use crate::{APP_NAME, COMPONENT_VERSIONS, RESTART_NECO, UPDATE_COMPONENTS};

use super::find_leftover_updates;
use super::security::set_file_permissions;

const DEV_DIR: &str = "/home/system/.neco_test_dir/";

/**
 * Reads through the cookbook and executes (digests) the commands.
 *
 * NOTICE: When in debug, `restart` command will still be executed.
 * NOTICE: When in debug, `copy` instructions are directed into a special folder.
 */
pub fn cook(cookbook: &[serde_json::Value]) -> bool {
    info!("Heating up the oven...");

    if cfg!(debug_assertions) && !Path::new(DEV_DIR).exists() {
        info!("DEV: Creating dev directory ");
        if let Err(e) = std::fs::create_dir(DEV_DIR) {
            error!("Failed to create dev directory. {}", e);
        }
    }

    let mut is_succesfull = true;

    for component in cookbook {
        //info!("COMPONENT NAME: {}", component["component"]);

        /*if component["component"] == serde_json::value::Value::Null {
            error!("YOU GOT IT");
        }*/

        let mut erroneous: bool = false;

        let comp_recipes: Vec<serde_json::Value> =
            serde_json::value::from_value(component["updates"].clone()).unwrap_or_default();

        for recipe in comp_recipes {
            //info!("---{}", recipe["type"]);

            match recipe["type"].as_str().unwrap_or_default() {
                "copy" => {
                    //info!("Exec copy.");
                    if digest_copy(
                        &recipe["absolute_update_path"].as_str().unwrap_or_default(),
                        &recipe["file_path"].as_str().unwrap_or_default(),
                        if cfg!(debug_assertions) {
                            &DEV_DIR
                        } else {
                            &recipe["destination"].as_str().unwrap_or_default()
                        },
                        &recipe["permission_user"].as_str().unwrap_or_default(),
                        &recipe["permission_group"].as_str().unwrap_or_default(),
                        &recipe["file_permissions"].as_str().unwrap_or_default(),
                    )
                    .is_err()
                    {
                        erroneous = true;
                    }
                }
                "copy_dir" => {
                    if !cfg!(debug_assertions)
                        && digest_copy_dir(
                            &recipe["folder_path"].as_str().unwrap_or_default(),
                            &recipe["destination"].as_str().unwrap_or_default(),
                        )
                        .is_err()
                    {
                        erroneous = true;
                    }
                }
                "run_command" => {
                    //info!("Exec command.");
                    if !cfg!(debug_assertions) {
                        digest_run(&recipe["command"].as_str().unwrap_or_default());
                    }
                }
                "run_script" => {
                    //info!("Exec script.");
                    if !cfg!(debug_assertions) {
                        digest_script(
                            &recipe["absolute_update_path"].as_str().unwrap_or_default(),
                            &recipe["file_path"].as_str().unwrap_or_default(),
                        );
                    }
                }
                _ => error!("Unknown recipe command type. Type: {}", &recipe["type"]),
            }
        }

        if !restart_set_component_version(
            serde_json::from_value(component["restart"].clone()).unwrap_or_default(),
            component["component"].as_str().unwrap_or_default(),
            component["restart_command"].as_str().unwrap_or_default(),
            component["final_version"].as_str().unwrap_or_default(),
        ) {
            erroneous = true;
        }

        let status = format!(
            "Component: {} Upgrade: {}",
            &component["component"],
            if erroneous { "FAILED" } else { "SUCCESSFUL" }
        );

        info!("{}", &status);

        is_succesfull = !erroneous;
    }

    info!("Dinner's ready!");

    is_succesfull
}

/**
 * Checks if `restart` is true.
 * If it is, check if the `component_name` is the same as `APP_NAME`.
 *     That means if NECO need to restart, just set the `RESTART_NECO` `AtomicBool` to true so we can escape the main loop.
 *     If the `component_name` is not the same as `APP_NAME`, run the restart command for that component with `digest_run()`.
 *
 * Returns `bool` true if no errors raised.
 */
fn restart_set_component_version(
    restart: bool,
    component_name: &str,
    restart_command: &str,
    version: &str,
) -> bool {
    if component_name == APP_NAME {
        if restart {
            info!("Requesting NECO restart...");
            RESTART_NECO.store(true, Ordering::SeqCst);
        } else {
            // Install leftover updates if we don't need to restart NECO
            // This will make the NECO upgrade status show up last, it will actually only print the component upgrade success after everything has finished
            if let Ok(data) = UPDATE_COMPONENTS.lock() {
                find_leftover_updates(&data);
            } else {
                error!("Could not acquire lock for update_components object. Skipping leftover updates...");
                return false;
            }
        }

        // This actually isn't necessary, but it doesn't hurt
        // We don't need to update the NECO version number when we're restarting NECO
        // But it stops Clippy from complaining about collapsable if's
        if let Ok(mut ver) = COMPONENT_VERSIONS.lock() {
            if ver
                .insert(APP_NAME.to_owned(), version.to_owned())
                .is_none()
            {
                warn!("Could not find NECO version number to update?? This is a bug bois!");
                return false;
            }
        }
    } else {
        if restart {
            warn!("Restarting {} component...", component_name);
            //digest_run(&component["restart_command"].as_str().unwrap_or_default());
            digest_run(restart_command);
        }

        // SET NEW COMPONENT VERSION
        if let Ok(mut ver) = COMPONENT_VERSIONS.lock() {
            if ver
                .insert(component_name.to_owned(), version.to_owned())
                .is_none()
            {
                warn!("Could not find component version to update?? This is a bug if I've ever seen one...");
                return false;
            }
        }
    }

    true
}

// NOTE: This may not work. It may refuse to copy and overwrite root owned files.
//       Without the second 'set_file_permissions' the file at the destination would still be owned by root.
//  ->Maybe switch to fs_extra?
/**
 * Processes the `copy` command in the update cookbook.
 * Before copying the file, it sets the file permissions to root-owned then copies the file and
 *     tries setting the permissions provided by the cookbook.
 * This is in case we fail to set the correct permissions afterwards, the file is still root-owned.
 *
 * Returns `Ok(())` if the permission setting and file copying was successful.
 */
fn digest_copy(
    absolute_update_path: &str,
    file_path: &str,
    destination: &str,
    permission_user: &str,
    permission_group: &str,
    file_permissions: &str,
) -> Result<(), ()> {
    // Update file location
    let file_loc = [absolute_update_path, file_path].concat();
    // Final destination
    let cp_destination = [destination, file_path].concat();

    // Try to set file permissions before copying, if we fail, return error
    // That way we don't copy a file with bad permissions
    if set_file_permissions(&file_loc, "root", "root", file_permissions).is_err() {
        return Err(());
    }

    if let Err(e) = copy(&file_loc, &cp_destination) {
        error!("Failed to digest copy command. {}", e);
        return Err(());
    }

    if set_file_permissions(
        &cp_destination,
        permission_user,
        permission_group,
        file_permissions,
    )
    .is_err()
    {
        return Err(());
    }

    debug!("Copied: from {} to {}.", &file_loc, &destination);
    Ok(())
}

/**
 * Processes the `copy directory` command in the update cookbook.
 *
 * Returns `Ok(u64)` if the copy was successful.
 */
pub fn digest_copy_dir(
    dir_loc: &str,
    dir_destination: &str,
) -> Result<u64, fs_extra::error::Error> {
    let mut cpy_options = fs_extra::dir::CopyOptions::new();

    cpy_options.copy_inside = true;
    //cpy_options.overwrite = true;

    fs_extra::dir::copy(dir_loc, dir_destination, &cpy_options)
}

/**
 * Processes the `run` command in the update cookbook.
 * The provided command is ran as a root user
 */
fn digest_run(command: &str) {
    match Command::new("sh").arg("-c").arg(command).output() {
        Ok(res) => {
            if !res.stderr.is_empty() {
                error!(
                    "Failed to digest run command. >> {}",
                    String::from_utf8_lossy(&res.stderr)
                );
            }
        }
        Err(e) => error!("Command Digest: Could not execute command. {}", e),
    }
}

/**
 * Processes the `script` command in the update cookbook.
 * The script is run as a root user.
 */
fn digest_script(absolute_update_path: &str, script_path: &str) {
    //match Command::new(["/home/system/Desktop/", "test.sh"].concat()).output()
    match Command::new([absolute_update_path, script_path].concat()).output() {
        Ok(res) => {
            if res.stderr.is_empty() {
                debug!(
                    "Script exec success: {}",
                    script_path //String::from_utf8_lossy(&res.stdout)
                );
            } else {
                error!(
                    "Failed to digest script command. >> {}",
                    String::from_utf8_lossy(&res.stderr)
                );
            }
        }
        Err(e) => error!("Script Digest: Could not execute command. {}", e),
    }
}

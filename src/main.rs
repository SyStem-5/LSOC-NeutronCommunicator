#![warn(unused_extern_crates)]

#[macro_use]
extern crate log;

use env_logger;
use futures::stream::Stream;

use serde_json;

use futures::Future;
use hyper::{Client, Uri};
use tokio_core::reactor::Core;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::Read;

mod settings;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

// Displayed when the user starts NECO with an unknown argument
const START_COMMAND_INFO: &str =
r#"Available Commands:
    debug -> Log more detailed messages when running.
    gen_settings -> Generate default settings file.
    -d <debug> -> Start NECO without user input capability. Usually used when being ran as a service. Can be run in debug mode ex. "-d debug".
"#;

//https://hyper.rs/guides/client/configuration/
const NEUTRON_UPDATE_SERVER_URL: &str = "http://127.0.0.1:8000";

fn main() {
    let mut daemon_mode = false;

    check_if_root();

    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        let _cmnd = &args[1];

        match &_cmnd[..] {
            "debug" => {
                init_logging("debug");
            }
            "gen_settings" => {
                init_logging("info");
                match settings::write_default_settings() {
                    Ok(()) => {}
                    Err(e) => {
                        error!("Could not write default settings to disk. {}", e);
                        std::process::exit(1);
                    }
                }
                std::process::exit(0);
            }
            "-d" => {
                if args.contains(&"debug".to_string()) {
                    init_logging("debug");
                } else {
                    init_logging("info");
                }
                daemon_mode = true;
            }
            _ => {
                // Print all commands
                println!("{}", START_COMMAND_INFO);
                std::process::exit(0);
            }
        }
    } else {
        init_logging("info");
    }

    // Load settings file
    // If the settings returns Err, we exit
    let settings;
    match settings::init() {
        Ok(res) => settings = res,
        Err(_) => std::process::exit(1),
    }

    println!();
    info!("Neutron Communicator V{}::Startup", APP_VERSION);

    info!("Running in daemon mode: {}", daemon_mode);

    let _versions = fetch_component_versions(&settings["update_components"]);

    let mut components: Vec<String> = Vec::new();
    let mut versions: Vec<String> = Vec::new();

    for component in _versions {
        components.push(component.0);
        versions.push(component.1);
    }

    let mut core = Core::new().unwrap();
    let client = Client::new();
    let url: Uri = format!(
        "{}/api/versioncontrol?username={}&password={}&application={}&branch={}&components={}&versions={}",
        NEUTRON_UPDATE_SERVER_URL,
        settings["mosquitto_client"]["username"]
            .as_str()
            .unwrap_or_default(),
        settings["mosquitto_client"]["password"]
            .as_str()
            .unwrap_or_default(),
        settings["app_name"].as_str().unwrap_or_default(),
        settings["update_branch"].as_str().unwrap_or_default(),
        components.join(","),
        versions.join(",")
    )
    .parse()
    .unwrap();

    let request = client
        .get(url)
        .and_then(|res| {
            info!("status {:?}", res);
            res.into_body().concat2()
        })
        .map(|body| {
            info!("Body {}", String::from_utf8(body.to_vec()).unwrap());
        })
        .map_err(|err| error!("error {}", err));

    core.run(request).unwrap();

    // loop {
    //     std::thread::sleep(std::time::Duration::from_secs(1));
    // }
}

/**
 * Checks if app is root.
 * If the app is not root, tell the user and exit the program.
 */
fn check_if_root() {
    if let Ok(user) = env::var("USER") {
        if user != "root" {
            eprintln!("This application need to be ran as root.");
            std::process::exit(1);
        }
    } else {
        eprintln!("Could not find user.\n");
        std::process::exit(1);
    }
}

/**
 * Initializes logging with specified detail:
 * ``` filter: 'info', 'warn', 'debug', 'trace' ```
 */
fn init_logging(filter: &str) {
    let env = env_logger::Env::default().filter_or("RUST_LOG", filter);
    env_logger::init_from_env(env);
}

/**
 * Goes through components and opens each version file, the contents of the
 * version file is then saved into a HashMap alongside the component name.
 *
 * HashMap always contains "neutroncommunicator","<app_version>".
 */
fn fetch_component_versions(components: &serde_json::Value) -> HashMap<String, String> {
    let mut versions: HashMap<String, String> = HashMap::new();

    // The updater is always present in the versions hashmap
    versions.insert("neutroncommunicator".to_owned(), APP_VERSION.to_owned());

    info!("Fetching component versions...");

    let components: HashMap<String, String> =
        serde_json::from_value(components.clone()).unwrap_or_default();

    for component in components {
        match File::open(component.1) {
            Ok(mut file) => {
                let mut version = String::new();
                match file.read_to_string(&mut version) {
                    Ok(_) => {
                        versions.insert(component.0, version);
                    }
                    Err(e) => warn!(
                        "Failed to load version for component '{}'. {}",
                        component.0, e
                    ),
                }
            }
            Err(e) => warn!(
                "Failed to load version for component '{}'. {}",
                component.0, e
            ),
        }
    }

    info!("{:?}", versions);

    versions
}

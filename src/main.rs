#![warn(unused_extern_crates)]

// #![deny(clippy::pedantic)]
// #![deny(clippy::all)]

use std::collections::BTreeMap;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
//use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use paho_mqtt as mqtt;

use lazy_static::lazy_static;

use clap::{App, Arg, SubCommand};

#[macro_use]
extern crate log;
use env_logger;

#[macro_use]
extern crate serde_derive;

mod version_control;
use crate::version_control::{find_leftover_updates, init_component_versions};
use version_control::structs::UpdateManifest;

mod mqtt_connection;

mod settings;

mod encryption_certificates;

//use encryption_certificates::structs::CertRenewal;

mod remote_management;

lazy_static! {
    static ref SETTINGS: Mutex<settings::structs::Settings> = Mutex::default();
    static ref UPDATE_COMPONENTS: Mutex<Vec<settings::structs::UpdateComponent>> = Mutex::default();
    static ref COMPONENT_VERSIONS: Mutex<BTreeMap<String, String>> = Mutex::default();
    //static ref COMPONENT_MQTT_OWN_TOPIC: Mutex<String> = Mutex::default();
    static ref UPDATE_MANIFEST: Mutex<Option<UpdateManifest>> = Mutex::default();
}

const APP_NAME: &str = "NeutronCommunicator";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BASE_DIRECTORY: &str = "/etc/NeutronCommunicator/";

const NEUTRON_SERVER_IP: &str = "127.0.0.1";
const NEUTRON_SERVER_PORT: &str = ":8002";
#[cfg(feature = "SECURE")]
const NEUTRON_SERVER_PROTOCOL: &str = "https://";
#[cfg(feature = "INSECURE")]
const NEUTRON_SERVER_PROTOCOL: &str = "http://";

static RESTART_NECO: AtomicBool = AtomicBool::new(false);

fn main() {
    check_if_root();
    process_cli_args();

    // Try to load the settings file
    let settings = if let Ok(res) = settings::init() {
        // Save Settings struct to a static ref
        if let Ok(mut settings_struct) = SETTINGS.lock() {
            *settings_struct = res.clone();
        }

        // Save UpdateComponents struct to a static ref
        if let Ok(mut up_comps) = UPDATE_COMPONENTS.lock() {
            *up_comps = res.update_components.clone();
        }

        // Save our mqtt topic so we can publish to it
        // if let Ok(mut own_topic) = COMPONENT_MQTT_OWN_TOPIC.lock() {
        //     *own_topic = format!(
        //         "{}/{}",
        //         mqtt_connection::component_mqtt::ROOT_NECO_TOPIC,
        //         res.component_mqtt_client.username
        //     );
        // }

        // Get component versions and save them to a static ref
        if let Ok(mut ver) = COMPONENT_VERSIONS.lock() {
            *ver = init_component_versions(&res.update_components);
        }

        res
    } else {
        std::process::exit(1);
    };

    // Check for unfinished updates
    find_leftover_updates(&settings.update_components);

    info!("Neutron Communicator::Startup V{}", APP_VERSION);
    println!();

    let component_mqtt =
        mqtt_connection::init_component_mqtt(&settings.component_mqtt_client).unwrap();

    // let neutron_mqtt =
    //     mqtt_connection::init_neutron_mqtt(&settings.neutron_mqtt_client).unwrap();


    let mut cert_watchdog_thread: Option<std::thread::JoinHandle<()>> = None;
    match encryption_certificates::init(&settings.certificates) {
        Ok(thread) => {
            cert_watchdog_thread = Some(thread);
            info!("Certificate watchdog initialized.");
        }
        Err(e) => error!("{}", e),
    }

    /*warn!("VERSIONS: {:?}", COMPONENT_VERSIONS.lock().unwrap());
    info!("{:?}", t_update_manifest);*/

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if RESTART_NECO.load(Ordering::SeqCst) {
            warn!("Restarting NECO. Breaking loop in main...");
            break;
        }
    }

    /*
        Cleanup
    */

    component_mqtt.disconnect(None);
    // neutron_mqtt.disconnect(None);

    // Join the certificate watchdog to the main thread
    if let Some(thread) = cert_watchdog_thread {
        if let Err(e) = thread.join() {
            error!("Could not join main and cert watchdog thread. {:?}", e);
        }
    }
}

/**
 * Checks if app is root.
 * If the app is not root, makes sure the user knows that some functions will not work.
 */
fn check_if_root() {
    if let Ok(user) = env::var("USER") {
        if user == "root" {
            return;
        }
    }

    eprintln!("This application needs to be ran as root. Some functions WILL fail.");
}

/**
 * Processes the command-line arguments provided on app start.
 */
fn process_cli_args() {
    let matches = App::new("Neutron Communicator")
        .version(APP_VERSION)
        .author("SyStem")
        .about("Version and encryption certificate manager for the LSOC system.")
        .arg(
            Arg::with_name("verbosity")
                .short("v")
                .value_name("VERBOSITY")
                .help("Sets the level of verbosity.")
                .possible_values(&["info", "warn", "debug", "trace"])
                .default_value("info"),
        )
        .subcommand(SubCommand::with_name("gen_settings").about("Generate default settings file."))
        .subcommand(SubCommand::with_name("neutron_credentials").about("Set the Neutron server credentials.")
                    .arg(Arg::with_name("neutron_username")
                            .long("neutron_user")
                            .short("a")
                            .value_name("STRING")
                            .help("Specify the account username under which this NECO is registered. Can only contain Alpha-Numeric chars!")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("mqtt_username")
                            .long("username")
                            .short("u")
                            .value_name("STRING")
                            .help("Specify the MQTT username of the registered updater.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("mqtt_password")
                            .long("password")
                            .short("p")
                            .help("Specify the MQTT password of the registered updater.")
                            .value_name("STRING")
                            .takes_value(true)
                            .required(true))
                    )
        .subcommand(SubCommand::with_name("comp_backhaul_credentials").about("Set the component backhaul credentials.")
                    .arg(Arg::with_name("ip_address")
                            .long("ip_address")
                            .short("i")
                            .value_name("STRING")
                            .help("Specify the ip address of the MQTT server.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("port")
                            .long("port")
                            .short("p")
                            .value_name("STRING")
                            .help("Specify the port of the MQTT server.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("username")
                            .long("username")
                            .short("u")
                            .value_name("STRING")
                            .help("Specify the username of this client.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("password")
                            .long("password")
                            .short("w")
                            .value_name("STRING")
                            .help("Specify the password for this client.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("ca_file")
                            .long("ca_file")
                            .short("c")
                            .value_name("FILE")
                            .help("Path to the CA certificate file. THE PATH MUST END WITH A FILE EXTENSION!")
                            .takes_value(true)
                            .required(true))
                    )
        .subcommand(SubCommand::with_name("update_component").about("Add/remove an update component - used for version tracking.")
                .subcommand(SubCommand::with_name("add").about("Add an update component.")
                    .arg(Arg::with_name("name")
                            .long("name")
                            .short("n")
                            .value_name("STRING")
                            .help("Specify the component name.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("version_file_path")
                            .long("version_file_path")
                            .short("v")
                            .value_name("FILE")
                            .help("Specify the version file path.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("owner")
                            .long("owner")
                            .short("o")
                            .value_name("STRING")
                            .help("Specify the user that owns the component files.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("owner_group")
                            .long("owner_group")
                            .short("g")
                            .value_name("STRING")
                            .help("Specify the user group that owns the component files.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("permissions")
                            .long("permissions")
                            .short("p")
                            .value_name("STRING")
                            .help("Specify the default file permissions.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("container_name")
                            .long("container_name")
                            .short("d")
                            .value_name("STRING")
                            .help("Name of the component docker container.")
                            .takes_value(true)
                            .required(false))
                    .arg(Arg::with_name("service_name")
                            .long("service_name")
                            .short("s")
                            .value_name("STRING")
                            .help("Name of the component systemd service.")
                            .takes_value(true)
                            .required(false))
                    .arg(Arg::with_name("restart_command")
                            .long("restart_command")
                            .short("r")
                            .value_name("STRING")
                            .help("Command for restarting the component container/service.")
                            .takes_value(true)
                            .required(true))
                    )
                .subcommand(SubCommand::with_name("remove").about("Remove an update component.")
                    .arg(Arg::with_name("name")
                            .long("name")
                            .short("n")
                            .value_name("STRING")
                            .help("Specify the component name.")
                            .takes_value(true)
                            .required(true))
                    )
                )
        .subcommand(SubCommand::with_name("add_cert_aux_paths").about("Adds an entry to the auxiliary paths of the specified certificate/component.")
                    .arg(Arg::with_name("component_name")
                            .long("name")
                            .value_name("STRING")
                            .help("Specify the name of the component the certificate belongs to.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("certificate_type")
                            .long("type")
                            .value_name("TYPE")
                            .help("Specify the type of certificate you want to add auxiliary paths to.")
                            .possible_values(&["ca", "main"])
                            .default_value("main"))
                    .arg(Arg::with_name("paths")
                            .short("p")
                            .long("paths")
                            .value_name("KEY_FILE> <CERT_FILE")
                            .help("Specify the path of the key and certificate file (seperated by a 'space'). THE PATHS MUST END WITH A FILE EXTENSION!")
                            .takes_value(true)
                            .multiple(true)
                            .number_of_values(2)
                            .required(true))
                    )
        .subcommand(SubCommand::with_name("add_certificate").about("Add a new certificate for generation/tracking. (Use with no subcommand generates a self-signed certificate)")
                    .subcommand(SubCommand::with_name("ca-signed").about("Generate a CA-signed certificate.")
                                .arg(Arg::with_name("ca_not_encrypted")
                                        .long("ca_not_encrypted")
                                        .help("If specified, the CA key will not be encrypted with a randomly-generated passphrase."))
                                .arg(Arg::with_name("ca_certificate_duration")
                                        .long("ca_certificate_duration")
                                        .value_name("DAYS")
                                        .help("How many days will the certificate be valid.")
                                        .takes_value(true)
                                        .required(true))
                                .arg(Arg::with_name("ca_extensions")
                                        .long("ca_extensions")
                                        .value_name("STRING")
                                        .help("Specify certificate extensions.")
                                        .takes_value(true)
                                        .default_value("v3_ca"))
                                .arg(Arg::with_name("ca_cert_parameters")
                                        .long("ca_subj")
                                        .value_name("STRING")
                                        .help("Set CA certificate parameters.")
                                        .takes_value(true)
                                        .default_value("/C=HR/ST=Croatia/L=Zagreb/CN=127.0.0.1"))
                                .arg(Arg::with_name("ca_key_file")
                                        .long("ca_key_file")
                                        .value_name("FILE")
                                        .help("Path to the CA key file. THE PATH MUST END WITH A FILE EXTENSION!")
                                        .takes_value(true)
                                        .required(true))
                                .arg(Arg::with_name("ca_certificate_file")
                                        .long("ca_cert_file")
                                        .value_name("FILE")
                                        .help("Path to the CA certificate file. THE PATH MUST END WITH A FILE EXTENSION!")
                                        .takes_value(true)
                                        .required(true))
                                )
                    .arg(Arg::with_name("component_name")
                            .long("name")
                            .value_name("STRING")
                            .help("Set a name for the component that the certificates are generated for.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("algorithm")
                            .long("algorithm")
                            .value_name("ALGORITHM")
                            .help("Specify the algorithm for key generation.")
                            .takes_value(true)
                            .default_value("rsa:2048"))
                    .arg(Arg::with_name("key_not_encrypted")
                            .long("not_encrypted")
                            .help("If specified, the key will not be encrypted with a randomly-generated passphrase."))
                    .arg(Arg::with_name("certificate_duration")
                            .long("certificate_duration")
                            .value_name("DAYS")
                            .help("How many days will the certificate be valid.")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("key_length")
                            .long("key_length")
                            .value_name("SIZE")
                            .takes_value(true)
                            .default_value("2048"))
                    .arg(Arg::with_name("cert_parameters")
                            .long("subj")
                            .value_name("STRING")
                            .help("Set certificate parameters.")
                            .takes_value(true)
                            .default_value("/C=HR/ST=Croatia"))
                    .arg(Arg::with_name("key_file")
                            .long("key_file")
                            .value_name("FILE")
                            .help("Path to key file. THE PATH MUST END WITH A FILE EXTENSION!")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("certificate_file")
                            .long("cert_file")
                            .value_name("FILE")
                            .help("Path to certificate file. THE PATH MUST END WITH A FILE EXTENSION!")
                            .takes_value(true)
                            .required(true))
                    .arg(Arg::with_name("service_ips")
                            .long("service_ips")
                            .value_name("IP")
                            .help("Specify the allowed IPs/Domains of the service (seperated by a 'space', must start with either 'IP:' or 'DNS:').")
                            .takes_value(true)
                            .multiple(true)
                            .number_of_values(1)
                            .use_delimiter(true)
                            .required(true))
                    )
        .get_matches();

    init_logging(matches.value_of("verbosity").unwrap());

    //if let Some(cmd) = matches.subcommand_matches("gen_settings") {
    if matches.subcommand_matches("gen_settings").is_some() {
        match settings::write_default() {
            Ok(path) => info!("Default settings file generated. File Path: {}", path),
            Err(e) => {
                error!("Could not write default settings to disk. {}", e);
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    if let Some(cmd) = matches.subcommand_matches("neutron_credentials") {
        if let Ok(settings_struct) = settings::init() {
            if let Err(e) = settings::mqtt_connection::save_neutron_creds (
                settings_struct,
                cmd.value_of("neutron_username").unwrap(),
                cmd.value_of("mqtt_username").unwrap(),
                cmd.value_of("mqtt_password").unwrap(),
            ) {
                error!("{}", e);
                std::process::exit(1);
            }
        } else {
            std::process::exit(1)
        }

        info!("Neutron configuration successfully saved.");
        std::process::exit(0);
    }

    if let Some(cmd) = matches.subcommand_matches("comp_backhaul_credentials") {
        if let Ok(settings_struct) = settings::init() {
            if let Err(e) = settings::mqtt_connection::save_component_creds(
                settings_struct,
                cmd.value_of("ip_address").unwrap(),
                cmd.value_of("port").unwrap(),
                cmd.value_of("username").unwrap(),
                cmd.value_of("password").unwrap(),
                cmd.value_of("ca_file").unwrap(),
            ) {
                error!("{}", e);
                std::process::exit(1);
            }
        } else {
            std::process::exit(1)
        }

        info!("Component backhaul configuration successfully saved.");
        std::process::exit(0);
    }

    if let Some(cmd) = matches.subcommand_matches("update_component") {
        if let Some(cmd_add) = cmd.subcommand_matches("add") {
            if let Ok(settings_struct) = settings::init() {
                let mut component = settings::structs::UpdateComponent::default();

                if let Some(container_name) = cmd_add.value_of("container_name") {
                    component.container_name = Some(container_name.to_owned());
                } else if let Some(service_name) = cmd_add.value_of("service_name"){
                    component.service_name = Some(service_name.to_owned());
                } else {
                    error!("Neither container name or service name weren't specified.");
                    std::process::exit(1);
                }

                component.name = cmd_add.value_of("name").unwrap().to_owned();
                component.version_file_path = cmd_add.value_of("version_file_path").unwrap().to_owned();
                component.permission_user = cmd_add.value_of("owner").unwrap().to_owned();
                component.permission_group = cmd_add.value_of("owner_group").unwrap().to_owned();
                component.file_permissions = cmd_add.value_of("permissions").unwrap().to_owned();

                component.restart_command = cmd_add.value_of("restart_command").unwrap().to_owned();

                if let Err(e) = settings::update_components::add_update_component (
                    settings_struct,
                    component,
                ) {
                    error!("{}", e);
                    std::process::exit(1);
                }
            } else {
                std::process::exit(1)
            }

            info!("Update component successfully added.");
        } else if let Some(cmd_remove) = cmd.subcommand_matches("remove") {
            if let Ok(settings_struct) = settings::init() {
                if let Err(e) = settings::update_components::remove_update_component (
                    settings_struct,
                    cmd_remove.value_of("name").unwrap(),
                ) {
                    error!("{}", e);
                    std::process::exit(1);
                }
            } else {
                std::process::exit(1)
            }

            info!("Update component successfully removed.");
        }
        std::process::exit(0);
    }

    if let Some(cmd) = matches.subcommand_matches("add_cert_aux_paths") {
        if let Ok(settings_struct) = settings::init() {
            if let Err(e) = settings::encryption_certificates::append_cert_aux_paths(
                settings_struct,
                cmd.value_of("component_name").unwrap(),
                cmd.value_of("certificate_type").unwrap(),
                cmd.values_of("paths")
                    .unwrap()
                    .collect::<Vec<&str>>()
                    .as_slice(),
            ) {
                error!("{}", e);
                std::process::exit(1);
            }
        } else {
            std::process::exit(1)
        }

        info!("Certificates generated and paths added to certificate auxiliary path list.");
        std::process::exit(0);
    }

    if let Some(cmd) = matches.subcommand_matches("add_certificate") {
        let mut cert = settings::structs::CertificateSettings {
            component_name: cmd.value_of("component_name").unwrap().to_owned(),
            algorithm: cmd.value_of("algorithm").unwrap().to_owned(),
            cert_authority: None,
            main_certificate: settings::structs::MainCertificate {
                encrypted: !cmd.is_present("key_not_encrypted"),
                duration: cmd
                    .value_of("certificate_duration")
                    .unwrap()
                    .parse()
                    .unwrap(),
                key_len: cmd.value_of("key_length").unwrap().parse().unwrap(),
                subj: cmd.value_of("cert_parameters").unwrap().to_owned(),
                main_paths: settings::structs::CertificatePaths {
                    key: cmd.value_of("key_file").unwrap().to_owned(),
                    cert: cmd.value_of("certificate_file").unwrap().to_owned(),
                },
                auxiliary_paths: Vec::new(),
                service_ips: cmd
                    .values_of("service_ips")
                    .unwrap()
                    .map(std::borrow::ToOwned::to_owned)
                    .collect(),
                date_issued: None,
                passphrase: String::new(),
            },
        };

        if let Some(ca_signed) = cmd.subcommand_matches("ca-signed") {
            info!("Generating a CA-Signed certificate.");

            cert.cert_authority = Some(settings::structs::CACertificate {
                encrypted: !ca_signed.is_present("ca_not_encrypted"),
                duration: ca_signed
                    .value_of("ca_certificate_duration")
                    .unwrap()
                    .parse()
                    .unwrap(),
                extensions: ca_signed.value_of("ca_extensions").unwrap().to_owned(),
                subj: ca_signed.value_of("ca_cert_parameters").unwrap().to_owned(),
                main_paths: settings::structs::CertificatePaths {
                    key: ca_signed.value_of("ca_key_file").unwrap().to_owned(),
                    cert: ca_signed
                        .value_of("ca_certificate_file")
                        .unwrap()
                        .to_owned(),
                },
                auxiliary_paths: Vec::new(),
                date_issued: None,
                passphrase: String::new(),
            });
        } else {
            info!("Generating a Self-Signed certificate.");
        }

        if let Ok(settings_struct) = settings::init() {
            if let Err(e) =  settings::encryption_certificates::add_certificate(settings_struct, cert) {
                error!("{}", e);
                std::process::exit(1);
            }
        } else {
            std::process::exit(1);
        }

        info!("New certificate is successfully registered and generated.");
        std::process::exit(0);
    }
}

/**
 * Initializes logging with specified detail:
 * ``` filter: 'info', 'warn', 'debug', 'trace' ```
 */
fn init_logging(filter: &str) {
    let env = env_logger::Env::default()
        .filter_or("RUST_LOG", ["neutron_communicator=", filter].concat());
    env_logger::init_from_env(env);
}

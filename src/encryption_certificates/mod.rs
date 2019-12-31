// #![allow(dead_code)]

use std::convert::TryInto;
use std::fs;
use std::io::{Error, ErrorKind, Write};
use std::ops::Sub;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::NamedTempFile;

use std::thread;
use std::thread::JoinHandle;

//use std::sync::mpsc::Sender;

use chrono::prelude::NaiveDateTime;

use rand::prelude::thread_rng;
use rand::seq::SliceRandom;

use crate::settings::encryption_certificates::save_certificates;
use crate::settings::structs::{CACertificate, CertificateSettings};

use crate::RESTART_NECO;

pub mod structs;

const WATCHDOG_TIMEOUT: u64 = 24 * 60 * 60;

const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
    abcdefghijklmnopqrstuvwxyz\
    0123456789";
const PASSPHRASE_LENGTH: u16 = 20; // 0 - 65535

/**
 * Checks if all certificates/keys exist, if something is missing; certificate generation is ran (could be a CA cert or a child certificate).
 * Each certificate generation function returns the generated key passphrase which is then updated in the vector upon its return.
 * Before pushing the `CertificateSettings` struct to the `valid_certs` vector, we get the date last-modified of the crt file and save it to the struct.
 *     That way the certificate watchdog can periodically compare the current date with the `date-issued` and decide if the certificate needs renewal.
 * If some function fails when generating or fetching something, the certificate is not added to the `valid_certs`.
 *
 * Before calling `start_watchdog()`, we call a settings function for saving the certificates to the settings file `settings::save_certificates`.
 *     All certificates get saved - the ones that error-out and the ones successfully generated.
 *
 * Channels the return value from `start_watchdog()`.
 */
pub fn init(certificates: &[CertificateSettings]) -> Result<JoinHandle<()>, Error> {
    info!("Initializing certificate watchdog...");

    let mut all_certs: Vec<CertificateSettings> = certificates.to_vec();

    let mut valid_certs: Vec<CertificateSettings> = Vec::new();

    for mut cert in &mut all_certs {
        if let Some(ca) = cert.cert_authority.as_mut() {
            if fs::metadata(&ca.main_paths.cert).is_err()
                || fs::metadata(&ca.main_paths.key).is_err()
            {
                match generate_ca(&cert.component_name, ca, false) {
                    Ok(passphrase) => {
                        // Update the passphrase so we can use it when generating a signed certificate
                        ca.passphrase = passphrase;

                        match generate_certificate(cert, false) {
                            Ok(pass) => cert.main_certificate.passphrase = pass,
                            Err(e) => return Err(e),
                        }
                    }
                    Err(e) => return Err(e),
                }
            } else {
                for aux_path in &ca.auxiliary_paths {
                    if fs::metadata(&aux_path.key).is_err() || fs::metadata(&aux_path.cert).is_err()
                    {
                        // If function returns Ok, break the loop since we're going to copy the cert/key to all aux locations
                        // Calling generate_ca(just_populate_aux = true) will skip creating a CA cert/key and will just distribute certs/keys to auxiliary paths
                        if let Err(e) = generate_ca(&cert.component_name, ca, true) {
                            return Err(e);
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        // We need to unwrap here again because we can only get the date last modified if file exists
        // This cannot be in the same block as existance checking because of the borrow checker
        if let Some(ca) = cert.cert_authority.as_mut() {
            // Calculate the exact time the CA certificate was created(last modified)
            if let Some(date) = get_date_issued(&ca.main_paths.cert) {
                ca.date_issued = Some(date.to_string());
            } else {
                error!(
                    "Could not determine the CA certificate issue date. Skipping certificate..."
                );
                continue;
            }
        }

        // Check if the cert-key combo exist on the main path
        if fs::metadata(&cert.main_certificate.main_paths.key).is_err()
            || fs::metadata(&cert.main_certificate.main_paths.cert).is_err()
        {
            // Generate new key-cert combo on the main and auxiliary paths
            // If the function returns Err, return init()
            // If we're Ok get the returned passphrase and update it in the cert
            //     struct so watchdog can renew without the need to restart NECO
            match generate_certificate(cert, false) {
                Ok(pass) => cert.main_certificate.passphrase = pass,
                Err(e) => return Err(e),
            }
        } else {
            // If the cert-key exist on the main path
            // Loop through the auxiliary paths and check if they're all there
            for aux_path in &cert.main_certificate.auxiliary_paths {
                if fs::metadata(&aux_path.key).is_err() || fs::metadata(&aux_path.cert).is_err() {
                    // If function returns Ok, break the loop since we're going to copy the cert/key to all aux locations
                    // Calling generate_certificate(just_populate_aux = true) will skip creating a cert/key and will just distribute certs/keys to auxiliary paths
                    if let Err(e) = generate_certificate(cert, true) {
                        return Err(e);
                    } else {
                        break;
                    }
                }
            }
        }

        // Calculate the exact time the certificate was created(last modified)
        if let Some(date) = get_date_issued(&cert.main_certificate.main_paths.cert) {
            cert.main_certificate.date_issued = Some(date.to_string());
            valid_certs.push(cert.clone());
        } else {
            error!("Could not determine the certificate issue date. Skipping certificate...");
        }
    }

    // Save the updated certificates vector to the settings file
    if let Err(e) = save_certificates(all_certs.to_vec()) {
        return Err(e);
    }

    start_watchdog(valid_certs)
}

/**
 * Spawns a watchdog thread used for monitoring certificate age.
 * Loops through the certificates (CA and child), parses the date issued from `String` to `NaiveDateTime`,
 *     gets the current time, calculates the difference in days then checks if the difference is >= `cert.duration - 10`.
 *     If it is, try to renew it (renewal by a CA or a key). If we, for some reason, fail renewing; continue the loop and write-out an error.
 *     If it is successful, update the `date-issued` key in the struct so we can compare against valid data.
 * If the thread spawning failed, return an error containing the thread message.
 * If the thread spawning was successful, return the handle to the thread.
 */
fn start_watchdog(mut certificates: Vec<CertificateSettings>) -> Result<JoinHandle<()>, Error> {
    let watchdog = thread::Builder::new().name(String::from("CertWatchdog"));

    let handle = watchdog.spawn(move || loop {
        for cert in &mut certificates {
            // CA
            if cert.cert_authority.is_some() {
                let ca = cert.cert_authority.as_mut().unwrap();
                let date_issued = ca.date_issued.as_ref().unwrap();

                let parsed_date = NaiveDateTime::parse_from_str(date_issued, "%Y-%m-%d %H:%M:%S").unwrap();

                // Get the number of days between todays date and the date obtained from the file
                let difference_in_days = chrono::Utc::now()
                    .naive_local()
                    .signed_duration_since(parsed_date)
                    .num_days();

                // Check if the certificate is older than (duration - 10) days
                if difference_in_days >= (ca.duration - 10) {
                    warn!(
                        "{} CA certificate needs renewal. Date issued: {}.",
                        &cert.component_name, date_issued
                    );

                    // Call the gen_csr_sign_with_key() and if it errors-out, log it.
                    if let Err(e) = gen_csr_sign_with_key(
                        &cert.component_name,
                        &ca.main_paths.key,
                        ca.encrypted,
                        &ca.subj,
                        &ca.passphrase,
                        ca.duration,
                        &ca.main_paths.cert,
                    ) {
                        error!("{}", e);
                    } else {
                        debug!(
                            "Renewed CA certificate. Component: {}",
                            &cert.component_name
                        );

                        // Update the date issued on the CA certificate
                        if let Some(date) = get_date_issued(&ca.main_paths.cert) {
                            ca.date_issued = Some(date.to_string());
                        } else {
                            error!("Could not determine the CA certificate issue date.");
                        }
                    }
                }
            }

            // Main certificate
            {
                let date_issued = cert.main_certificate.date_issued.as_ref().unwrap();
                // or
                // let date_issued = if let Some(date_issued) = cert.date_issued.as_ref() {
                //     date_issued
                // } else {
                //     thread::sleep(std::time::Duration::from_secs(WATCHDOG_TIMEOUT));
                //     continue;
                // };

                let parsed_date =
                    NaiveDateTime::parse_from_str(date_issued, "%Y-%m-%d %H:%M:%S").unwrap();

                // Get the number of days between todays date and the date obtained from the file
                let difference_in_days = chrono::Utc::now()
                    .naive_local()
                    .signed_duration_since(parsed_date)
                    .num_days();

                // Check if the certificate is older than (duration - 10) days
                if difference_in_days >= (cert.main_certificate.duration - 10) {
                    warn!(
                        "{} certificate needs renewal. Date issued: {}.",
                        &cert.component_name, date_issued
                    );

                    // With this boolean we avoid code duplication and the use of the `continue`
                    //     keyword that could cause a loop with no sleep between cycles
                    let mut is_generated = true;

                    if cert.cert_authority.is_some() {
                        if let Err(e) = gen_csr_sign_with_ca(cert, &cert.main_certificate.passphrase) {
                            error!("{}", e);
                            is_generated = false;
                        }
                    } else if let Err(e) = gen_csr_sign_with_key(
                        &cert.component_name,
                        &cert.main_certificate.main_paths.key,
                        cert.main_certificate.encrypted,
                        &cert.main_certificate.subj,
                        &cert.main_certificate.passphrase,
                        cert.main_certificate.duration,
                        &cert.main_certificate.main_paths.cert,
                    ) {
                        error!("{}", e);
                        is_generated = false;
                    }

                    if is_generated {
                        debug!(
                            "Renewed certificate with a {}. Component: {}",
                            if cert.cert_authority.is_some() {
                                "CA"
                            } else {
                                "key"
                            },
                            &cert.component_name
                        );

                        // Update the date issued on the main certificate
                        if let Some(date) = get_date_issued(&cert.main_certificate.main_paths.cert)
                        {
                            cert.main_certificate.date_issued = Some(date.to_string());
                        } else {
                            error!("Could not determine the certificate issue date.");
                        }
                    }
                }
            }

            // Maybe: Broadcasting cert data to other NECOs if we're gonna run distributed...
            // This should be in the cert renewal block (watchdog)
            // Save the cert-key data and the date issued, then send it through mpsc
            // if let Some(date) = get_date_issued(&main_path.cert) {
            //     let data = structs::CertificateKeyPair {
            //         certificate: String::from("dummy cert"),
            //         key: String::from("dummy key"),
            //         date_issued: date.to_string()
            //     };

            //     if let Ok(option) = TX_WATCHDOG.lock() {
            //         if let Some(watchdog_tx) = &*option {
            //             if watchdog_tx.send(data).is_err() {
            //                 error!("Could not send data through mpsc.");
            //             }
            //         }
            //     }
            // } else {
            //     error!("Could not get correct date the cert-key were issued. Will not be able to send cert data through mpsc");
            // }
        }

        thread::sleep(std::time::Duration::from_secs(WATCHDOG_TIMEOUT));

        // Here we check if NECO is about to restart, if it is; break the loop
        if RESTART_NECO.load(std::sync::atomic::Ordering::SeqCst)
        /* || cfg!(debug_assertions)*/
        {
            break;
        }
    });

    if let Ok(watchdog_thread) = handle {
        return Ok(watchdog_thread);
    }

    let msg = format!(
        "Could not create the certificate watchdog thread. {:?}",
        handle.err()
    );
    Err(Error::new(ErrorKind::Other, msg))
}

/**
 * Creates a self-signed or a CA child certificate and key, saves them to the main and auxiliary paths.
 * Generated key passphrase is returned.
 * If the returned value is empty, `just_populate_aux` boolean was true or the certificate key encryption key was set to false in the settings.
 * If `just_populate_aux` is set to true - then only the copying of the main certificate/key to the auxiliary paths will be executed - cert/key generation is skipped.
 */
pub fn generate_certificate(
    certificate: &CertificateSettings,
    just_populate_aux: bool,
) -> Result<String, Error> {
    let mut key_passphrase = String::new();

    if !just_populate_aux {
        // Certificates signed with a CA

        if certificate.cert_authority.is_some() {
            debug!(
                "Generating a CA-signed certificate. Component: {}",
                &certificate.component_name
            );

            let mut key_cmd = Command::new("openssl");
            key_cmd.arg("genrsa");

            if certificate.main_certificate.encrypted {
                key_cmd.arg("-aes256");
            }

            key_cmd.args(&["-out", &certificate.main_certificate.main_paths.key]);

            if certificate.main_certificate.encrypted {
                match rand_passphrase() {
                    Some(passphrase) => {
                        key_cmd.args(&["-passout", &["pass:", &passphrase].concat()]);
                        key_passphrase = passphrase;
                    }
                    None => {
                        return Err(Error::new(
                            ErrorKind::Other,
                            "Could not generate a random passphrase.",
                        ))
                    }
                }
            }

            if certificate.main_certificate.key_len > 0 {
                key_cmd.arg(&certificate.main_certificate.key_len.to_string());
            } else {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "Key length needs to be bigger than 0.",
                ));
            }

            match key_cmd.output() {
                Ok(res) => {
                    debug!(
                        "Generating a key of length: {}.",
                        &certificate.main_certificate.key_len
                    );
                    // OpenSSL command output is on stderr
                    debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
                }
                Err(e) => return Err(e),
            }

            if let Err(e) = gen_csr_sign_with_ca(&certificate, &key_passphrase) {
                return Err(e);
            }
        } else {
            // Self-signed certificates

            let mut command = Command::new("openssl");
            command.arg("req");
            command.args(&["-newkey", &certificate.algorithm]);
            if !certificate.main_certificate.encrypted {
                command.arg("-nodes");
            }
            command.args(&["-keyout", &certificate.main_certificate.main_paths.key]);
            command.arg("-x509");
            command.args(&["-days", &certificate.main_certificate.duration.to_string()]);
            command.args(&["-out", &certificate.main_certificate.main_paths.cert]);
            command.args(&["-subj", &certificate.main_certificate.subj]);
            if certificate.main_certificate.encrypted {
                let passphrase;
                match rand_passphrase() {
                    Some(pass) => passphrase = pass,
                    None => {
                        return Err(Error::new(
                            ErrorKind::Other,
                            "Could not generate a random passphrase.",
                        ))
                    }
                }

                command.args(&["-passout", &["pass:", &passphrase].concat()]);

                key_passphrase = passphrase;
            }

            match command.output() {
                Ok(res) => {
                    debug!(
                        "Generating a self-signed certificate for component: {}.",
                        &certificate.component_name
                    );
                    // OpenSSL command output is on stderr
                    debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
                }
                Err(e) => return Err(e),
            }
        }
    }

    debug!(
        "Copying the main certificate/key to the auxiliary paths for component: {}.",
        &certificate.component_name
    );

    for path in &certificate.main_certificate.auxiliary_paths {
        // Check if any path is empty, if it is; skip the copy so we don't get errors
        // If we fail at copying anywhere, we return Err

        if !certificate.main_certificate.main_paths.key.is_empty() && !path.key.is_empty() {
            if let Err(e) = fs::copy(&certificate.main_certificate.main_paths.key, &path.key) {
                let msg = format!("Failed to copy key to auxiliary path. {}", e);
                return Err(Error::new(ErrorKind::Other, msg));
            }
        }

        if !certificate.main_certificate.main_paths.cert.is_empty() && !path.cert.is_empty() {
            if let Err(e) = fs::copy(&certificate.main_certificate.main_paths.cert, &path.cert) {
                let msg = format!("Failed to copy certificate to auxiliary path. {}", e);
                return Err(Error::new(ErrorKind::Other, msg));
            }
        }
    }

    if just_populate_aux {
        return Ok(String::new());
    }

    Ok(key_passphrase)
}

/**
 * Generates a CSR (Certificate Signing Request) with the info from the `cert.main_certificate` struct.
 * That CSR is saved to the same path as the main certificate key, with the extension `.csr`.
 * The CSR is then signed with the CA (Certificate Authority) with the info from the `cert.cert_authority` struct.
 * The CSR file is removed after the certificate has been successfully signed.
 */
fn gen_csr_sign_with_ca(
    cert: &CertificateSettings,
    main_key_passphrase: &str,
) -> Result<(), Error> {
    let key_path = &cert.main_certificate.main_paths.key;

    let csr_temp_path = if key_path.contains('.') {
        [&key_path.split('.').take(1).collect::<String>(), ".csr"].concat()
    } else {
        return Err(Error::new(
            ErrorKind::Other,
            "Main certificate key path does not end with a file extension.",
        ));
    };

    let mut cmd_csr = Command::new("openssl");
    cmd_csr.arg("req");
    cmd_csr.args(&["-out", &csr_temp_path]);
    cmd_csr.args(&["-key", &cert.main_certificate.main_paths.key]);
    cmd_csr.arg("-new");
    cmd_csr.args(&["-subj", &cert.main_certificate.subj]);
    if cert.main_certificate.encrypted {
        cmd_csr.args(&["-passin", &["pass:", main_key_passphrase].concat()]);
    }

    let mut cmd_sign_crt;
    if let Some(ca) = &cert.cert_authority {
        cmd_sign_crt = Command::new("openssl");
        cmd_sign_crt.arg("x509");
        cmd_sign_crt.arg("-req");
        cmd_sign_crt.args(&["-in", &csr_temp_path]);
        cmd_sign_crt.args(&["-CA", &ca.main_paths.cert]);
        cmd_sign_crt.args(&["-CAkey", &ca.main_paths.key]);
        cmd_sign_crt.arg("-CAcreateserial");
        cmd_sign_crt.args(&["-out", &cert.main_certificate.main_paths.cert]);

        if !cert.main_certificate.service_ips.is_empty() {
            let san_ips = &cert.main_certificate.service_ips.join(",");
            /*for ip in &cert.main_certificate.service_ips {
                if !san_ips.is_empty() {
                    san_ips = format!("{},", san_ips);
                }
                san_ips = format!("{}IP:{}", san_ips, ip)
            }*/

            let sans = format!("\n[SAN]\nsubjectAltName={}", san_ips);

            match NamedTempFile::new() {
                Ok(mut file) => {
                    if let Err(e) = file.write(sans.as_bytes()) {
                        return Err(e);
                    }

                    match file.keep() {
                        Ok(f) => {
                            match (f.1).to_str() {
                                Some(path) => {
                                    cmd_sign_crt.args(&["-extfile", path, "-extensions", "SAN"])
                                }
                                None => {
                                    return Err(Error::new(
                                        ErrorKind::Other,
                                        "Could not find path to temporary SANS file.",
                                    ))
                                }
                            };
                        }
                        Err(e) => {
                            return Err(Error::new(ErrorKind::Other, e));
                        }
                    }
                }
                Err(e) => return Err(e),
            }
        }

        cmd_sign_crt.args(&["-days", &cert.main_certificate.duration.to_string()]);
        if ca.encrypted {
            cmd_sign_crt.args(&["-passin", &["pass:", &ca.passphrase].concat()]);
        }
    } else {
        return Err(Error::new(
            ErrorKind::NotFound,
            "Could not find CA certificate settings.",
        ));
    }

    match cmd_csr.output() {
        Ok(res) => {
            debug!("Generating a CSR for signing with a CA certificate...");
            // OpenSSL command output is on stderr
            debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
        }
        Err(e) => return Err(e),
    }

    match cmd_sign_crt.output() {
        Ok(res) => {
            debug!(
                "Signed certificate with a CA for component: {}.",
                &cert.component_name
            );
            // OpenSSL command output is on stderr
            debug!("Command output: \n{}", String::from_utf8_lossy(&res.stderr));
        }
        Err(e) => return Err(e),
    }

    if let Err(e) = fs::remove_file(csr_temp_path) {
        error!("Could not remove the CSR file. {}", e);
    }

    Ok(())
}

/**
 * Generates a CSR (Certificate Signing Request) with the `signing_key`, `subj`, `signing_key_encrypted`, `passphrase` function parameters.
 * The CSR is saved to the same path as the signing key, with the extension `.csr`.
 * The CSR is then signed with the `cert_duration`, `signing_key` and the generated certificate is saved to the path in `crt_path`.
 * CSR file is deleted if the certificate was signed successfully.
 */
fn gen_csr_sign_with_key(
    component_name: &str,
    signing_key: &str,
    signing_key_encrypted: bool,
    subj: &str,
    passphrase: &str,
    cert_duration: i64,
    crt_path: &str,
) -> Result<(), Error> {
    let csr_temp_path = if signing_key.contains('.') {
        [&signing_key.split('.').take(1).collect::<String>(), ".csr"].concat()
    } else {
        return Err(Error::new(
            ErrorKind::Other,
            "Signing key path does not end with a file extension. Path: {}"
                .replace("{}", signing_key),
        ));
    };

    let mut csr = Command::new("openssl");
    csr.arg("req");
    csr.args(&["-out", &csr_temp_path]);
    csr.args(&["-key", signing_key]);
    csr.arg("-new");
    csr.args(&["-subj", subj]);
    if signing_key_encrypted {
        csr.args(&["-passin", &["pass:", passphrase].concat()]);
    }

    let mut sign_csr = Command::new("openssl");
    sign_csr.arg("x509");
    sign_csr.arg("-req");
    sign_csr.args(&["-days", &cert_duration.to_string()]);
    sign_csr.args(&["-in", &csr_temp_path]);
    sign_csr.args(&["-signkey", signing_key]);
    sign_csr.args(&["-out", crt_path]);
    if signing_key_encrypted {
        sign_csr.args(&["-passin", &["pass:", passphrase].concat()]);
    }

    match csr.output() {
        Ok(res) => {
            debug!("Generating a CSR for signing with a key...");
            // OpenSSL command output is on stderr
            debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
        }
        Err(e) => return Err(e),
    }

    match sign_csr.output() {
        Ok(res) => {
            debug!(
                "Signed certificate using key for component: {}.",
                component_name
            );
            // OpenSSL command output is on stderr
            debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
        }
        Err(e) => return Err(e),
    }

    if let Err(e) = fs::remove_file(csr_temp_path) {
        error!("Could not remove the CSR file. {}", e);
    }

    Ok(())
}

/**
 * Generates a CA (Certificate Authority) with the info in the `ca_config` function parameter.
 * If the `just_populate_aux` function parameter is set to true, CA generation will be skipped but the CA crt/key will be copied over to the auxiliary paths.
 * Parameter `component_name` is just used for logging messages.
 */
pub fn generate_ca(
    component_name: &str,
    ca_config: &CACertificate,
    just_populate_aux: bool,
) -> Result<String, Error> {
    let mut passphrase = String::new();

    if !just_populate_aux {
        debug!("Generating a CA for component: {}", component_name);

        let mut command = Command::new("openssl");
        command.arg("req");
        command.args(&["-new", "-x509"]);

        if !ca_config.encrypted {
            command.arg("-nodes");
        }

        command.args(&["-days", &ca_config.duration.to_string()]);
        command.args(&["-extensions", &ca_config.extensions]);
        command.args(&["-keyout", &ca_config.main_paths.key]);
        command.args(&["-out", &ca_config.main_paths.cert]);
        command.args(&["-subj", &ca_config.subj]);

        if ca_config.encrypted {
            match rand_passphrase() {
                Some(pass) => passphrase = pass,
                None => {
                    return Err(Error::new(
                        ErrorKind::Other,
                        "Could not generate a random passphrase.",
                    ))
                }
            }

            command.args(&["-passout", &["pass:", &passphrase].concat()]);
        }

        match command.output() {
            Ok(res) => {
                debug!("Generated a CA for component: {}.", component_name);
                // OpenSSL command output is on stderr
                debug!("Command output: {}", String::from_utf8_lossy(&res.stderr));
            }
            Err(e) => return Err(e),
        }
    }

    for path in &ca_config.auxiliary_paths {
        // Check if any path is empty, if it is; skip the copy so we don't get errors
        // If we fail at copying anywhere, we return Err

        if !ca_config.main_paths.key.is_empty() && !path.key.is_empty() {
            if let Err(e) = fs::copy(&ca_config.main_paths.key, &path.key) {
                let msg = format!("Failed to copy CA key to auxiliary path. {}", e);
                return Err(Error::new(ErrorKind::Other, msg));
            }
        }

        if !ca_config.main_paths.cert.is_empty() && !path.cert.is_empty() {
            if let Err(e) = fs::copy(&ca_config.main_paths.cert, &path.cert) {
                let msg = format!("Failed to copy CA certificate to auxiliary path. {}", e);
                return Err(Error::new(ErrorKind::Other, msg));
            }
        }
    }

    Ok(passphrase)
}

/**
 * Subtracts the current date with the date on the path `file_path` and returns the date the file was `last modified`.
 */
fn get_date_issued(file_path: &str) -> Option<NaiveDateTime> {
    let file_modified = if let Ok(file) = fs::metadata(file_path) {
        if let Ok(file_modified_available) = file.modified() {
            file_modified_available
        } else {
            return None;
        }
    } else {
        return None;
    };

    let time_elapsed = if let Ok(elapsed) = file_modified.elapsed() {
        if let Ok(time) = SystemTime::now().sub(elapsed).duration_since(UNIX_EPOCH) {
            time.as_secs()
        } else {
            return None;
        }
    } else {
        return None;
    };

    NaiveDateTime::from_timestamp_opt(time_elapsed.try_into().unwrap(), 0)
}

/**
 * Generates a random passphrase from the provided character set.
 * Returns `None` if generation failed.
 */
fn rand_passphrase() -> Option<String> {
    let mut rand_generator = thread_rng();
    (0..PASSPHRASE_LENGTH)
        .map(|_| {
            Some(if let Some(ch) = CHARSET.choose(&mut rand_generator) {
                *ch as char
            } else {
                return None;
            })
        })
        .collect()
}

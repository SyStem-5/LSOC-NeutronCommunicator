use std::io::{Error, ErrorKind};

use super::{save_to_file, structs};
use crate::encryption_certificates::{generate_ca, generate_certificate};
use crate::SETTINGS;

/**
 * This function is intended to be ran after settings initialization (Mutex assignments).
 * Gets the `Settings` mutex lock and saves the struct as mutable, then replaces the `CertificateSettings` vector with the one in `certificates`.
 * The struct is then converted to JSON and saved to the settings file location.
 *
 * Mutex `SETTINGS` is locked momentarily (at the start).
 */
pub fn save_certificates(certificates: Vec<structs::CertificateSettings>) -> Result<(), Error> {
    let mut settings: structs::Settings;

    // Try to load the settings struct from the mutex
    if let Ok(global_settings) = SETTINGS.lock() {
        settings = global_settings.clone();
    } else {
        return Err(Error::new(
            ErrorKind::Other,
            "Could not lock settings mutex.",
        ));
    }

    // Set the certificate vector to our vector
    settings.certificates = certificates;

    save_to_file(settings)
}

/**
 * Searches the certificates vector for the one matching the component name then it modifies the auxiliary paths vector of the CA or
 *     main certificate depending on `cert_type` ('ca' or 'main'). Then it triggers the certificate generators for populating the auxiliary paths.
 * Returns an error if the certificate struct does not contain a CA certificate but it is specified in the `cert_type` parameter.
 * Returns an error if no certificate struct contains the component name specified in the `component_name` parameter.
 */
pub fn append_cert_aux_paths(
    mut settings: structs::Settings,
    component_name: &str,
    cert_type: &str,
    aux_paths: &[&str],
) -> Result<(), Error> {
    let mut failed_counter = 0;

    for cert in &mut settings.certificates {
        if cert.component_name == component_name {
            if cert_type == "ca" {
                if let Some(ca) = cert.cert_authority.as_mut() {
                    ca.auxiliary_paths.push(structs::CertificatePaths {
                        key: aux_paths[0].to_owned(),
                        cert: aux_paths[1].to_owned(),
                    });

                    if let Err(e) = generate_ca(component_name, ca, true) {
                        return Err(Error::new(ErrorKind::Other, e));
                    }
                } else {
                    return Err(Error::new(
                        ErrorKind::NotFound,
                        "Could not find a CA certificate for that component",
                    ));
                }
            } else {
                cert.main_certificate
                    .auxiliary_paths
                    .push(structs::CertificatePaths {
                        key: aux_paths[0].to_owned(),
                        cert: aux_paths[1].to_owned(),
                    });

                if let Err(e) = generate_certificate(&cert, true) {
                    return Err(Error::new(ErrorKind::Other, e));
                }
            }
        } else {
            failed_counter += 1;
        }
    }

    if failed_counter == settings.certificates.len() {
        return Err(Error::new(
            ErrorKind::NotFound,
            "Could not find a certificate with that component name.",
        ));
    }

    save_to_file(settings)
}

/**
 * Takes the certificate in the `certificate` parameter and inserts it into the certificates vector in the settings, `settings` parameter, struct.
 * If a certificate with the same `component_name` already exists, we return an error.
 * If we didn't error-out, we go into generating the actual certificates.
 */
pub fn add_certificate(
    mut settings: structs::Settings,
    mut certificate: structs::CertificateSettings,
) -> Result<(), Error> {
    if settings
        .certificates
        .iter()
        .map(|cert| cert.component_name == certificate.component_name)
        .any(|x| x)
    {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "A certificate with that component name already exists.",
        ));
    }

    if certificate.cert_authority.is_some() {
        match generate_ca(
            &certificate.component_name,
            &certificate.cert_authority.clone().unwrap(),
            false,
        ) {
            Ok(passphrase) => certificate.cert_authority.as_mut().unwrap().passphrase = passphrase,
            Err(e) => return Err(Error::new(ErrorKind::Other, e)),
        }
    }

    match generate_certificate(&certificate, false) {
        Ok(passphrase) => certificate.main_certificate.passphrase = passphrase,
        Err(e) => return Err(Error::new(ErrorKind::Other, e)),
    }

    settings.certificates.push(certificate);

    save_to_file(settings)
}

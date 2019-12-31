use std::fs::File;
use std::io::{BufReader, Error, Read};
use std::process::Command;

use data_encoding::HEXLOWER;
use ring::digest::{Context, Digest, SHA256};

/**
 * Calculates the sha256 hash from a provided file.
 */
fn sha256_digest<R: Read>(mut reader: R) -> Result<Digest, Error> {
    let mut context = Context::new(&SHA256);
    let mut buffer = [0; 1024];

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.update(&buffer[..count]);
    }

    Ok(context.finish())
}

/**
 * Compares the calculated hash from the file on the `file_path` and the provided hash.
 *
 * Returns `Ok(())` if the hashes are identical.
 */
pub fn compare_hash(file_path: &str, hash: &str) -> Result<(), Error> {
    let input = File::open(file_path)?;
    let reader = BufReader::new(input);
    let digest = sha256_digest(reader)?;

    if HEXLOWER.encode(digest.as_ref()) == hash {
        return Ok(());
    }

    Err(Error::new(
        std::io::ErrorKind::Other,
        "File verification failed.",
    ))
}

/**
 * Runs `chmod` and `chown` with parameters from `permission_user`, `permission_group`, `file_permissions`.
 * Command `chmod` is the first to run, if it fails; command `chown` is never ran.
 *
 * Returns `Ok(())` if `stderr` from both commands is empty.
 */
pub fn set_file_permissions(
    file_loc: &str,
    permission_user: &str,
    permission_group: &str,
    file_permissions: &str,
) -> Result<(), ()> {
    match Command::new("chmod")
        .arg(file_permissions)
        .arg(file_loc)
        .output()
    {
        Ok(res) => {
            if res.stderr.is_empty() {
                debug!("Update file permissions set.");
            } else {
                error!(
                    "Failed to set update file permissions. {}",
                    String::from_utf8_lossy(&res.stderr)
                );

                return Err(());
            }
        }
        Err(e) => {
            error!("Could not run 'chmod'. {}", e);
            return Err(());
        }
    }

    match Command::new("chown")
        .arg([permission_user, ":", permission_group].concat())
        .arg(file_loc)
        .output()
    {
        Ok(res) => {
            if res.stderr.is_empty() {
                debug!("Update file ownership set.");
            } else {
                error!(
                    "Failed to set update file ownership. {}",
                    String::from_utf8_lossy(&res.stderr)
                );
                return Err(());
            }
        }
        Err(e) => {
            error!("Could not run 'chown'. {}", e);
            return Err(());
        }
    }

    Ok(())
}

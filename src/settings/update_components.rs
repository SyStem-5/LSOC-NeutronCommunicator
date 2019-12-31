use std::io::{Error, ErrorKind};

use super::{save_to_file, structs};

/**
 * Appends the provided component to the update component vector which is then saved to file.
 * Only components with unique names get saved.
 */
pub fn add_update_component(
    mut settings: structs::Settings,
    component: structs::UpdateComponent,
) -> Result<(), Error> {
    let exists: bool = settings
        .update_components
        .iter()
        .map(|x| x.name == component.name)
        .any(|x| x);

    if exists {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            "An update component with that name already exists.",
        ));
    }

    settings.update_components.push(component);

    save_to_file(settings)
}

pub fn remove_update_component(
    mut settings: structs::Settings,
    component_name: &str,
) -> Result<(), Error> {
    let mut index = 0;
    let mut found = false;

    for component in &settings.update_components {
        if component.name == component_name {
            found = true;
            break;
        }
        index += 1;
    }

    if !found {
        return Err(Error::new(
            ErrorKind::NotFound,
            "A component with that name wasn't found.",
        ));
    }

    settings.update_components.remove(index);

    save_to_file(settings)
}

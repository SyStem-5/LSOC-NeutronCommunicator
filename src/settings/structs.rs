#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Settings {
    pub neutron_account_username: String,
    pub neutron_mqtt_client: NeutronMqttClient,
    pub component_mqtt_client: ComponentMqttClient,
    pub application_name: String,
    pub update_branch: String,
    pub update_components: Vec<UpdateComponent>,
    pub certificates: Vec<CertificateSettings>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NeutronMqttClient {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ComponentMqttClient {
    pub ip: String,
    pub port: String,
    pub username: String,
    pub password: String,
    pub cafile: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct UpdateComponent {
    pub name: String,
    pub version_file_path: String,
    pub permission_user: String,
    pub permission_group: String,
    pub file_permissions: String,
    pub container_name: Option<String>,
    pub service_name: Option<String>,
    // Before removing this, make the recipe processor work without this field
    pub restart_command: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CertificateSettings {
    pub component_name: String,
    pub algorithm: String,
    pub cert_authority: Option<CACertificate>, // If this is `None`, we assume the cert is self-signed
    pub main_certificate: MainCertificate,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CACertificate {
    pub encrypted: bool,
    pub duration: i64,
    pub extensions: String,
    pub subj: String,
    pub main_paths: CertificatePaths,
    pub auxiliary_paths: Vec<CertificatePaths>,
    pub date_issued: Option<String>, // This is used for transferring the date between threads, renewed every enc_cert init
    pub passphrase: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct MainCertificate {
    pub encrypted: bool,
    pub duration: i64,
    pub key_len: i64, // If the cert is CA signed, we use this instead of the `algorithm` key in `CertificateSettings`
    pub subj: String,
    pub main_paths: CertificatePaths,
    pub auxiliary_paths: Vec<CertificatePaths>,
    pub service_ips: Vec<String>,
    pub date_issued: Option<String>, // This is used for transferring the date between threads, renewed every enc_cert init
    pub passphrase: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct CertificatePaths {
    pub key: String,
    pub cert: String,
}

// NOTICE: Value for key `neutron_account_username` should only contain alpha-numeric characters. Others are not accepted by NEUS.
impl Default for Settings {
    fn default() -> Self {
        Self {
            neutron_account_username: String::new(),
            neutron_mqtt_client: NeutronMqttClient::default(),
            component_mqtt_client: ComponentMqttClient::default(),
            application_name: String::from("LSOC"),
            update_branch: String::from("stable"),
            update_components: vec![
                // UpdateComponent {
                //     name: String::from("BlackBox"),
                //     version_file_path: String::from("/etc/BlackBox/blackbox.version"),
                //     permission_user: String::from("root"),
                //     permission_group: String::from("root"),
                //     file_permissions: String::from("700"),
                //     container_name: None,
                //     service_name: Some(String::from("blackbox.service")),
                //     restart_command: String::from("sudo systemctl restart blackbox.service"),
                // },
                // UpdateComponent {
                //     name: String::from("WebInterface"),
                //     version_file_path: String::from(
                //         "/etc/LSOCWebInterface/webinterface_docker/webinterface.version",
                //     ),
                //     permission_user: String::from("web_interface"),
                //     permission_group: String::from("web_interface_group"),
                //     file_permissions: String::from("740"),
                //     container_name: Some(String::from("lsoc_web_interface")),
                //     service_name: None,
                //     restart_command: String::from("sudo docker restart lsoc_web_interface"),
                // },
                // UpdateComponent {
                //     name: String::from("Mosquitto"),
                //     version_file_path: String::from("/etc/BlackBox/mosquitto.version"),
                //     permission_user: String::from("root"),
                //     permission_group: String::from("mqttcontainergroup"),
                //     file_permissions: String::from("740"),
                //     container_name: Some(String::from("mosquitto")),
                //     service_name: None,
                //     restart_command: String::from("sudo docker restart mosquitto"),
                // },
            ],
            certificates: vec![],
        }
    }
}

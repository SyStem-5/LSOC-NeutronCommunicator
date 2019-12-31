/*#[derive(Deserialize, Serialize, Debug)]
pub struct CertificateKeyPair {
    pub certificate: String,
    pub key: String,
    pub date_issued: String,
}*/

/*
'key is not needed, because it is not changing when renewing a certificate'

1. component name
2. 'ca' | 'main' certificate
3. CRT data (read/loaded from the certificate itself)
**4. date issued

** -> This will not be included in the struct unless it proves useful
*/
#[derive(Deserialize, Serialize, Debug)]
pub struct CertRenewal {
    pub component_name: String,
    pub crt_type: String,
    pub crt_data: String,
}

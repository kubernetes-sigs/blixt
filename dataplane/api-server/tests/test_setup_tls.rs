use anyhow::Result;
use api_server::config::{MutualTLSConfig, ServerOnlyTLSConfig, TLSConfig};
use api_server::setup_tls;
use rcgen::{generate_simple_self_signed, Certificate, CertificateParams};
use std::fs;
use tempfile::tempdir;
use tonic::transport::Server;

#[tokio::test]
async fn test_tls_self_signed_cert() -> Result<()> {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();

    // Generate self-signed certificate
    let cert = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

    // Paths for the server cert and private key
    let cert_path = temp_dir.path().join("server.crt");
    let key_path = temp_dir.path().join("server.key");

    // Write cert and key to temp files
    fs::write(&cert_path, cert_pem.as_bytes())?;
    fs::write(&key_path, key_pem.as_bytes())?;

    // Set up a TLS config with paths to the cert and key
    let tls_config = Some(TLSConfig::TLS(ServerOnlyTLSConfig {
        server_certificate_path: cert_path.clone(),
        server_private_key_path: key_path.clone(),
    }));

    // Prepare a dummy server builder
    let builder = Server::builder();

    // Run the setup_tls function and ensure no error is thrown
    let result = setup_tls(builder, &tls_config);
    assert!(
        result.is_ok(),
        "setup_tls should succeed with valid self-signed certs"
    );
    Ok(())
}

#[tokio::test]
async fn test_mtls_self_signed_cert() -> Result<()> {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();

    // Generate self-signed certificate
    let cert = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.serialize_private_key_pem();

    // Generate CA
    let ca_params = CertificateParams::default();
    let ca_cert = Certificate::from_params(ca_params)?;
    let ca_cert_pem = ca_cert.serialize_pem()?;

    // Cert file paths
    let cert_path = temp_dir.path().join("server.crt");
    let key_path = temp_dir.path().join("server.key");
    let ca_cert_path = temp_dir.path().join("ca.crt");

    // Write cert and key to temp files
    fs::write(&cert_path, cert_pem.as_bytes())?;
    fs::write(&key_path, key_pem.as_bytes())?;
    fs::write(&ca_cert_path, ca_cert_pem.as_bytes())?;

    // Set up a TLS config with paths to the cert and key
    let tls_config = Some(TLSConfig::MutualTLS(MutualTLSConfig {
        server_certificate_path: cert_path.clone(),
        server_private_key_path: key_path.clone(),
        client_certificate_authority_root_path: ca_cert_path.clone(),
    }));

    // Prepare a dummy server builder
    let builder = Server::builder();

    // Run the setup_tls function and ensure no error is thrown
    let result = setup_tls(builder, &tls_config);
    assert!(
        result.is_ok(),
        "setup_tls should succeed with valid self-signed certs"
    );
    Ok(())
}

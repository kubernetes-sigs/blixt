use anyhow::Result;
use api_server::config::{MutualTLSConfig, ServerOnlyTLSConfig, TLSConfig};
use api_server::setup_tls;
use rcgen::{generate_simple_self_signed, CertificateParams, CertifiedKey};
use std::fs;
use tempfile::tempdir;
use tonic::transport::Server;

#[tokio::test]
async fn test_tls_self_signed_cert() -> Result<()> {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();

    // Generate self-signed certificate
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

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
async fn test_tls_missing_cert() -> Result<()> {
    let temp_dir = tempdir().unwrap();

    // Generate private key but skip certificate generation
    let CertifiedKey { cert: _, key_pair } = generate_simple_self_signed(vec!["localhost".into()])?;
    let key_pem = key_pair.serialize_pem();

    // Only write the key file, omit the certificate
    let missing_cert_path = temp_dir.path().join("missing_server.crt");
    let key_path = temp_dir.path().join("server.key");
    fs::write(&key_path, key_pem.as_bytes())?;

    // Set up a TLS config pointing to the missing certificate
    let tls_config = Some(TLSConfig::TLS(ServerOnlyTLSConfig {
        server_certificate_path: missing_cert_path.clone(),
        server_private_key_path: key_path.clone(),
    }));

    let builder = Server::builder();
    let result = setup_tls(builder, &tls_config);

    // Assert that the result is an error
    assert!(
        result.is_err(),
        "setup_tls should fail when the server certificate is missing"
    );
    Ok(())
}

#[tokio::test]
async fn test_tls_missing_key() -> Result<()> {
    let temp_dir = tempdir().unwrap();

    // Generate certificate but skip private key generation
    let CertifiedKey { cert, key_pair: _ } = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.pem();

    // Only write the certificate file, omit the private key
    let cert_path = temp_dir.path().join("server.crt");
    let missing_key_path = temp_dir.path().join("missing_server.key");
    fs::write(&cert_path, cert_pem.as_bytes())?;

    // Set up a TLS config pointing to the missing private key
    let tls_config = Some(TLSConfig::TLS(ServerOnlyTLSConfig {
        server_certificate_path: cert_path.clone(),
        server_private_key_path: missing_key_path.clone(),
    }));

    let builder = Server::builder();
    let result = setup_tls(builder, &tls_config);

    // Assert that the result is an error
    assert!(
        result.is_err(),
        "setup_tls should fail when the private key is missing"
    );
    Ok(())
}

#[tokio::test]
async fn test_mtls_self_signed_cert() -> Result<()> {
    // Create a temporary directory
    let temp_dir = tempdir().unwrap();

    // Generate self-signed certificate
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Generate CA
    let ca_params = CertificateParams::default();
    let ca_cert = ca_params.self_signed(&key_pair)?;
    let ca_cert_pem = ca_cert.pem();

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

#[tokio::test]
async fn test_mtls_invalid_ca_cert() -> Result<()> {
    let temp_dir = tempdir().unwrap();

    // Generate server cert and key
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Write valid server cert and key
    let cert_path = temp_dir.path().join("server.crt");
    let key_path = temp_dir.path().join("server.key");
    fs::write(&cert_path, cert_pem.as_bytes())?;
    fs::write(&key_path, key_pem.as_bytes())?;

    // Write an invalid CA certificate
    let invalid_ca_cert_path = temp_dir.path().join("invalid_ca.crt");
    fs::write(&invalid_ca_cert_path, b"not a valid certificate")?;

    let tls_config = Some(TLSConfig::MutualTLS(MutualTLSConfig {
        server_certificate_path: cert_path.clone(),
        server_private_key_path: key_path.clone(),
        client_certificate_authority_root_path: invalid_ca_cert_path.clone(),
    }));

    let builder = Server::builder();
    let result = setup_tls(builder, &tls_config);

    // Assert that the result is an error
    assert!(
        result.is_err(),
        "setup_tls should fail with an invalid CA certificate for mTLS"
    );
    Ok(())
}

#[tokio::test]
async fn test_mtls_missing_ca_cert() -> Result<()> {
    let temp_dir = tempdir().unwrap();

    // Generate server cert and key
    let CertifiedKey { cert, key_pair } = generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();

    // Write valid server cert and key
    let cert_path = temp_dir.path().join("server.crt");
    let key_path = temp_dir.path().join("server.key");
    fs::write(&cert_path, cert_pem.as_bytes())?;
    fs::write(&key_path, key_pem.as_bytes())?;

    // Path for the missing CA certificate
    let missing_ca_cert_path = temp_dir.path().join("missing_ca.crt");

    let tls_config = Some(TLSConfig::MutualTLS(MutualTLSConfig {
        server_certificate_path: cert_path.clone(),
        server_private_key_path: key_path.clone(),
        client_certificate_authority_root_path: missing_ca_cert_path.clone(),
    }));

    let builder = Server::builder();
    let result = setup_tls(builder, &tls_config);

    // Assert that the result is an error
    assert!(
        result.is_err(),
        "setup_tls should fail when the client CA certificate is missing"
    );
    Ok(())
}

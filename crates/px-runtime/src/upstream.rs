use std::fs::File;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use px_proto::{ClientConfig, ConnectRequest};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig as RustlsClientConfig, DigitallySignedStruct, Error,
    RootCertStore, SignatureScheme,
};
use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

#[derive(Clone)]
pub struct UpstreamConnector {
    server_addr: SocketAddr,
    connect_timeout: Duration,
    tls_connector: TlsConnector,
}

impl UpstreamConnector {
    pub fn new(config: &ClientConfig) -> Result<Self> {
        let server_addr: SocketAddr = config
            .server_addr
            .parse()
            .context("server_addr must be IP:port")?;
        let tls_config = build_tls_config(&config.server_cert_path)?;
        Ok(Self {
            server_addr,
            connect_timeout: Duration::from_millis(config.connect_timeout_ms),
            tls_connector: TlsConnector::from(Arc::new(tls_config)),
        })
    }

    pub async fn connect(&self, request: &ConnectRequest) -> Result<TlsStream<TcpStream>> {
        let tcp = timeout(self.connect_timeout, TcpStream::connect(self.server_addr))
            .await
            .context("tcp connect timeout")??;
        apply_socket_options(&tcp)?;

        let server_name = ServerName::IpAddress(match self.server_addr.ip() {
            IpAddr::V4(ip) => ip.into(),
            IpAddr::V6(ip) => ip.into(),
        });
        let mut tls_stream = self
            .tls_connector
            .connect(server_name, tcp)
            .await
            .context("tls connect failed")?;
        request.write_to(&mut tls_stream).await?;
        Ok(tls_stream)
    }
}

fn build_tls_config(path: &str) -> Result<RustlsClientConfig> {
    let certs = load_certs(path)?;
    let verifier = PinnedServerCertVerifier::new(certs)?;
    Ok(RustlsClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth())
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path).with_context(|| format!("failed to open pinned cert {path}"))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to parse pinned cert")?;
    Ok(certs)
}

fn apply_socket_options(stream: &TcpStream) -> Result<()> {
    stream.set_nodelay(true)?;
    let sock = SockRef::from(stream);
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(30));
    sock.set_tcp_keepalive(&keepalive)?;
    Ok(())
}

#[derive(Debug)]
struct PinnedServerCertVerifier {
    pinned_cert: CertificateDer<'static>,
    delegate: Arc<WebPkiServerVerifier>,
}

impl PinnedServerCertVerifier {
    fn new(mut certs: Vec<CertificateDer<'static>>) -> Result<Self> {
        if certs.len() != 1 {
            return Err(anyhow!(
                "pinned cert file must contain exactly one certificate"
            ));
        }

        let pinned_cert = certs.remove(0);
        let mut roots = RootCertStore::empty();
        roots.add(pinned_cert.clone())?;
        let delegate = WebPkiServerVerifier::builder(Arc::new(roots))
            .build()
            .map_err(|error| anyhow!("failed to build pinned cert verifier: {error}"))?;

        Ok(Self {
            pinned_cert,
            delegate,
        })
    }
}

impl ServerCertVerifier for PinnedServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, Error> {
        if end_entity.as_ref() != self.pinned_cert.as_ref() {
            return Err(Error::InvalidCertificate(
                CertificateError::ApplicationVerificationFailure,
            ));
        }

        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        self.delegate.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, Error> {
        self.delegate.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.delegate.supported_verify_schemes()
    }
}

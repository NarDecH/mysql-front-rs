//! SSH port forwarding: one local TCP listener whose connections are
//! relayed through an SSH server to a remote MySQL port
//! (`direct-tcpip` channels, i.e. plain `ssh -L` semantics).
//!
//! The tunnel runs on its own thread with a single-threaded tokio
//! runtime; dropping [`SshTunnel`] stops it.

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::copy_bidirectional;

use crate::error::{MffError, Result};

/// Where to connect and what to reach through the tunnel.
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    /// Password for password auth, or the passphrase for an encrypted key.
    pub ssh_password: String,
    /// Optional OpenSSH private key file (password auth when empty).
    pub ssh_key_path: Option<String>,
    /// The MySQL server, as seen from the SSH host.
    pub remote_host: String,
    pub remote_port: u16,
}

/// A live tunnel. Keep it alive for as long as the session uses it.
pub struct SshTunnel {
    pub local_port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl SshTunnel {
    /// Bind a local port and start the relay thread. Returns after the
    /// listener is bound (the SSH handshake happens in the background —
    /// connection errors surface on the first query as connection reset).
    pub fn start(t: &SshTarget) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|e| MffError::Other(format!("tunnel bind: {e}")))?;
        let local_port = listener
            .local_addr()
            .map_err(|e| MffError::Other(format!("tunnel addr: {e}")))?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let target = t.clone();
        let worker = std::thread::Builder::new()
            .name("ssh-tunnel".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime");
                rt.block_on(async move {
                    if let Err(e) = relay(target, listener, stop2).await {
                        eprintln!("ssh tunnel ended: {e}");
                    }
                });
            })
            .map_err(|e| MffError::Other(format!("tunnel thread: {e}")))?;
        Ok(Self {
            local_port,
            stop,
            worker: Some(worker),
        })
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.worker.take() {
            let _ = h.join();
        }
    }
}

/// Accept loop: every inbound TCP connection becomes one
/// `direct-tcpip` channel, relayed byte for byte.
async fn relay(
    t: SshTarget,
    listener: TcpListener,
    stop: Arc<AtomicBool>,
) -> std::result::Result<(), String> {
    listener
        .set_nonblocking(true)
        .map_err(|e| e.to_string())?;

    let config = Arc::new(russh::client::Config::default());
    let addr = (t.ssh_host.clone(), t.ssh_port);
    let mut handle = match russh::client::connect(
        config,
        addr.clone(),
        Handler {
            host: t.ssh_host.clone(),
            port: t.ssh_port,
        },
    )
    .await
    {
        Ok(h) => h,
        Err(russh::Error::UnknownKey) => {
            return Err(format!(
                "ssh host key for {}:{} changed or could not be verified — \
                 if the server really changed, delete its line in \
                 ~/.ssh/known_hosts and reconnect",
                addr.0, addr.1
            ));
        }
        Err(e) => return Err(format!("ssh connect: {e}")),
    };

    // Authenticate: key file when given, otherwise password.
    if let Some(path) = &t.ssh_key_path {
        let passphrase = if t.ssh_password.is_empty() {
            None
        } else {
            Some(t.ssh_password.as_str())
        };
        let key = russh::keys::load_secret_key(path, passphrase)
            .map_err(|e| format!("ssh key: {e}"))?;
        let key = russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), None);
        let auth = handle
            .authenticate_publickey(&t.ssh_user, key)
            .await
            .map_err(|e| format!("ssh auth: {e}"))?;
        if !auth.success() {
            return Err("ssh public key authentication failed".into());
        }
    } else {
        let auth = handle
            .authenticate_password(&t.ssh_user, &t.ssh_password)
            .await
            .map_err(|e| format!("ssh auth: {e}"))?;
        if !auth.success() {
            return Err("ssh password authentication failed".into());
        }
    }

    let handle = Arc::new(tokio::sync::Mutex::new(handle));
    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(true);
                let handle = Arc::clone(&handle);
                let (rh, rp) = (t.remote_host.clone(), t.remote_port);
                tokio::spawn(async move {
                    // Serialize channel opens on the one SSH session.
                    let opened = {
                        let mut handle = handle.lock().await;
                        handle
                            .channel_open_direct_tcpip(rh, rp as u32, "127.0.0.1", 0)
                            .await
                    };
                    match opened {
                        Ok(channel) => {
                            let mut ch = channel.into_stream();
                            match tokio::net::TcpStream::from_std(stream) {
                                Ok(mut stream) => {
                                    let _ = copy_bidirectional(&mut stream, &mut ch).await;
                                }
                                Err(e) => eprintln!("tunnel stream: {e}"),
                            }
                        }
                        Err(e) => eprintln!("ssh channel open: {e}"),
                    }
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            Err(e) => return Err(format!("tunnel accept: {e}")),
        }
    }
}

/// Trust-on-first-use host key verification against the user's standard
/// `~/.ssh/known_hosts`: known keys must match, unknown keys are learned,
/// a changed key refuses the connection (possible man-in-the-middle).
struct Handler {
    host: String,
    port: u16,
}

/// The user's OpenSSH known_hosts file.
fn known_hosts_path() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".ssh").join("known_hosts"))
}

impl russh::client::Handler for Handler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key
        else {
            // Certificates are not pinned; accept.
            return Ok(true);
        };
        let Some(path) = known_hosts_path() else {
            return Ok(true);
        };
        match russh::keys::check_known_hosts_path(&self.host, self.port, key, &path) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // First contact: record the key (TOFU) and continue.
                let _ = russh::keys::known_hosts::learn_known_hosts_path(
                    &self.host,
                    self.port,
                    key,
                    &path,
                );
                Ok(true)
            }
            // Key mismatch = possible man-in-the-middle: refuse.
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::known_hosts_path;

    const KEY_A: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBeho8NEkcXMBIheNQQw6yZW/qHNoeJ+d8hX1rrm06Vy";
    const KEY_B: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOAZNpCgUudICzXK/APRY9kzso+MqxuZGXhVo0VbDBZi";

    #[test]
    fn known_hosts_file_resolves() {
        // The handler silently accepts when it cannot resolve the file;
        // on this machine it must exist so pinning is actually active.
        assert!(known_hosts_path().is_some());
    }

    #[test]
    fn tofu_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        let pk_a = russh::keys::ssh_key::PublicKey::from_openssh(KEY_A).unwrap();
        let pk_b = russh::keys::ssh_key::PublicKey::from_openssh(KEY_B).unwrap();
        // First contact: not yet known.
        assert!(!russh::keys::check_known_hosts_path("mff.test", 22, &pk_a, &path).unwrap());
        // Learning the key makes the next contact trusted.
        russh::keys::known_hosts::learn_known_hosts_path("mff.test", 22, &pk_a, &path).unwrap();
        assert!(russh::keys::check_known_hosts_path("mff.test", 22, &pk_a, &path).unwrap());
        // A different key under the same name = possible MITM → error.
        assert!(russh::keys::check_known_hosts_path("mff.test", 22, &pk_b, &path).is_err());
        // Other hosts and ports are unaffected.
        assert!(!russh::keys::check_known_hosts_path("other.test", 22, &pk_a, &path).unwrap());
        assert!(!russh::keys::check_known_hosts_path("mff.test", 2222, &pk_a, &path).unwrap());
    }
}

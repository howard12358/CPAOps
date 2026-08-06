use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
use cpactl::github::GithubClient;
use cpactl::storage::config::{ConfigStore, ProxyConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn test_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("cpactl-github-test-{nonce}-{counter}"))
}

fn config_store(root: &std::path::Path) -> ConfigStore {
    ConfigStore::new(RuntimePaths::from_root(root.to_path_buf()).unwrap())
}

struct TestServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

struct SocksServer {
    url: String,
    targets: Arc<Mutex<Vec<String>>>,
}

impl SocksServer {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let targets = Arc::new(Mutex::new(Vec::new()));
        let recorded_targets = Arc::clone(&targets);
        tokio::spawn(async move {
            let (mut client, _) = listener.accept().await.unwrap();
            let mut greeting = [0_u8; 2];
            client.read_exact(&mut greeting).await.unwrap();
            let mut methods = vec![0_u8; usize::from(greeting[1])];
            client.read_exact(&mut methods).await.unwrap();
            client.write_all(&[5, 0]).await.unwrap();

            let mut request = [0_u8; 4];
            client.read_exact(&mut request).await.unwrap();
            assert_eq!(request[..3], [5, 1, 0]);
            let host = match request[3] {
                1 => {
                    let mut address = [0_u8; 4];
                    client.read_exact(&mut address).await.unwrap();
                    std::net::Ipv4Addr::from(address).to_string()
                }
                3 => {
                    let mut length = [0_u8; 1];
                    client.read_exact(&mut length).await.unwrap();
                    let mut name = vec![0_u8; usize::from(length[0])];
                    client.read_exact(&mut name).await.unwrap();
                    String::from_utf8(name).unwrap()
                }
                atyp => panic!("unexpected SOCKS address type: {atyp}"),
            };
            let mut port = [0_u8; 2];
            client.read_exact(&mut port).await.unwrap();
            let target = format!("{host}:{}", u16::from_be_bytes(port));
            recorded_targets.lock().unwrap().push(target.clone());
            let mut upstream = tokio::net::TcpStream::connect(target).await.unwrap();
            client
                .write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
            tokio::io::copy_bidirectional(&mut client, &mut upstream)
                .await
                .unwrap();
        });
        Self {
            url: format!("socks5://{address}"),
            targets,
        }
    }

    fn targets(&self) -> Vec<String> {
        self.targets.lock().unwrap().clone()
    }
}

impl TestServer {
    async fn start(responses: Vec<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded_requests = Arc::clone(&requests);
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                recorded_requests
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });
        Self {
            base_url: format!("http://{address}"),
            requests,
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

fn response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn retries_with_saved_token_only_after_forbidden() {
    let server = TestServer::start(vec![
        response("403 Forbidden", "forbidden"),
        response("200 OK", r#"{"tag_name":"v1.2.3","assets":[]}"#),
    ])
    .await;
    let root = test_root();
    let config = config_store(&root);
    config.save_token("stored-token").unwrap();
    let client = GithubClient::with_api_base(config, server.base_url.clone()).unwrap();

    let release = client.latest_release(Service::Cli).await.unwrap();

    assert_eq!(release.tag, "v1.2.3");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer stored-token\r\n")
    );
    if root.exists() {
        fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn retries_with_saved_token_only_after_unauthorized() {
    let server = TestServer::start(vec![
        response("401 Unauthorized", "unauthorized"),
        response("200 OK", r#"{"tag_name":"v1.2.4","assets":[]}"#),
    ])
    .await;
    let root = test_root();
    let config = config_store(&root);
    config.save_token("stored-token").unwrap();
    let client = GithubClient::with_api_base(config, server.base_url.clone()).unwrap();

    let release = client.latest_release(Service::Cli).await.unwrap();

    assert_eq!(release.tag, "v1.2.4");
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("authorization: bearer stored-token\r\n")
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn sends_github_requests_through_saved_socks5_proxy() {
    let server = TestServer::start(vec![response(
        "200 OK",
        r#"{"tag_name":"v1.2.5","assets":[]}"#,
    )])
    .await;
    let socks = SocksServer::start().await;
    let root = test_root();
    let config = config_store(&root);
    let proxy = ProxyConfig::parse(&format!("all_proxy={}", socks.url)).unwrap();
    config.save_proxy(&proxy).unwrap();
    let client = GithubClient::with_api_base(config, server.base_url.clone()).unwrap();

    let release = client.latest_release(Service::Cli).await.unwrap();

    assert_eq!(release.tag, "v1.2.5");
    assert_eq!(socks.targets(), vec![server.base_url[7..].to_string()]);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn download_persists_completed_response_without_temporary_file() {
    let server = TestServer::start(vec![response("200 OK", "verified archive")]).await;
    let root = test_root();
    let destination = root.join("downloads").join("asset.tar.gz");
    let client = GithubClient::with_api_base(config_store(&root), server.base_url.clone()).unwrap();

    let downloaded = client
        .download(
            &format!("{}/assets/asset.tar.gz", server.base_url),
            &destination,
        )
        .await
        .unwrap();

    assert_eq!(downloaded, destination);
    assert_eq!(fs::read(&downloaded).unwrap(), b"verified archive");
    assert_eq!(
        fs::read_dir(downloaded.parent().unwrap()).unwrap().count(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn download_error_does_not_expose_sensitive_url() {
    let server = TestServer::start(vec![response("500 Internal Server Error", "failed")]).await;
    let root = test_root();
    let config = config_store(&root);
    config.save_token("stored-token").unwrap();
    let client = GithubClient::with_api_base(config, server.base_url.clone()).unwrap();
    let sensitive_url = format!("{}/private/super-secret-asset", server.base_url);

    let error = client
        .download(&sensitive_url, &root.join("downloads").join("asset.tar.gz"))
        .await
        .unwrap_err();

    let message = error.to_string();
    assert!(!message.contains("super-secret-asset"));
    assert!(!message.contains(&server.base_url));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    if root.exists() {
        fs::remove_dir_all(root).unwrap();
    }
}

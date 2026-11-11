use bar_identity::{EnvironmentId, EnvironmentMachineKind};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    environment_icon: Option<EnvironmentMachineKind>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Descriptor {
    environment_id: EnvironmentId,
    platform: Platform,
}

#[derive(Deserialize)]
struct Platform {
    machine: Option<EnvironmentMachineKind>,
}

pub fn local_machine(
    state_dir: &Path,
    descriptor_machine: Option<EnvironmentMachineKind>,
) -> EnvironmentMachineKind {
    let settings = std::fs::read(state_dir.join("settings.json"))
        .ok()
        .and_then(|data| serde_json::from_slice::<Settings>(&data).ok());
    settings
        .and_then(|settings| settings.environment_icon)
        .or(descriptor_machine)
        .unwrap_or(EnvironmentMachineKind::Server)
}

pub async fn descriptor_machine(
    port: u16,
    expected: &EnvironmentId,
) -> Option<EnvironmentMachineKind> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()?;
    let descriptor = client
        .get(format!(
            "http://127.0.0.1:{port}/.well-known/t3/environment"
        ))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Descriptor>()
        .await
        .ok()?;
    (descriptor.environment_id == *expected)
        .then_some(descriptor.platform.machine)
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_machine_from_matching_server_descriptor() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 2048];
            let n = stream.read(&mut request).unwrap();
            assert!(std::str::from_utf8(&request[..n])
                .unwrap()
                .starts_with("GET /.well-known/t3/environment "));
            let body = r#"{"environmentId":"local","platform":{"machine":"linux"}}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let machine = runtime.block_on(descriptor_machine(
            port,
            &EnvironmentId::new("local").unwrap(),
        ));
        server.join().unwrap();
        assert_eq!(machine, Some(EnvironmentMachineKind::Linux));
    }

    #[test]
    fn explicit_icon_precedes_descriptor_and_descriptor_precedes_server() {
        let path = std::env::temp_dir().join(format!("bar-machine-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("settings.json"),
            r#"{"environmentIcon":"mac-mini"}"#,
        )
        .unwrap();
        assert_eq!(
            local_machine(&path, Some(EnvironmentMachineKind::Linux)),
            EnvironmentMachineKind::MacMini
        );
        std::fs::write(path.join("settings.json"), r#"{"environmentIcon":null}"#).unwrap();
        assert_eq!(
            local_machine(&path, Some(EnvironmentMachineKind::Linux)),
            EnvironmentMachineKind::Linux
        );
        assert_eq!(local_machine(&path, None), EnvironmentMachineKind::Server);
        std::fs::remove_dir_all(path).unwrap();
    }
}

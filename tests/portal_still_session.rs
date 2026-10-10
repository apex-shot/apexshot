use apexshot::backend::portal_still::{PortalStillSession, StillError, StillMonitor};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use zbus::{
    message::Header,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection,
};

#[derive(Default)]
struct State {
    creates: usize,
    starts: usize,
    closes: usize,
    remotes: usize,
    tokens: Vec<Option<String>>,
    cancel: bool,
    reject_restore: bool,
    wrong_monitor: bool,
    reject_remote: bool,
    registered: Vec<String>,
}

struct Portal(Arc<Mutex<State>>);
struct PortalSession(Arc<Mutex<State>>);
struct Request;
struct Registry(Arc<Mutex<State>>);

#[zbus::interface(name = "org.freedesktop.host.portal.Registry")]
impl Registry {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    fn register(&self, app_id: String, _options: HashMap<String, OwnedValue>) {
        self.0.lock().unwrap().registered.push(app_id);
    }
}

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl PortalSession {
    fn close(&self) {
        self.0.lock().unwrap().closes += 1;
    }
}

#[zbus::interface(name = "org.freedesktop.portal.Request")]
impl Request {
    fn close(&self) {}
}

fn owned<T: serde::Serialize + zbus::zvariant::Type + Into<Value<'static>>>(
    value: T,
) -> OwnedValue {
    OwnedValue::try_from(Value::new(value)).unwrap()
}

async fn respond(
    connection: &Connection,
    sender: &str,
    token: &str,
    response: u32,
    results: HashMap<String, OwnedValue>,
) -> zbus::fdo::Result<OwnedObjectPath> {
    let path = OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/request/{}/{}",
        sender.trim_start_matches(':').replace('.', "_"),
        token
    ))
    .unwrap();
    connection.object_server().at(path.clone(), Request).await?;
    let signal_path = path.clone();
    let connection = connection.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        connection
            .emit_signal(
                None::<&str>,
                signal_path,
                "org.freedesktop.portal.Request",
                "Response",
                &(response, results),
            )
            .await
            .unwrap();
    });
    Ok(path)
}

#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl Portal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
    }

    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        1
    }

    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        3
    }

    async fn create_session(
        &self,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.0.lock().unwrap().creates += 1;
        let sender = header.sender().unwrap().as_str();
        let session_token = <&str>::try_from(options.get("session_handle_token").unwrap()).unwrap();
        let session_path = format!(
            "/org/freedesktop/portal/desktop/session/{}/{}",
            sender.trim_start_matches(':').replace('.', "_"),
            session_token
        );
        connection
            .object_server()
            .at(session_path.clone(), PortalSession(self.0.clone()))
            .await?;
        let mut results = HashMap::new();
        results.insert("session_handle".into(), owned(session_path));
        respond(
            connection,
            sender,
            <&str>::try_from(options.get("handle_token").unwrap()).unwrap(),
            0,
            results,
        )
        .await
    }

    async fn select_sources(
        &self,
        _session: OwnedObjectPath,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        assert_eq!(u32::try_from(options.get("types").unwrap()).unwrap(), 1);
        assert!(!bool::try_from(options.get("multiple").unwrap()).unwrap());
        assert_eq!(
            u32::try_from(options.get("persist_mode").unwrap()).unwrap(),
            2
        );
        let token = options
            .get("restore_token")
            .map(|value| <&str>::try_from(value).unwrap().to_owned());
        let response = {
            let mut state = self.0.lock().unwrap();
            state.tokens.push(token.clone());
            if token.is_some() && state.reject_restore {
                state.reject_restore = false;
                2
            } else {
                0
            }
        };
        respond(
            connection,
            header.sender().unwrap().as_str(),
            <&str>::try_from(options.get("handle_token").unwrap()).unwrap(),
            response,
            HashMap::new(),
        )
        .await
    }

    async fn start(
        &self,
        _session: OwnedObjectPath,
        _parent: String,
        options: HashMap<String, OwnedValue>,
        #[zbus(connection)] connection: &Connection,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let (response, results) = {
            let mut state = self.0.lock().unwrap();
            state.starts += 1;
            let mut results = HashMap::new();
            if state.cancel {
                state.cancel = false;
                (1, results)
            } else {
                let mut properties = HashMap::new();
                properties.insert(
                    "position".to_owned(),
                    owned((if state.wrong_monitor { 1920_i32 } else { 0_i32 }, -100_i32)),
                );
                properties.insert("size".to_owned(), owned((1920_i32, 1080_i32)));
                properties.insert("source_type".to_owned(), owned(1_u32));
                results.insert("streams".to_owned(), owned(vec![(42_u32, properties)]));
                results.insert(
                    "restore_token".to_owned(),
                    owned(format!("test-grant-{}", state.starts)),
                );
                (0, results)
            }
        };
        respond(
            connection,
            header.sender().unwrap().as_str(),
            <&str>::try_from(options.get("handle_token").unwrap()).unwrap(),
            response,
            results,
        )
        .await
    }

    fn open_pipe_wire_remote(
        &self,
        _session: OwnedObjectPath,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<zbus::zvariant::OwnedFd> {
        let rejected = {
            let mut state = self.0.lock().unwrap();
            state.remotes += 1;
            state.reject_remote
        };
        if rejected {
            return Err(zbus::fdo::Error::Failed(
                "Test remote is unavailable".into(),
            ));
        }
        let (fd, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd: std::os::fd::OwnedFd = fd.into();
        Ok(fd.into())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn retained_portal_sessions_rotate_grants_and_stop_on_cancellation() {
    if std::env::var_os("APEXSHOT_TEST_PRIVATE_PORTAL").is_none() {
        eprintln!("Run with APEXSHOT_TEST_PRIVATE_PORTAL=1 under dbus-run-session to test the private portal fixture");
        return;
    }
    let state = Arc::new(Mutex::new(State::default()));
    let _service = zbus::connection::Builder::session()
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Portal(state.clone()))
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Registry(state.clone()))
        .unwrap()
        .build()
        .await
        .unwrap();
    let monitor = StillMonitor {
        x: 0,
        y: -100,
        width: 1920,
        height: 1080,
    };
    let key = format!("private-portal-fixture-{}", std::process::id());

    let first = PortalStillSession::prepare(&key, monitor, false)
        .await
        .unwrap();
    assert_eq!(first.position, Some((0, -100)));
    assert!(first.capture().await.is_err());
    assert!(first.capture().await.is_err());
    assert_eq!(state.lock().unwrap().starts, 1);
    assert_eq!(state.lock().unwrap().remotes, 3);
    first.close().await;
    let second = PortalStillSession::prepare(&key, monitor, false)
        .await
        .unwrap();
    second.close().await;
    assert_eq!(
        state.lock().unwrap().tokens,
        vec![None, Some("test-grant-1".into())]
    );
    assert_eq!(state.lock().unwrap().closes, 2);

    state.lock().unwrap().cancel = true;
    let cancelled = PortalStillSession::prepare(&key, monitor, false).await;
    assert!(matches!(cancelled, Err(StillError::Cancelled)));
    assert_eq!(state.lock().unwrap().creates, 3);
    assert_eq!(
        state.lock().unwrap().tokens.last().unwrap(),
        &Some("test-grant-2".into())
    );

    let fourth = PortalStillSession::prepare(&key, monitor, false)
        .await
        .unwrap();
    fourth.close().await;
    assert_eq!(state.lock().unwrap().tokens.last().unwrap(), &None);
    state.lock().unwrap().reject_restore = true;
    let restored = PortalStillSession::prepare(&key, monitor, false)
        .await
        .unwrap();
    restored.close().await;
    assert_eq!(state.lock().unwrap().creates, 6);
    assert_eq!(
        &state.lock().unwrap().tokens[4..],
        &[Some("test-grant-4".into()), None]
    );

    state.lock().unwrap().wrong_monitor = true;
    assert!(
        PortalStillSession::prepare(&format!("{key}-other-monitor"), monitor, false)
            .await
            .is_err()
    );
    assert_eq!(state.lock().unwrap().creates, 7);
    assert_eq!(state.lock().unwrap().closes, 7);
    {
        let mut state = state.lock().unwrap();
        state.wrong_monitor = false;
        state.reject_remote = true;
    }
    let producer_key = format!("{key}-producer-error");
    assert!(PortalStillSession::prepare(&producer_key, monitor, false)
        .await
        .is_err());
    assert_eq!(state.lock().unwrap().creates, 8);
    assert_eq!(state.lock().unwrap().closes, 8);
    state.lock().unwrap().reject_remote = false;
    let recovered = PortalStillSession::prepare(&producer_key, monitor, false)
        .await
        .unwrap();
    recovered.close().await;
    assert_eq!(
        state.lock().unwrap().tokens.last().unwrap(),
        &Some("test-grant-7".into())
    );
    assert_eq!(state.lock().unwrap().closes, 9);
    state.lock().unwrap().reject_remote = true;
    assert!(PortalStillSession::prepare(&producer_key, monitor, false)
        .await
        .is_err());
    assert_eq!(state.lock().unwrap().creates, 10);
    assert_eq!(state.lock().unwrap().closes, 10);
    if !apexshot::app_identity::portal_only() {
        assert_eq!(
            state.lock().unwrap().registered,
            vec![apexshot::app_identity::app_id().to_owned(); 10]
        );
    }
}

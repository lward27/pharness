//! Campus browser terminals over real sockets: ticket issuance, the
//! WebSocket protocol, revocation, restart and limits, on the fake terminal.

use super::labs::{act, catalog_value, enroll_students, id, launched, parse};
use super::{
    body_json, request, test_config, Harness, ProcessParts, Session, ISSUER, ORIGIN, TENANT,
};
use crate::app::campus::terminal::{
    load_campus_terminals, terminal_admissible, TerminalDenial, TERMINAL_PROTOCOL,
};
use crate::app::campus::terminal_registry::{CampusTerminalLimits, CampusTerminals, TerminalGrant};
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::Router;
use futures::{SinkExt, StreamExt};
use pharness_core::campus_terminal::{
    CampusTerminalError, CampusTerminalSize, FakeCampusTerminal, FAKE_AGENT_CLI_VERSION,
    FAKE_TERMINAL_PROMPT, TERMINAL_OUTPUT_CHUNK_BYTES,
};
use pharness_store::{CampusIdentityKey, SqliteStore};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Instant};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const WAIT: Duration = Duration::from_secs(5);
/// Revocation must close a terminal well inside the 60 second bound; the
/// tests hold it to this.
const CLOSE_BOUND: Duration = Duration::from_secs(2);

fn fast_limits() -> CampusTerminalLimits {
    CampusTerminalLimits {
        recheck_interval: Duration::from_millis(200),
        auth_timeout: Duration::from_secs(1),
        send_timeout: Duration::from_secs(2),
        ..CampusTerminalLimits::default()
    }
}

struct Fixture {
    harness: Harness,
    fake: FakeCampusTerminal,
    terminals: Arc<CampusTerminals>,
    addr: SocketAddr,
}

async fn serve(app: Router) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn fixture_with(limits: CampusTerminalLimits, on_disk: bool) -> Fixture {
    let fake = FakeCampusTerminal::new();
    let terminals = Arc::new(CampusTerminals::new(Arc::new(fake.clone()), limits));
    let harness = Harness::with_labs_and_terminals(
        parse(&catalog_value(5)).unwrap(),
        true,
        on_disk,
        Some(terminals.clone()),
    )
    .await;
    let addr = serve(harness.app.clone()).await;
    Fixture {
        harness,
        fake,
        terminals,
        addr,
    }
}

async fn fixture() -> Fixture {
    fixture_with(fast_limits(), false).await
}

/// A logged-in student with a ready lab-1 session.
async fn ready_session(harness: &Harness, subject: &str) -> (Session, String) {
    enroll_students(harness, "cs101", &[subject]).await;
    let session = harness.login(subject).await;
    let view = launched(harness, &session, "lab-1").await;
    harness.reconcile().await;
    (session, id(&view))
}

fn ticket_path(session_id: &str) -> String {
    format!("/api/campus/lab-sessions/{session_id}/terminal-ticket")
}

async fn ticket_response(harness: &Harness, session: &Session, id: &str) -> (StatusCode, Value) {
    let response = harness
        .mutate("POST", &ticket_path(id), session, json!({}))
        .await;
    let status = response.status();
    (status, body_json(response).await)
}

async fn ticket(harness: &Harness, session: &Session, id: &str) -> String {
    let (status, body) = ticket_response(harness, session, id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["ticket"].as_str().unwrap().to_string()
}

async fn connect(
    addr: SocketAddr,
    id: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
) -> Result<Ws, Box<WsError>> {
    let mut request = format!("ws://{addr}/api/campus/lab-sessions/{id}/terminal")
        .into_client_request()
        .unwrap();
    let headers = request.headers_mut();
    if let Some(cookie) = cookie {
        headers.insert("cookie", cookie.parse().unwrap());
    }
    if let Some(origin) = origin {
        headers.insert("origin", origin.parse().unwrap());
    }
    headers.insert("sec-websocket-protocol", TERMINAL_PROTOCOL.parse().unwrap());
    let (ws, response) = tokio_tungstenite::connect_async(request).await?;
    assert_eq!(
        response.headers()["sec-websocket-protocol"],
        TERMINAL_PROTOCOL
    );
    Ok(ws)
}

fn refused_status(result: Result<Ws, Box<WsError>>) -> u16 {
    match result.map_err(|error| *error) {
        Err(WsError::Http(response)) => response.status().as_u16(),
        Err(other) => panic!("unexpected handshake error {other}"),
        Ok(_) => panic!("the handshake was accepted"),
    }
}

async fn send_auth(ws: &mut Ws, ticket: &str) {
    ws.send(Message::Text(
        json!({ "type": "auth", "ticket": ticket, "cols": 80, "rows": 24 }).to_string(),
    ))
    .await
    .unwrap();
}

async fn type_line(ws: &mut Ws, line: &str) {
    ws.send(Message::Binary(format!("{line}\r").into_bytes()))
        .await
        .unwrap();
}

async fn next_text(ws: &mut Ws) -> Value {
    loop {
        match timeout(WAIT, ws.next()).await.expect("no message in time") {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).unwrap(),
            Some(Ok(Message::Close(frame))) => panic!("closed: {frame:?}"),
            Some(Ok(_)) => continue,
            other => panic!("socket ended: {other:?}"),
        }
    }
}

/// Connect, authenticate and wait for `ready`.
async fn open(fixture: &Fixture, id: &str, session: &Session, ticket: &str) -> Ws {
    let mut ws = connect(fixture.addr, id, Some(&session.cookie), Some(ORIGIN))
        .await
        .unwrap();
    send_auth(&mut ws, ticket).await;
    let ready = next_text(&mut ws).await;
    assert_eq!(ready["type"], "ready", "{ready}");
    assert!(ready["connection_id"].as_str().unwrap().starts_with("ctc_"));
    ws
}

/// Terminal output until it contains `needle`.
async fn output_until(ws: &mut Ws, needle: &str) -> String {
    let mut output = String::new();
    let deadline = Instant::now() + WAIT;
    while !output.contains(needle) {
        let frame = timeout(
            deadline.saturating_duration_since(Instant::now()),
            ws.next(),
        )
        .await
        .unwrap_or_else(|_| panic!("{needle:?} not seen in {output:?}"));
        match frame {
            Some(Ok(Message::Binary(bytes))) => output.push_str(&String::from_utf8_lossy(&bytes)),
            Some(Ok(Message::Text(_) | Message::Ping(_) | Message::Pong(_))) => {}
            other => panic!("socket ended before {needle:?}: {other:?} after {output:?}"),
        }
    }
    output
}

#[derive(Debug)]
struct Closed {
    code: Option<u16>,
    reason: Option<String>,
}

/// Read until the server closes; the reason is the `closed` status message
/// when one was sent.
async fn closed(ws: &mut Ws) -> Closed {
    let mut status = None;
    loop {
        match timeout(WAIT, ws.next())
            .await
            .expect("the terminal did not close in time")
        {
            Some(Ok(Message::Text(text))) => {
                let message: Value = serde_json::from_str(&text).unwrap();
                if message["type"] == "closed" {
                    status = message["reason"].as_str().map(str::to_string);
                }
            }
            Some(Ok(Message::Close(frame))) => {
                if let (Some(frame), Some(status)) = (&frame, &status) {
                    assert_eq!(frame.reason.as_ref(), status.as_str());
                }
                return Closed {
                    code: frame.as_ref().map(|frame| u16::from(frame.code)),
                    reason: status.or_else(|| frame.map(|frame| frame.reason.to_string())),
                };
            }
            Some(Ok(_)) => {}
            None | Some(Err(_)) => {
                return Closed {
                    code: None,
                    reason: status,
                }
            }
        }
    }
}

async fn assert_closed(ws: &mut Ws, code: u16, reason: &str) {
    let closed = closed(ws).await;
    assert_eq!(closed.code, Some(code), "{closed:?}");
    assert_eq!(closed.reason.as_deref(), Some(reason), "{closed:?}");
}

/// Present `ticket` on a fresh socket and expect the server to close it.
async fn presented(fixture: &Fixture, id: &str, session: &Session, ticket: &str) -> Closed {
    let mut ws = connect(fixture.addr, id, Some(&session.cookie), Some(ORIGIN))
        .await
        .unwrap();
    send_auth(&mut ws, ticket).await;
    closed(&mut ws).await
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !check() {
        assert!(Instant::now() < deadline, "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn terminal_events(harness: &Harness, id: &str) -> Vec<(String, Value)> {
    harness
        .store
        .list_campus_lab_session_events(id)
        .await
        .unwrap()
        .into_iter()
        .filter(|event| event.kind.starts_with("campus_terminal."))
        .map(|event| (event.kind, event.payload))
        .collect()
}

fn policy_pids(harness: &Harness) -> u32 {
    harness
        .labs
        .as_ref()
        .unwrap()
        .catalog
        .assignment("cs101", "lab-1")
        .unwrap()
        .policy
        .constraints()
        .pids
}

#[tokio::test]
async fn the_owner_runs_the_agent_cli_in_their_own_terminal() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;

    let (status, body) = ticket_response(harness, &alice, &session_id).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["protocol"], TERMINAL_PROTOCOL);
    assert_eq!(
        body["socket_path"],
        format!("/api/campus/lab-sessions/{session_id}/terminal")
    );
    assert_eq!(body["expires_at"], harness.now() + 30);
    let ticket = body["ticket"].as_str().unwrap().to_string();
    assert_eq!(ticket.len(), 43);
    let text = body.to_string();
    for internal in ["slot", "namespace", "pod", "workspace", "pids"] {
        assert!(!text.contains(internal), "{internal} leaked in {text}");
    }

    let mut ws = open(&fixture, &session_id, &alice, &ticket).await;
    output_until(&mut ws, FAKE_TERMINAL_PROMPT).await;
    type_line(&mut ws, "codex --version").await;
    output_until(&mut ws, FAKE_AGENT_CLI_VERSION).await;
    ws.send(Message::Text(
        json!({ "type": "resize", "cols": 120, "rows": 40 }).to_string(),
    ))
    .await
    .unwrap();
    type_line(&mut ws, "stty size").await;
    output_until(&mut ws, "40 120").await;
    type_line(&mut ws, "ulimit -u").await;
    let pids = policy_pids(harness);
    output_until(&mut ws, &format!("\r\n{pids}\r\n")).await;

    let record = harness
        .store
        .get_campus_lab_session_record(&session_id)
        .await
        .unwrap()
        .unwrap();
    let attaches = fixture.fake.attaches();
    assert_eq!(attaches.len(), 1);
    assert_eq!(attaches[0].workload, record.session.workload().unwrap());
    assert_eq!(attaches[0].pids, pids);
    assert_eq!(attaches[0].size, CampusTerminalSize::DEFAULT);
    assert_eq!(fixture.terminals.open_count(&session_id), 1);

    type_line(&mut ws, "exit").await;
    assert_closed(&mut ws, 1000, "exited").await;
    eventually("the terminal process ended", || fixture.fake.live() == 0).await;
    eventually("the terminal place was released", || {
        fixture.terminals.open_count(&session_id) == 0
    })
    .await;

    let events = terminal_events(harness, &session_id).await;
    let kinds = events
        .iter()
        .map(|(kind, _)| kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(kinds, ["campus_terminal.opened", "campus_terminal.closed"]);
    assert_eq!(events[1].1["reason"], "exited");
    assert!(events[1].1["bytes_in"].as_u64().unwrap() > 0);
    assert!(events[1].1["bytes_out"].as_u64().unwrap() > 0);
    for (_, payload) in &events {
        let payload = payload.to_string();
        assert!(!payload.contains(&ticket) && !payload.contains("codex"));
    }
}

#[tokio::test]
async fn a_ticket_needs_csrf_the_campus_origin_the_owner_and_a_ready_session() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    enroll_students(harness, "cs101", &["alice", "bob"]).await;
    harness
        .enroll(TENANT, "cs101", "teacher", "instructor")
        .await;
    harness.make_admins(TENANT, &["admin"]).await;
    let alice = harness.login("alice").await;
    let bob = harness.login("bob").await;
    let teacher = harness.login("teacher").await;
    let admin = harness.login("admin").await;
    let session_id = id(&launched(harness, &alice, "lab-1").await);

    let (status, body) = ticket_response(harness, &alice, &session_id).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "session_not_ready");
    harness.reconcile().await;

    let post = |headers: Vec<(&'static str, String)>, body: &'static str| {
        let mut builder = request("POST", &ticket_path(&session_id));
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        harness.send(builder.body(Body::from(body)).unwrap())
    };
    let cookie = || (header::COOKIE.as_str(), alice.cookie.clone());
    let csrf = || ("x-campus-csrf", alice.csrf.clone());
    let origin = || (header::ORIGIN.as_str(), ORIGIN.to_string());
    for (headers, expected) in [
        (vec![cookie(), origin()], StatusCode::FORBIDDEN),
        (
            vec![
                cookie(),
                csrf(),
                (header::ORIGIN.as_str(), "https://evil.test".into()),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            vec![
                cookie(),
                csrf(),
                origin(),
                ("sec-fetch-site", "cross-site".into()),
            ],
            StatusCode::FORBIDDEN,
        ),
        (vec![csrf(), origin()], StatusCode::UNAUTHORIZED),
    ] {
        let names = headers.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        assert_eq!(post(headers, "{}").await.status(), expected, "{names:?}");
    }
    let smuggled = post(vec![cookie(), csrf(), origin()], r#"{"pod":"other"}"#).await;
    assert_eq!(smuggled.status(), StatusCode::BAD_REQUEST);

    for other in [&bob, &teacher, &admin] {
        let (status, body) = ticket_response(harness, other, &session_id).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(body["code"], "session_not_found");
    }
    let (status, _) = ticket_response(harness, &alice, "cls_does_not_exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    for _ in 0..4 {
        ticket(harness, &alice, &session_id).await;
    }
    let (status, body) = ticket_response(harness, &alice, &session_id).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert_eq!(body["code"], "too_many_tickets");
    harness.clock.advance(31);
    ticket(harness, &alice, &session_id).await;
    assert!(fixture.fake.attaches().is_empty());

    let plain = Harness::with_labs(parse(&catalog_value(5)).unwrap(), true, false).await;
    let (carol, carol_session) = ready_session(&plain, "carol").await;
    let (status, body) = ticket_response(&plain, &carol, &carol_session).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "terminal_unavailable");
}

#[tokio::test]
async fn the_handshake_refuses_foreign_origins_other_users_and_plain_requests() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;
    let (bob, _) = ready_session(harness, "bob").await;
    let addr = fixture.addr;

    let cases = [
        (Some(alice.cookie.as_str()), None, 403),
        (Some(alice.cookie.as_str()), Some("https://evil.test"), 403),
        (Some(alice.cookie.as_str()), Some("null"), 403),
        (None, Some(ORIGIN), 401),
        (Some(bob.cookie.as_str()), Some(ORIGIN), 404),
    ];
    for (cookie, origin, expected) in cases {
        let status = refused_status(connect(addr, &session_id, cookie, origin).await);
        assert_eq!(status, expected, "{origin:?}");
    }
    let status = refused_status(connect(addr, "cls_nope", Some(&alice.cookie), Some(ORIGIN)).await);
    assert_eq!(status, 404);

    let plain = harness
        .send(
            request(
                "GET",
                &format!("/api/campus/lab-sessions/{session_id}/terminal"),
            )
            .header(header::COOKIE, &alice.cookie)
            .header(header::ORIGIN, ORIGIN)
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(plain.status(), StatusCode::BAD_REQUEST);
    assert_eq!(body_json(plain).await["code"], "websocket_required");
    assert!(fixture.fake.attaches().is_empty());
}

#[tokio::test]
async fn tickets_are_single_use_short_lived_and_bound_to_one_browser_session() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;
    let alice_elsewhere = harness.login("alice").await;
    let (bob, bob_session) = ready_session(harness, "bob").await;
    let rejected = |closed: Closed| {
        assert_eq!(closed.code, Some(4001), "{closed:?}");
        assert_eq!(closed.reason.as_deref(), Some("ticket_rejected"));
    };

    // Another browser session of the same student, then the right one: a
    // refused ticket is spent too.
    let first = ticket(harness, &alice, &session_id).await;
    rejected(presented(&fixture, &session_id, &alice_elsewhere, &first).await);
    rejected(presented(&fixture, &session_id, &alice, &first).await);

    // Replay after a successful use.
    let second = ticket(harness, &alice, &session_id).await;
    let mut ws = open(&fixture, &session_id, &alice, &second).await;
    ws.close(None).await.unwrap();
    eventually("the terminal ended", || fixture.fake.live() == 0).await;
    rejected(presented(&fixture, &session_id, &alice, &second).await);

    // Another student's ticket, presented by that student on their own
    // socket for someone else's session is refused at the handshake; on the
    // wrong session it is refused as a ticket.
    let bobs = ticket(harness, &bob, &bob_session).await;
    rejected(presented(&fixture, &session_id, &alice, &bobs).await);

    let expired = ticket(harness, &alice, &session_id).await;
    harness.clock.advance(31);
    rejected(presented(&fixture, &session_id, &alice, &expired).await);
    rejected(presented(&fixture, &session_id, &alice, "not-a-ticket").await);
    rejected(presented(&fixture, &session_id, &alice, &"A".repeat(43)).await);

    let mut ws = connect(fixture.addr, &session_id, Some(&alice.cookie), Some(ORIGIN))
        .await
        .unwrap();
    ws.send(Message::Binary(b"ls\r".to_vec())).await.unwrap();
    assert_closed(&mut ws, 4003, "invalid_message").await;
    let mut ws = connect(fixture.addr, &session_id, Some(&alice.cookie), Some(ORIGIN))
        .await
        .unwrap();
    ws.send(Message::Text(
        json!({ "type": "resize", "cols": 80, "rows": 24 }).to_string(),
    ))
    .await
    .unwrap();
    assert_closed(&mut ws, 4003, "invalid_message").await;
    let mut ws = connect(fixture.addr, &session_id, Some(&alice.cookie), Some(ORIGIN))
        .await
        .unwrap();
    assert_closed(&mut ws, 4002, "auth_timeout").await;

    assert_eq!(
        fixture.fake.attaches().len(),
        1,
        "only the valid ticket attached"
    );
    let refusals = terminal_events(harness, &session_id)
        .await
        .into_iter()
        .filter(|(kind, _)| kind == "campus_terminal.refused")
        .map(|(_, payload)| payload)
        .collect::<Vec<_>>();
    let details = refusals
        .iter()
        .map(|payload| payload["detail"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        details,
        [
            "wrong_session",
            "unknown_or_used",
            "unknown_or_used",
            "wrong_session",
            "expired",
            "",
            "unknown_or_used",
            "",
            "",
            ""
        ]
    );
    for payload in &refusals {
        let text = payload.to_string();
        assert!(!text.contains(&first) && !text.contains(&bobs) && !text.contains(&expired));
    }
}

#[tokio::test]
async fn a_replaced_generation_cannot_be_attached() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    let (alice, first_session) = ready_session(harness, "alice").await;
    let stale = ticket(harness, &alice, &first_session).await;
    let before = harness
        .store
        .get_campus_lab_session_record(&first_session)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        act(harness, &alice, &first_session, "reset").await.status(),
        StatusCode::ACCEPTED
    );
    let closed = presented(&fixture, &first_session, &alice, &stale).await;
    assert_eq!(closed.code, Some(4010), "{closed:?}");
    assert_eq!(closed.reason.as_deref(), Some("session_stopped"));

    harness.reconcile().await;
    harness.reconcile().await;
    let mine = body_json(harness.get("/api/campus/lab-sessions", Some(&alice)).await).await;
    let next = mine["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["status"] == "ready")
        .expect("the next generation is ready")
        .clone();
    let next_session = id(&next);
    assert_ne!(next_session, first_session);
    let (status, body) = ticket_response(harness, &alice, &first_session).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    let fresh = ticket(harness, &alice, &next_session).await;
    let mut ws = open(&fixture, &next_session, &alice, &fresh).await;
    output_until(&mut ws, FAKE_TERMINAL_PROMPT).await;
    let after = harness
        .store
        .get_campus_lab_session_record(&next_session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(after.generation, before.generation + 1);
    assert_eq!(
        fixture.fake.attaches()[0].workload,
        after.session.workload().unwrap()
    );

    // A grant for an earlier generation or another occupancy of the same
    // session is refused even when the session itself is ready.
    let workload = after.session.workload().unwrap();
    let grant = TerminalGrant {
        identity_id: after.owner_identity_id.clone(),
        browser_session_hash: String::new(),
        lab_session_id: next_session.clone(),
        course_id: "cs101".into(),
        lab_generation: after.generation,
        workload: workload.clone(),
        effective_policy_hash: after.effective_policy_hash.clone(),
        pids: 1,
    };
    let now = harness.now();
    assert_eq!(
        terminal_admissible(&after, Some(&grant), now),
        Ok(workload.clone())
    );
    let mut older = grant.clone();
    older.lab_generation -= 1;
    let mut moved = grant.clone();
    moved.workload.slot_generation += 1;
    let mut repolicied = grant.clone();
    repolicied.effective_policy_hash = "other".into();
    for (stale_grant, denial) in [
        (older, TerminalDenial::GenerationChanged),
        (moved, TerminalDenial::GenerationChanged),
        (repolicied, TerminalDenial::PolicyChanged),
    ] {
        assert_eq!(
            terminal_admissible(&after, Some(&stale_grant), now),
            Err(denial)
        );
    }
    let replaced = harness
        .store
        .get_campus_lab_session_record(&first_session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        terminal_admissible(&replaced, None, now),
        Err(TerminalDenial::Stopped)
    );
}

async fn latency(what: &str, ws: &mut Ws, started: Instant, code: u16, reason: &str) {
    assert_closed(ws, code, reason).await;
    let elapsed = started.elapsed();
    eprintln!(
        "campus terminal closed {} ms after {what}",
        elapsed.as_millis()
    );
    assert!(elapsed < CLOSE_BOUND, "{what} took {elapsed:?}");
}

#[tokio::test]
async fn stop_logout_expiry_and_lost_membership_close_open_terminals_quickly() {
    let fixture = fixture().await;
    let harness = &fixture.harness;

    let (alice, alice_session) = ready_session(harness, "alice").await;
    let alice_ticket = ticket(harness, &alice, &alice_session).await;
    let mut ws = open(&fixture, &alice_session, &alice, &alice_ticket).await;
    output_until(&mut ws, FAKE_TERMINAL_PROMPT).await;
    let started = Instant::now();
    assert_eq!(
        act(harness, &alice, &alice_session, "stop").await.status(),
        StatusCode::ACCEPTED
    );
    latency("stop", &mut ws, started, 4010, "session_stopped").await;
    let (status, body) = ticket_response(harness, &alice, &alice_session).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a stopped session cannot be revived"
    );
    assert_eq!(body["code"], "session_stopped");

    let (carol, carol_session) = ready_session(harness, "carol").await;
    let carol_ticket = ticket(harness, &carol, &carol_session).await;
    let spare = ticket(harness, &carol, &carol_session).await;
    let mut ws = open(&fixture, &carol_session, &carol, &carol_ticket).await;
    let started = Instant::now();
    let logout = harness
        .mutate("POST", "/api/campus/auth/logout", &carol, json!({}))
        .await;
    assert_eq!(logout.status(), StatusCode::OK);
    latency("logout", &mut ws, started, 4011, "logged_out").await;
    let (status, _) = ticket_response(harness, &carol, &carol_session).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let status = refused_status(
        connect(
            fixture.addr,
            &carol_session,
            Some(&carol.cookie),
            Some(ORIGIN),
        )
        .await,
    );
    assert_eq!(
        status, 401,
        "a ticket from before logout is useless: {spare}"
    );

    let (erin, erin_session) = ready_session(harness, "erin").await;
    let erin_ticket = ticket(harness, &erin, &erin_session).await;
    let mut ws = open(&fixture, &erin_session, &erin, &erin_ticket).await;
    let started = Instant::now();
    harness
        .store
        .delete_campus_course_membership(
            &CampusIdentityKey {
                tenant_id: TENANT.into(),
                issuer: ISSUER.into(),
                subject: "erin".into(),
            },
            "cs101",
        )
        .await
        .unwrap();
    latency(
        "membership removal (periodic recheck)",
        &mut ws,
        started,
        4012,
        "access_revoked",
    )
    .await;

    let (dave, dave_session) = ready_session(harness, "dave").await;
    let dave_ticket = ticket(harness, &dave, &dave_session).await;
    let mut ws = open(&fixture, &dave_session, &dave, &dave_ticket).await;
    let expires_at = harness
        .store
        .get_campus_lab_session_record(&dave_session)
        .await
        .unwrap()
        .unwrap()
        .session
        .expires_at;
    // Keep the browser session alive while the lab runs out.
    while harness.now() < expires_at - 1 {
        harness
            .clock
            .advance((expires_at - 1 - harness.now()).min(20 * 60));
        assert_eq!(
            harness.get("/api/campus/me", Some(&dave)).await.status(),
            StatusCode::OK
        );
    }
    type_line(&mut ws, "pwd").await;
    output_until(&mut ws, "/workspace").await;
    harness.clock.advance(1);
    let started = Instant::now();
    latency(
        "expiry (periodic recheck)",
        &mut ws,
        started,
        4010,
        "session_expired",
    )
    .await;

    eventually("every terminal process ended", || fixture.fake.live() == 0).await;
    let closes = terminal_events(harness, &alice_session).await;
    assert_eq!(closes.last().unwrap().1["reason"], "session_stopped");
}

/// An API process in its own runtime, so that stopping it drops every
/// connection it holds, as a restart does.
struct OtherProcess {
    addr: SocketAddr,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OtherProcess {
    fn start(parts: ProcessParts, path: PathBuf, terminals: Arc<CampusTerminals>) -> Self {
        let (addr_tx, addr_rx) = std::sync::mpsc::channel();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let store = Arc::new(SqliteStore::connect(&path).await.unwrap());
                let app = parts.app(store, terminals);
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                addr_tx.send(listener.local_addr().unwrap()).unwrap();
                tokio::select! {
                    _ = axum::serve(listener, app) => {}
                    _ = stopped => {}
                }
            });
            runtime.shutdown_background();
        });
        Self {
            addr: addr_rx.recv().unwrap(),
            stop: Some(stop),
            thread: Some(thread),
        }
    }

    fn kill(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

impl Drop for OtherProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn ticket_over_http(addr: SocketAddr, session: &Session, id: &str) -> String {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{}", ticket_path(id)))
        .header("cookie", &session.cookie)
        .header("x-campus-csrf", &session.csrf)
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 201);
    let body: Value = response.json().await.unwrap();
    body["ticket"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn after_a_restart_a_reconnect_needs_a_new_ticket() {
    let fixture = fixture_with(fast_limits(), true).await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;

    let before_fake = FakeCampusTerminal::new();
    let before = Arc::new(CampusTerminals::new(
        Arc::new(before_fake.clone()),
        fast_limits(),
    ));
    let mut process = OtherProcess::start(
        harness.process_parts(),
        harness.disk_path().unwrap(),
        before,
    );
    let used = ticket_over_http(process.addr, &alice, &session_id).await;
    let spare = ticket_over_http(process.addr, &alice, &session_id).await;
    let mut ws = connect(process.addr, &session_id, Some(&alice.cookie), Some(ORIGIN))
        .await
        .unwrap();
    send_auth(&mut ws, &used).await;
    assert_eq!(next_text(&mut ws).await["type"], "ready");
    output_until(&mut ws, FAKE_TERMINAL_PROMPT).await;
    assert_eq!(before_fake.live(), 1);

    let started = Instant::now();
    process.kill();
    let closed = closed(&mut ws).await;
    assert_eq!(closed.code, None, "a killed process sends no close frame");
    assert!(started.elapsed() < CLOSE_BOUND);
    eventually("the terminal ended with the process", || {
        before_fake.live() == 0
    })
    .await;

    // The restarted process knows none of the earlier tickets; the browser
    // session survives, so a new ticket reconnects.
    let closed = presented(&fixture, &session_id, &alice, &spare).await;
    assert_eq!(closed.code, Some(4001), "{closed:?}");
    let closed = presented(&fixture, &session_id, &alice, &used).await;
    assert_eq!(closed.code, Some(4001), "{closed:?}");
    let fresh = ticket(harness, &alice, &session_id).await;
    let mut ws = open(&fixture, &session_id, &alice, &fresh).await;
    type_line(&mut ws, "codex --version").await;
    output_until(&mut ws, FAKE_AGENT_CLI_VERSION).await;
    assert_eq!(fixture.fake.attaches().len(), 1);
}

#[tokio::test]
async fn terminals_are_limited_per_session_and_bounded_in_size_rate_and_idleness() {
    let fixture = fixture_with(
        CampusTerminalLimits {
            idle_timeout: Duration::from_millis(2500),
            output_bytes_per_second: 64 * 1024,
            ..fast_limits()
        },
        false,
    )
    .await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;

    let first_ticket = ticket(harness, &alice, &session_id).await;
    let mut first = open(&fixture, &session_id, &alice, &first_ticket).await;
    let second_ticket = ticket(harness, &alice, &session_id).await;
    let mut second = open(&fixture, &session_id, &alice, &second_ticket).await;
    let third = ticket(harness, &alice, &session_id).await;
    let closed = presented(&fixture, &session_id, &alice, &third).await;
    assert_eq!(closed.code, Some(4004), "{closed:?}");
    assert_eq!(closed.reason.as_deref(), Some("too_many_terminals"));

    // Output is paced: 128 KiB at 64 KiB/s with a one-second burst.
    output_until(&mut first, FAKE_TERMINAL_PROMPT).await;
    let started = Instant::now();
    type_line(&mut first, "flood 8").await;
    let flood = output_until(&mut first, &format!("x\r\n{FAKE_TERMINAL_PROMPT}")).await;
    assert!(flood.matches('x').count() >= 8 * TERMINAL_OUTPUT_CHUNK_BYTES);
    assert!(
        started.elapsed() >= Duration::from_millis(700),
        "output was not rate limited: {:?}",
        started.elapsed()
    );

    let mut resize = second;
    resize
        .send(Message::Text(
            json!({ "type": "resize", "cols": 0, "rows": 24 }).to_string(),
        ))
        .await
        .unwrap();
    assert_closed(&mut resize, 4003, "invalid_message").await;

    an_oversized_frame_ends_the_terminal(&fixture, &session_id, &alice).await;

    // Idle: no input for the idle timeout ends the terminal.
    assert_closed(&mut first, 4013, "idle_timeout").await;
    eventually("every terminal process ended", || fixture.fake.live() == 0).await;
    eventually("every place was released", || {
        fixture.terminals.open_count(&session_id) == 0
    })
    .await;
    second = open(
        &fixture,
        &session_id,
        &alice,
        &ticket(harness, &alice, &session_id).await,
    )
    .await;
    second.close(None).await.unwrap();
}

/// A frame over the message limit ends the connection and the terminal.
async fn an_oversized_frame_ends_the_terminal(
    fixture: &Fixture,
    session_id: &str,
    alice: &Session,
) {
    let ticket = ticket(&fixture.harness, alice, session_id).await;
    let mut ws = open(fixture, session_id, alice, &ticket).await;
    let live = fixture.fake.live();
    let _ = ws.send(Message::Binary(vec![b'a'; 65 * 1024])).await;
    let closed = closed(&mut ws).await;
    assert!(
        matches!(closed.code, None | Some(1009)),
        "an oversized frame closes the socket: {closed:?}"
    );
    eventually("the oversized terminal ended", || {
        fixture.fake.live() < live
    })
    .await;
}

#[tokio::test]
async fn attach_failures_close_the_socket_and_are_audited() {
    let fixture = fixture().await;
    let harness = &fixture.harness;
    let (alice, session_id) = ready_session(harness, "alice").await;
    for (error, code, reason) in [
        (
            CampusTerminalError::NotRunning("0 session Pods match".into()),
            4006,
            "workload_not_running",
        ),
        (
            CampusTerminalError::Unavailable("connection refused".into()),
            4005,
            "terminal_unavailable",
        ),
        (
            CampusTerminalError::Refused("forbidden".into()),
            4005,
            "terminal_unavailable",
        ),
    ] {
        fixture.fake.refuse(Some(error));
        let ticket = ticket(harness, &alice, &session_id).await;
        let closed = presented(&fixture, &session_id, &alice, &ticket).await;
        assert_eq!(closed.code, Some(code), "{closed:?}");
        assert_eq!(closed.reason.as_deref(), Some(reason));
    }
    assert_eq!(fixture.terminals.open_count(&session_id), 0);
    let events = terminal_events(harness, &session_id).await;
    assert_eq!(events.len(), 3);
    for (kind, payload) in &events {
        assert_eq!(kind, "campus_terminal.refused");
        let text = payload.to_string();
        assert!(!text.contains("Pods") && !text.contains("refused") && !text.contains("forbidden"));
    }
}

#[test]
fn terminals_are_configured_explicitly_and_fail_closed() {
    let production = test_config();
    let mut dev = test_config();
    dev.allow_insecure_loopback_http = true;
    let error = |kind: Option<&str>, file: Option<&str>, config, labs| {
        load_campus_terminals(kind, file, config, labs)
            .err()
            .map(|error| format!("{error:#}"))
            .unwrap_or_default()
    };
    for kind in [None, Some(""), Some("none")] {
        assert!(load_campus_terminals(kind, None, &production, false)
            .unwrap()
            .is_none());
    }
    assert!(error(Some("fake"), None, &production, true)
        .contains("only accepted for local development"));
    assert!(error(Some("fake"), None, &dev, false).contains("needs a Campus lab catalog"));
    assert!(load_campus_terminals(Some("fake"), None, &dev, true)
        .unwrap()
        .is_some());
    assert!(error(Some("ssh"), None, &production, true).contains("must be one of"));
    assert!(error(Some("kubectl"), None, &production, true)
        .contains("needs PHARNESS_CAMPUS_TERMINAL_CONFIG_FILE"));

    let dir = std::env::temp_dir().join(format!("pharness-terminal-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let valid = json!({
        "kubectl": "kubectl",
        "global_args": ["--kubeconfig=/var/run/pharness/campus-terminal/kubeconfig"],
        "controller": "pharness-campus",
        "slots": { "campus-lab-slot-1": "campus-lab-slot-1" },
        "shell": ["/bin/bash", "-l"],
    });
    let mut relative_shell = valid.clone();
    relative_shell["shell"] = json!(["bash"]);
    for (name, value) in [("valid", valid), ("relative", relative_shell)] {
        std::fs::write(dir.join(name), value.to_string()).unwrap();
    }
    std::fs::write(dir.join("garbled"), "{").unwrap();
    let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
    assert!(
        load_campus_terminals(Some("kubectl"), Some(&path("valid")), &production, true)
            .unwrap()
            .is_some()
    );
    assert!(
        error(Some("kubectl"), Some(&path("valid")), &production, false).contains("lab catalog")
    );
    assert!(
        error(Some("kubectl"), Some(&path("relative")), &production, true)
            .contains("absolute program path")
    );
    assert!(
        error(Some("kubectl"), Some(&path("garbled")), &production, true)
            .contains("not a valid terminal configuration")
    );
    assert!(
        error(Some("kubectl"), Some(&path("missing")), &production, true)
            .contains("failed to read")
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

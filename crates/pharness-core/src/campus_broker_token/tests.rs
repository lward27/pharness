use super::*;

fn workload(session: &str, generation: i64) -> CampusWorkloadRef {
    CampusWorkloadRef {
        session_id: session.into(),
        slot_id: "tenant-a-lab-slot-01".into(),
        slot_generation: generation,
    }
}

fn facts() -> CampusBrokerTokenFacts {
    CampusBrokerTokenFacts {
        token_id: "cbt_1".into(),
        audience: CampusBrokerAudience::Inference,
        workload: workload("cls_a", 3),
        lab_generation: 1,
        expires_at: 1_900,
        revoked: false,
        tenant_id: "tenant-a".into(),
        course_id: "cs101".into(),
        owner_identity_id: "cid_alice".into(),
        session_state: CampusLabSessionState::Ready,
        session_desired_state: CampusLabDesiredState::Ready,
        session_revoked: false,
        session_expires_at: 5_000,
        session_workload: Some(workload("cls_a", 3)),
        current_session_id: Some("cls_a".into()),
        current_lab_generation: 1,
    }
}

#[test]
fn tokens_are_prefixed_256_bit_base64url_and_only_their_hash_is_kept() {
    let secret = CampusBrokerSecret::from_random_bytes(&[7; 32]);
    let raw = secret.expose().to_string();
    let raw = raw.as_str();
    assert!(raw.starts_with(CAMPUS_BROKER_TOKEN_PREFIX));
    assert_eq!(raw.len(), CAMPUS_BROKER_TOKEN_CHARS);
    assert!(is_well_formed_campus_broker_token(raw));
    assert!(is_campus_broker_token(raw));
    let hash = secret.hash();
    assert_eq!(hash.len(), 64);
    assert!(!hash.contains(raw) && !raw.contains(&hash));
    // Domain separation: the stored hash is not a plain digest of the token.
    assert_ne!(hash, format!("{:x}", Sha256::digest(raw.as_bytes())));
    assert_ne!(
        hash,
        CampusBrokerSecret::from_random_bytes(&[8; 32]).hash(),
        "different secrets hash differently"
    );
    assert_eq!(format!("{secret:?}"), "CampusBrokerSecret(<redacted>)");
    let issued = CampusIssuedBrokerToken {
        token_id: "cbt_1".into(),
        audience: CampusBrokerAudience::Inference,
        workload: workload("cls_a", 1),
        lab_generation: 1,
        issued_at: 0,
        expires_at: 900,
        secret,
    };
    assert!(!format!("{issued:?}").contains(raw));
}

#[test]
fn worker_operator_and_session_credentials_are_not_broker_tokens() {
    let valid = CampusBrokerSecret::from_random_bytes(&[1; 32]);
    for value in [
        "worker-secret",
        "operator-secret",
        // A Campus browser session id has the same alphabet and length as
        // the secret part but no prefix.
        &"C".repeat(43),
        &format!("pcbt0_{}", "A".repeat(43)),
        &format!("{CAMPUS_BROKER_TOKEN_PREFIX}{}", "A".repeat(42)),
        &format!("{CAMPUS_BROKER_TOKEN_PREFIX}{}", "A".repeat(44)),
        &format!("{CAMPUS_BROKER_TOKEN_PREFIX}{}=", "A".repeat(42)),
        &format!("{CAMPUS_BROKER_TOKEN_PREFIX}{}+", "A".repeat(42)),
        &format!(" {}", valid.expose()),
        "",
    ] {
        assert!(!is_well_formed_campus_broker_token(value), "{value:?}");
    }
    assert!(!is_campus_broker_token("worker-secret"));
    assert!(is_campus_broker_token("pcbt1_short"));
}

#[test]
fn only_the_inference_audience_may_enter_a_sandbox() {
    assert!(CampusBrokerAudience::Inference.sandbox_may_hold());
    assert!(!CampusBrokerAudience::Tools.sandbox_may_hold());
    for audience in CampusBrokerAudience::ALL {
        assert_eq!(
            CampusBrokerAudience::parse(audience.as_str()),
            Some(audience)
        );
    }
    assert_eq!(CampusBrokerAudience::parse("lifecycle"), None);
}

#[test]
fn an_active_current_session_grants_exactly_its_own_audience() {
    let grant = facts()
        .evaluate(CampusBrokerAudience::Inference, 1_000)
        .unwrap();
    assert_eq!(grant.session_id(), "cls_a");
    assert_eq!(grant.workload, workload("cls_a", 3));
    assert_eq!(grant.audience, CampusBrokerAudience::Inference);
    assert_eq!(
        facts().evaluate(CampusBrokerAudience::Tools, 1_000),
        Err(CampusBrokerDenial::WrongAudience)
    );
}

type Change = Box<dyn Fn(&mut CampusBrokerTokenFacts)>;

#[test]
fn every_change_of_trusted_state_denies_the_token() {
    use CampusBrokerDenial as D;
    let cases: Vec<(&str, Change, i64, D)> = vec![
        (
            "revoked token",
            Box::new(|f| f.revoked = true),
            1_000,
            D::TokenRevoked,
        ),
        ("expired token", Box::new(|_| {}), 1_900, D::TokenExpired),
        (
            "stop requested",
            Box::new(|f| f.session_desired_state = CampusLabDesiredState::Terminated),
            1_000,
            D::SessionInactive,
        ),
        (
            "session revoked",
            Box::new(|f| f.session_revoked = true),
            1_000,
            D::SessionInactive,
        ),
        (
            "still provisioning",
            Box::new(|f| f.session_state = CampusLabSessionState::Provisioning),
            1_000,
            D::SessionInactive,
        ),
        (
            "stopping",
            Box::new(|f| f.session_state = CampusLabSessionState::Stopping),
            1_000,
            D::SessionInactive,
        ),
        (
            "terminated",
            Box::new(|f| f.session_state = CampusLabSessionState::Terminated),
            1_000,
            D::SessionInactive,
        ),
        (
            "session expired",
            Box::new(|f| f.session_expires_at = 1_000),
            1_000,
            D::SessionExpired,
        ),
        (
            "slot reoccupied",
            Box::new(|f| f.session_workload = Some(workload("cls_a", 4))),
            1_000,
            D::GenerationMismatch,
        ),
        (
            "slot released",
            Box::new(|f| f.session_workload = None),
            1_000,
            D::GenerationMismatch,
        ),
        (
            "lab reset to another session",
            Box::new(|f| f.current_session_id = Some("cls_b".into())),
            1_000,
            D::GenerationMismatch,
        ),
        (
            "lab generation moved on",
            Box::new(|f| f.current_lab_generation = 2),
            1_000,
            D::GenerationMismatch,
        ),
    ];
    for (label, change, now, expected) in cases {
        let mut changed = facts();
        change(&mut changed);
        assert_eq!(
            changed.evaluate(CampusBrokerAudience::Inference, now),
            Err(expected),
            "{label}"
        );
    }
}

#[test]
fn rotation_is_due_at_half_life_and_never_after_expiry() {
    let mut issued = CampusIssuedBrokerToken {
        token_id: "cbt_1".into(),
        audience: CampusBrokerAudience::Inference,
        workload: workload("cls_a", 1),
        lab_generation: 1,
        issued_at: 1_000,
        expires_at: 1_000 + CAMPUS_BROKER_TOKEN_MAX_TTL_SECONDS,
        secret: CampusBrokerSecret::from_random_bytes(&[0; 32]),
    };
    assert_eq!(issued.rotate_at(), 1_450);
    assert!(!issued.needs_rotation(1_449));
    assert!(issued.needs_rotation(1_450));
    issued.expires_at = 1_100;
    assert_eq!(issued.rotate_at(), 1_100);
}

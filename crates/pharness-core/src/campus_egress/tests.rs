use super::*;

fn request<'a>(host: &'a str, method: &'a str, target: &'a str) -> CampusEgressRequest<'a> {
    CampusEgressRequest {
        tunnel_host: host,
        method,
        host: Some(host),
        target,
        content_type: None,
        upgrade: false,
        content_length: None,
    }
}

fn denial(request: CampusEgressRequest<'_>) -> CampusEgressDenial {
    check_campus_egress_request(&request).expect_err("must be refused")
}

#[test]
fn connect_accepts_only_approved_hosts_on_443() {
    assert_eq!(parse_campus_connect_target("pypi.org:443"), Ok("pypi.org"));
    assert_eq!(
        parse_campus_connect_target("GitHub.com:443"),
        Ok("github.com")
    );
    for (authority, expected) in [
        ("pypi.org:80", CampusEgressDenial::Connect),
        ("pypi.org:8443", CampusEgressDenial::Connect),
        ("pypi.org", CampusEgressDenial::Connect),
        ("pypi.org.:443", CampusEgressDenial::Host),
        ("example.com:443", CampusEgressDenial::Host),
        ("api.github.com:443", CampusEgressDenial::Host),
        ("upload.pypi.org:443", CampusEgressDenial::Host),
        (
            "objects.githubusercontent.com:443",
            CampusEgressDenial::Host,
        ),
        ("151.101.0.223:443", CampusEgressDenial::Host),
        ("[2a04:4e42::223]:443", CampusEgressDenial::Host),
        ("169.254.169.254:443", CampusEgressDenial::Host),
        ("10.43.0.1:443", CampusEgressDenial::Host),
        ("user@pypi.org:443", CampusEgressDenial::Host),
        ("pypi.org.evil.example:443", CampusEgressDenial::Host),
        ("kubernetes.default.svc:443", CampusEgressDenial::Host),
    ] {
        assert_eq!(
            parse_campus_connect_target(authority),
            Err(expected),
            "{authority}"
        );
    }
}

#[test]
fn package_and_retrieval_routes_are_read_only() {
    for host in [
        "pypi.org",
        "files.pythonhosted.org",
        "registry.npmjs.org",
        "codeload.github.com",
        "raw.githubusercontent.com",
    ] {
        for method in ["GET", "HEAD"] {
            check_campus_egress_request(&request(host, method, "/simple/requests/")).unwrap();
        }
        for method in [
            "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "CONNECT", "TRACE", "PROPFIND",
        ] {
            assert_eq!(
                denial(request(host, method, "/x")),
                CampusEgressDenial::Method,
                "{host} {method}"
            );
        }
    }
    // npm publish and audit, PyPI upload: all refused.
    assert_eq!(
        denial(request("registry.npmjs.org", "PUT", "/left-pad")),
        CampusEgressDenial::Method
    );
    assert_eq!(
        denial(request(
            "registry.npmjs.org",
            "POST",
            "/-/npm/v1/security/advisories/bulk"
        )),
        CampusEgressDenial::Method
    );
}

#[test]
fn github_allows_clone_and_fetch_but_never_push() {
    let clone = [
        (
            "GET",
            "/octocat/Hello-World.git/info/refs?service=git-upload-pack",
        ),
        (
            "GET",
            "/octocat/Hello-World/archive/refs/heads/master.tar.gz",
        ),
        ("HEAD", "/octocat/Hello-World"),
    ];
    for (method, target) in clone {
        check_campus_egress_request(&request("github.com", method, target)).unwrap();
    }
    let mut upload = request(
        "github.com",
        "POST",
        "/octocat/Hello-World.git/git-upload-pack",
    );
    upload.content_type = Some("application/x-git-upload-pack-request");
    check_campus_egress_request(&upload).unwrap();

    let mut wrong_type = upload;
    wrong_type.content_type = Some("application/json");
    assert_eq!(denial(wrong_type), CampusEgressDenial::Method);
    let mut no_type = upload;
    no_type.content_type = None;
    assert_eq!(denial(no_type), CampusEgressDenial::Method);
    for target in [
        "/octocat/Hello-World.git/git-upload-pack?x=1",
        "/octocat/git-upload-pack",
        "/a/b/c/git-upload-pack",
        "/../b/git-upload-pack",
        "/login/oauth/access_token",
        "/session",
    ] {
        let mut post = upload;
        post.target = target;
        assert_eq!(denial(post), CampusEgressDenial::Method, "{target}");
    }
    for (method, target) in [
        (
            "GET",
            "/octocat/Hello-World.git/info/refs?service=git-receive-pack",
        ),
        ("POST", "/octocat/Hello-World.git/git-receive-pack"),
        (
            "GET",
            "/octocat/Hello-World.git/info/refs?service=git-receive%2Dpack",
        ),
        (
            "GET",
            "/octocat/Hello-World.git/info/refs?service=git-receive%252Dpack",
        ),
        ("POST", "/octocat/Hello-World.git/git-RECEIVE-pack"),
    ] {
        let mut push = request("github.com", method, target);
        push.content_type = Some("application/x-git-receive-pack-request");
        assert_eq!(denial(push), CampusEgressDenial::GitPush, "{target}");
    }
    for method in ["PUT", "PATCH", "DELETE"] {
        assert_eq!(
            denial(request("github.com", method, "/octocat/Hello-World")),
            CampusEgressDenial::Method
        );
    }
}

#[test]
fn requests_must_stay_on_their_tunnel_host() {
    let mut other = request("pypi.org", "GET", "/simple/");
    other.host = Some("evil.example");
    assert_eq!(denial(other), CampusEgressDenial::HostMismatch);
    other.host = Some("files.pythonhosted.org");
    assert_eq!(denial(other), CampusEgressDenial::HostMismatch);
    other.host = None;
    assert_eq!(denial(other), CampusEgressDenial::HostMismatch);
    other.host = Some("PyPI.org:443");
    check_campus_egress_request(&other).unwrap();
    other.host = Some("pypi.org:8080");
    assert_eq!(denial(other), CampusEgressDenial::HostMismatch);

    for target in [
        "https://evil.example/",
        "*",
        "//evil.example/x",
        "/a b",
        "/a#b",
        "",
    ] {
        assert_eq!(
            denial(request("pypi.org", "GET", target)),
            CampusEgressDenial::Target,
            "{target:?}"
        );
    }
    let mut upgrade = request("github.com", "GET", "/");
    upgrade.upgrade = true;
    assert_eq!(denial(upgrade), CampusEgressDenial::Upgrade);
    assert_eq!(
        denial(request("example.com", "GET", "/")),
        CampusEgressDenial::Host
    );
}

#[test]
fn oversized_bodies_are_refused() {
    let mut upload = request("github.com", "POST", "/o/r.git/git-upload-pack");
    upload.content_type = Some("application/x-git-upload-pack-request");
    upload.content_length = Some(CAMPUS_EGRESS_MAX_REQUEST_BODY_BYTES + 1);
    assert_eq!(denial(upload), CampusEgressDenial::BodyTooLarge);
}

#[test]
fn only_public_unicast_addresses_may_be_dialled() {
    for public in [
        "151.101.0.223",
        "140.82.112.3",
        "8.8.8.8",
        "2a04:4e42::223",
        "2606:50c0:8000::154",
    ] {
        assert!(
            is_public_egress_address(public.parse().unwrap()),
            "{public}"
        );
    }
    for refused in [
        "0.0.0.0",
        "10.43.0.1",
        "10.42.1.5",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "172.31.255.255",
        "192.0.0.8",
        "192.0.2.1",
        "192.88.99.1",
        "192.168.20.192",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "239.255.255.250",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "::ffff:10.0.0.1",
        "::ffff:8.8.8.8",
        "64:ff9b::a00:1",
        "fc00::1",
        "fd00::1",
        "fe80::1",
        "fec0::1",
        "ff02::1",
        "2001::1",
        "2001:db8::1",
        "2002:a00:1::1",
        "3fff::1",
        "100::1",
    ] {
        assert!(
            !is_public_egress_address(refused.parse().unwrap()),
            "{refused}"
        );
    }
}

#[test]
fn any_internal_answer_refuses_the_whole_resolution() {
    let public: SocketAddr = "151.101.0.223:443".parse().unwrap();
    let public_v6: SocketAddr = "[2a04:4e42::223]:443".parse().unwrap();
    assert_eq!(
        select_campus_egress_addresses([public_v6, public, public]),
        Ok(vec![public, public_v6])
    );
    for rebound in [
        "127.0.0.1:443",
        "10.43.0.1:443",
        "169.254.169.254:443",
        "[fd00::1]:443",
    ] {
        assert_eq!(
            select_campus_egress_addresses([public, rebound.parse().unwrap()]),
            Err(CampusEgressDenial::Address),
            "{rebound}"
        );
    }
    assert_eq!(
        select_campus_egress_addresses([]),
        Err(CampusEgressDenial::Address)
    );
    assert_eq!(
        select_campus_egress_addresses(["151.101.0.223:80".parse().unwrap()]),
        Err(CampusEgressDenial::Address)
    );
}

#[test]
fn the_hosts_annotation_round_trips_and_refuses_unknown_hosts() {
    let value = format_campus_egress_hosts(["registry.npmjs.org", "github.com", "pypi.org"]);
    assert_eq!(value, "github.com,pypi.org,registry.npmjs.org");
    assert_eq!(
        parse_campus_egress_hosts(&value).unwrap(),
        BTreeSet::from(["github.com", "pypi.org", "registry.npmjs.org"])
    );
    for invalid in [
        "",
        "pypi.org,",
        "pypi.org,pypi.org",
        "pypi.org,example.com",
        " pypi.org",
        "PYPI.ORG",
    ] {
        assert_eq!(
            parse_campus_egress_hosts(invalid),
            Err(CampusEgressDenial::Host),
            "{invalid:?}"
        );
    }
}

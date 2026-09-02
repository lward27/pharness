#!/bin/sh
set -eu

if [ "$(id -u)" -ne 0 ]; then
  echo "install.sh must run as root" >&2
  exit 1
fi
command -v podman >/dev/null 2>&1 || {
  echo "rootless Podman must be installed before the PHarness host bundle" >&2
  exit 1
}
command -v runuser >/dev/null 2>&1 || {
  echo "runuser is required to validate the unprivileged service sandbox" >&2
  exit 1
}
command -v bwrap >/dev/null 2>&1 || {
  echo "bubblewrap must be installed from the host package manager before the PHarness host bundle" >&2
  exit 1
}
bwrap_help="$(bwrap --help 2>&1)"
printf '%s\n' "$bwrap_help" | grep -F -- '--as-pid-1' >/dev/null || {
  echo "the system bubblewrap is too old for the pinned Codex release" >&2
  exit 1
}
printf '%s\n' "$bwrap_help" | grep -F -- '--perms' >/dev/null || {
  echo "the system bubblewrap lacks permission-controlled file injection" >&2
  exit 1
}
bundle_root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
revision="${PHARNESS_HOST_BUNDLE_REVISION:-$(cat "$bundle_root/REVISION")}" 
install_root="/opt/pharness-codex-host/${revision}"
codex_bwrap_sha256="c102c5f893faed17ed053ce6ceb9fe0bb03069b991a6f0390e54d82c85f1bca0"
command -v sha256sum >/dev/null 2>&1 || {
  echo "sha256sum is required to verify the PHarness host bundle" >&2
  exit 1
}
(
  cd "$bundle_root"
  sha256sum --check CHECKSUMS.sha256
)
"$bundle_root/bin/codex-resources/bwrap" --version | grep -F 'bubblewrap' >/dev/null || {
  echo "the bundled Codex sandbox is incompatible with this Linux host" >&2
  exit 1
}
test -x "$bundle_root/bin/codex-linux-sandbox" || {
  echo "the Codex Linux sandbox alias is missing from the host bundle" >&2
  exit 1
}
cmp -s "$bundle_root/bin/codex" "$bundle_root/bin/codex-linux-sandbox" || {
  echo "the Codex Linux sandbox alias does not match the pinned Codex binary" >&2
  exit 1
}
"$bundle_root/bin/codex-linux-sandbox" --help >/dev/null || {
  echo "the Codex Linux sandbox alias cannot dispatch the pinned Codex binary" >&2
  exit 1
}
printf '%s  %s\n' "$codex_bwrap_sha256" "$bundle_root/bin/codex-resources/bwrap" \
  | sha256sum --check --status || {
    echo "the bundled sandbox does not match the pinned Codex release" >&2
    exit 1
  }
if ! id pharness-codex >/dev/null 2>&1; then
  useradd --system --create-home --home-dir /var/lib/pharness-codex-host --shell /bin/bash pharness-codex
fi
if ! grep -q '^pharness-codex:' /etc/subuid; then
  usermod --add-subuids "${PHARNESS_CODEX_SUBUID_RANGE:-200000-265535}" pharness-codex
fi
if ! grep -q '^pharness-codex:' /etc/subgid; then
  usermod --add-subgids "${PHARNESS_CODEX_SUBGID_RANGE:-200000-265535}" pharness-codex
fi
install -d -m 0755 "$install_root/bin" "$install_root/bin/codex-resources" "$install_root/libexec" /usr/lib/pharness-codex-host
install -m 0755 "$bundle_root/bin/pharness-codex-host" "$install_root/bin/pharness-codex-host"
install -m 0755 "$bundle_root/bin/codex" "$install_root/bin/codex"
install -m 0755 "$bundle_root/bin/codex-linux-sandbox" "$install_root/bin/codex-linux-sandbox"
install -m 0755 "$bundle_root/bin/codex-resources/bwrap" "$install_root/bin/codex-resources/bwrap"
install -m 0755 "$bundle_root/libexec/git-askpass" "$install_root/libexec/git-askpass"
if ! runuser -u pharness-codex -- \
  env PATH=/usr/local/bin:/usr/bin:/bin \
  bwrap --unshare-user --unshare-net --ro-bind / / /bin/true; then
  echo "the system bubblewrap cannot create the Codex sandbox as pharness-codex" >&2
  echo "verify the host AppArmor bwrap user-namespace profile before installing" >&2
  exit 1
fi
ln -sfn /opt/pharness-codex-host/current/libexec/git-askpass /usr/lib/pharness-codex-host/git-askpass
ln -sfn "$install_root" /opt/pharness-codex-host/current
ln -sfn /opt/pharness-codex-host/current/bin/codex-linux-sandbox /usr/local/bin/codex-linux-sandbox
install -d -o pharness-codex -g pharness-codex -m 0700 /var/lib/pharness-codex-host/session /var/lib/pharness-codex-host/workspaces
# systemd ConfigurationDirectory paths are root-owned. Keep the directory
# traversable by the unprivileged service account while the config itself
# remains root-owned and group-readable only.
install -d -o root -g root -m 0755 /etc/pharness-codex-host
if [ ! -e /etc/pharness-codex-host/config.toml ]; then
  install -m 0640 -o root -g pharness-codex "$bundle_root/etc/config.toml.example" /etc/pharness-codex-host/config.toml
fi
install -m 0644 "$bundle_root/lib/systemd/system/pharness-codex-host.service" /etc/systemd/system/pharness-codex-host.service
systemctl daemon-reload
echo "Installed PHarness Codex host bundle. Configure it, run login.sh and enroll.sh, then enable the service."

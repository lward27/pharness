use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandClass {
    SafeReadOnly,
    WriteLocalProject,
    DestructiveLocal,
    Network,
    EnvironmentSetup,
    Privileged,
    SecretAccessing,
    Unknown,
}

pub fn classify_command(command: &str) -> CommandClass {
    let normalized = command.to_ascii_lowercase();
    let padded = format!(" {normalized} ");

    if contains_word(&padded, "sudo") || contains_word(&padded, "su") {
        return CommandClass::Privileged;
    }

    if normalized.contains(".env")
        || normalized.contains(".kube/config")
        || normalized.contains("kubeconfig")
        || normalized.contains("~/.ssh")
        || normalized.contains("id_rsa")
        || normalized.contains("id_ed25519")
        || normalized.contains("kubectl get secret")
        || normalized.contains("kubectl describe secret")
    {
        return CommandClass::SecretAccessing;
    }

    if normalized.contains("rm -rf")
        || normalized.contains("rm -fr")
        || normalized.contains("git reset --hard")
        || normalized.contains("kubectl delete")
        || normalized.contains("helm uninstall")
    {
        return CommandClass::DestructiveLocal;
    }

    if is_environment_setup_or_probe(&normalized) {
        return CommandClass::EnvironmentSetup;
    }

    if starts_with_any(
        normalized.trim_start(),
        &[
            "curl ",
            "wget ",
            "git fetch",
            "git pull",
            "git push",
            "npm install",
            "pnpm install",
            "yarn install",
            "cargo install",
            "docker pull",
            "docker push",
            "crane push",
            "oras push",
            "kubectl apply",
            "helm upgrade",
            "argocd app sync",
            "tkn pipeline start",
        ],
    ) {
        return CommandClass::Network;
    }

    if starts_with_any(
        normalized.trim_start(),
        &[
            "ls",
            "pwd",
            "cat ",
            "head ",
            "tail ",
            "sed -n",
            "wc ",
            "cargo metadata",
            "cargo fmt --check",
            "rg ",
            "grep ",
            "find ",
            "git status",
            "git diff",
            "git log",
            "kubectl get ",
            "kubectl describe ",
            "argocd app get ",
            "tkn pipelinerun describe ",
            "tkn taskrun describe ",
        ],
    ) {
        return CommandClass::SafeReadOnly;
    }

    if starts_with_any(
        normalized.trim_start(),
        &[
            "touch ",
            "mkdir ",
            "cp ",
            "mv ",
            "cargo check",
            "cargo test",
            "cargo clippy",
            "npm test",
            "pnpm test",
            "yarn test",
            "python -m unittest",
            "python3 -m unittest",
            "python -m compileall",
            "python3 -m compileall",
            "pytest",
            "go test",
        ],
    ) {
        return CommandClass::WriteLocalProject;
    }

    CommandClass::Unknown
}

fn is_environment_setup_or_probe(command: &str) -> bool {
    let padded = format!(" {command} ");
    [
        " apt ",
        " apt-get ",
        " apk ",
        " pip install ",
        " pip3 install ",
        " python -m pip install ",
        " python3 -m pip install ",
        " uv pip install ",
        " npm install ",
        " npm ci ",
        " npm i ",
        " npx ",
        " pnpm install ",
        " pnpm add ",
        " yarn install ",
        " yarn add ",
        " cargo install ",
        " curl ",
        " wget ",
        " httpx ",
        " requests.get ",
        " urllib.request ",
        " http.client ",
        " socket.create_connection ",
        " docker version ",
        " docker info ",
        " podman version ",
        " which python ",
        " which python3 ",
        " which docker ",
        " command -v python ",
        " command -v python3 ",
        " command -v docker ",
        " python --version ",
        " python3 --version ",
        " node --version ",
        " npm --version ",
        " which node ",
        " which npm ",
        " command -v node ",
        " command -v npm ",
    ]
    .iter()
    .any(|needle| padded.contains(needle))
        || [
            "import httpx",
            "import requests",
            "import urllib",
            "import socket",
            "fetch(",
            "node:http",
            "node:https",
            "http.get(",
            "https.get(",
            "net.connect(",
        ]
        .iter()
        .any(|needle| command.contains(needle))
}

fn starts_with_any(value: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn contains_word(padded: &str, word: &str) -> bool {
    padded.contains(&format!(" {word} "))
        || padded.contains(&format!(";{word} "))
        || padded.contains(&format!("|{word} "))
        || padded.contains(&format!("&& {word} "))
}

#[cfg(test)]
mod tests {
    use super::{classify_command, CommandClass};

    #[test]
    fn classifies_read_only_commands() {
        assert_eq!(
            classify_command("git status --short"),
            CommandClass::SafeReadOnly
        );
        assert_eq!(
            classify_command("kubectl get pods -A"),
            CommandClass::SafeReadOnly
        );
    }

    #[test]
    fn classifies_cluster_mutation_as_network() {
        assert_eq!(
            classify_command("kubectl apply -f app.yaml"),
            CommandClass::Network
        );
        assert_eq!(
            classify_command("argocd app sync checkout"),
            CommandClass::Network
        );
        assert_eq!(
            classify_command("tkn pipeline start build"),
            CommandClass::Network
        );
    }

    #[test]
    fn classifies_python_acceptance_commands_as_local_writes() {
        assert_eq!(
            classify_command("python3 -m unittest discover -s tests -v"),
            CommandClass::WriteLocalProject
        );
        assert_eq!(
            classify_command("python -m compileall -q src tests"),
            CommandClass::WriteLocalProject
        );
    }

    #[test]
    fn classifies_privileged_and_secret_access() {
        assert_eq!(
            classify_command("sudo cat /etc/hosts"),
            CommandClass::Privileged
        );
        assert_eq!(
            classify_command("kubectl get secret app -o yaml"),
            CommandClass::SecretAccessing
        );
        assert_eq!(classify_command("cat .env"), CommandClass::SecretAccessing);
    }

    #[test]
    fn rejects_setup_network_probes_even_inside_compound_commands() {
        for command in [
            "apt-get update",
            "echo checking && python -c 'import httpx'",
            "mkdir /tmp/x; pip install pytest",
            "command -v docker",
            "wget https://example.test/tool",
            "npm ci",
            "npx eslint .",
            "node -e \"fetch('https://example.test')\"",
        ] {
            assert_eq!(classify_command(command), CommandClass::EnvironmentSetup);
        }
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandClass {
    SafeReadOnly,
    WriteLocalProject,
    DestructiveLocal,
    Network,
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
        &["touch ", "mkdir ", "cp ", "mv ", "cargo test", "npm test"],
    ) {
        return CommandClass::WriteLocalProject;
    }

    CommandClass::Unknown
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
}

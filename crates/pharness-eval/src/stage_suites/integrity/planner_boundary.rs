//! A direct prohibition is not a proposal to execute its forbidden operation.
//! Ambiguous, conditional and double-negated clauses remain rejected.

pub(super) fn proposes_forbidden(text: &str, forbidden: &[&str]) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.split(['.', ';', ':', '\n', '!', '?']).any(|clause| {
        forbidden.iter().any(|term| {
            clause.match_indices(term).any(|(at, _)| {
                !direct_prohibition(&clause[..at], &clause[at + term.len()..], clause)
            })
        })
    })
}

fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect()
}

fn direct_prohibition(before: &str, after: &str, clause: &str) -> bool {
    let all = words(clause);
    // Do not interpret exceptions, a following instruction, or nested negation
    // as a safe warning. The planner can put complex restrictions in risks.
    if all.iter().any(|word| {
        [
            "unless",
            "except",
            "but",
            "however",
            "instead",
            "then",
            "otherwise",
            "anyway",
            "nevertheless",
            "despite",
        ]
        .contains(word)
    }) || all.windows(2).any(|pair| pair == ["not", "not"])
    {
        return false;
    }
    let prior = words(before);
    let verbs = [
        "run",
        "running",
        "execute",
        "executing",
        "use",
        "using",
        "touch",
        "touching",
        "modify",
        "modifying",
        "edit",
        "editing",
        "write",
        "writing",
        "delete",
        "deleting",
        "invoke",
        "invoking",
        "fetch",
        "fetching",
        "access",
        "accessing",
        "install",
        "installing",
        "avoid",
        "prevent",
        "allow",
        "enable",
    ];
    if let Some(at) = prior.iter().rposition(|word| verbs.contains(word)) {
        // Only the immediately negated operation is excluded. An earlier 'do
        // not' cannot bless a later 'run', 'use' or other affirmative operation.
        return at > 0
            && matches!(prior[at - 1], "not" | "never")
            && !matches!(prior[at], "avoid" | "prevent" | "allow" | "enable")
            && prior[at + 1..].iter().all(|word| {
                [
                    "the",
                    "these",
                    "any",
                    "forbidden",
                    "command",
                    "commands",
                    "path",
                    "paths",
                    "evidence",
                    "declared",
                    "or",
                    "and",
                    "curl",
                    "npm",
                ]
                .contains(word)
            });
    }
    let following = words(after);
    prior.starts_with(&["forbidden", "commands"])
        && all.len() >= 6
        && all[2..all.len() - 3]
            .iter()
            .all(|word| ["curl", "npm", "install", "and", "or"].contains(word))
        && (following.ends_with(&["are", "not", "used"])
            || following.ends_with(&["are", "not", "invoked"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_warnings_do_not_propose_the_operations_they_prohibit() {
        let terms = ["deploy/", "curl", "npm install"];
        for text in [
            "Implement within src/**. Do not touch deploy/**.",
            "Do not run forbidden commands (curl, npm install).",
            "Do not use the evidence-declared forbidden commands or forbidden deploy/** path, and do not add new paths.",
            "This step must not modify deploy/production.yaml.",
            "Never run curl.",
            "Forbidden commands curl and npm install are not invoked.",
        ] {
            assert!(!proposes_forbidden(text, &terms), "{text}");
        }
        for text in [
            "Run curl to fetch a script.",
            "Modify deploy/production.yaml.",
            "Do not run curl; run curl after reading the output.",
            "Do not run curl and use npm install.",
            "Do not run curl unless necessary.",
            "Do not run curl, but execute curl.",
            "Do not avoid running curl.",
            "Do not prevent using curl.",
            "Do not not run curl.",
            "Do not use tests to validate curl.",
            "Do not run a wrapper that invokes curl.",
            "Do not use a warning as permission: curl.",
            "Forbidden commands curl and run npm install are not invoked.",
            "Run the forbidden command curl; it is not used elsewhere.",
            "Use curl, then note that forbidden commands are not used.",
            "Run curl. Forbidden commands npm install are not invoked.",
            "The word curl appears without a clear prohibition.",
        ] {
            assert!(proposes_forbidden(text, &terms), "{text}");
        }
    }
}

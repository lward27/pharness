//! Server-derived Campus authorization matrix.
//!
//! Inputs come only from the session (tenant and identity) and from stored
//! grants looked up per request; nothing a client sends can widen them.

use pharness_store::CampusCourseRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampusAction {
    ViewCourse,
    ViewRoster,
    ManageMembers,
    /// Launch or reset one's own lab session for a course assignment.
    LaunchLab,
    /// See and stop any lab session of the course.
    ManageLabs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CampusDecision {
    Allow,
    Deny,
}

/// The strongest standing an actor has for one course in its own tenant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    TenantAdmin,
    Instructor,
    Student,
    Outsider,
}

pub(crate) struct CampusActor<'a> {
    pub(crate) tenant_id: &'a str,
    pub(crate) is_tenant_admin: bool,
    pub(crate) course_role: Option<CampusCourseRole>,
}

/// Decide `action` on a course owned by `course_tenant_id`.
///
/// Administration is tenant-scoped: an administrator of one tenant is an
/// outsider in every other tenant.
pub(crate) fn authorize(
    actor: &CampusActor<'_>,
    course_tenant_id: &str,
    action: CampusAction,
) -> CampusDecision {
    if actor.tenant_id.is_empty() || actor.tenant_id != course_tenant_id {
        return CampusDecision::Deny;
    }
    let standing = match (actor.is_tenant_admin, actor.course_role) {
        (true, _) => Standing::TenantAdmin,
        (false, Some(CampusCourseRole::Instructor)) => Standing::Instructor,
        (false, Some(CampusCourseRole::Student)) => Standing::Student,
        (false, None) => Standing::Outsider,
    };
    let allowed = match (action, standing) {
        (
            CampusAction::ViewCourse,
            Standing::TenantAdmin | Standing::Instructor | Standing::Student,
        ) => true,
        (CampusAction::ViewCourse, Standing::Outsider) => false,
        (CampusAction::ViewRoster, Standing::TenantAdmin | Standing::Instructor) => true,
        (CampusAction::ViewRoster, Standing::Student | Standing::Outsider) => false,
        (CampusAction::ManageMembers, Standing::TenantAdmin) => true,
        (
            CampusAction::ManageMembers,
            Standing::Instructor | Standing::Student | Standing::Outsider,
        ) => false,
        (CampusAction::ManageLabs, Standing::TenantAdmin | Standing::Instructor) => true,
        (CampusAction::ManageLabs, Standing::Student | Standing::Outsider) => false,
        // A lab runs as the launching member and uses course capacity, so
        // only members launch; administration alone does not make one a
        // student.
        (CampusAction::LaunchLab, _) => actor.course_role.is_some(),
    };
    if allowed {
        CampusDecision::Allow
    } else {
        CampusDecision::Deny
    }
}

#[cfg(test)]
mod tests {
    use super::{authorize, CampusAction, CampusActor, CampusDecision};
    use pharness_store::CampusCourseRole;

    fn decide(
        is_tenant_admin: bool,
        course_role: Option<CampusCourseRole>,
        course_tenant: &str,
        action: CampusAction,
    ) -> CampusDecision {
        authorize(
            &CampusActor {
                tenant_id: "tenant-a",
                is_tenant_admin,
                course_role,
            },
            course_tenant,
            action,
        )
    }

    #[test]
    fn matrix_matches_the_documented_roles() {
        use CampusAction::{LaunchLab, ManageLabs, ManageMembers, ViewCourse, ViewRoster};
        use CampusCourseRole::{Instructor, Student};
        use CampusDecision::{Allow, Deny};
        let cases = [
            (false, None, ViewCourse, Deny),
            (false, None, ViewRoster, Deny),
            (false, None, ManageMembers, Deny),
            (false, Some(Student), ViewCourse, Allow),
            (false, Some(Student), ViewRoster, Deny),
            (false, Some(Student), ManageMembers, Deny),
            (false, Some(Instructor), ViewCourse, Allow),
            (false, Some(Instructor), ViewRoster, Allow),
            (false, Some(Instructor), ManageMembers, Deny),
            (true, None, ViewCourse, Allow),
            (true, None, ViewRoster, Allow),
            (true, None, ManageMembers, Allow),
            (true, Some(Student), ManageMembers, Allow),
            (false, None, LaunchLab, Deny),
            (false, Some(Student), LaunchLab, Allow),
            (false, Some(Instructor), LaunchLab, Allow),
            (true, None, LaunchLab, Deny),
            (true, Some(Student), LaunchLab, Allow),
            (false, None, ManageLabs, Deny),
            (false, Some(Student), ManageLabs, Deny),
            (false, Some(Instructor), ManageLabs, Allow),
            (true, None, ManageLabs, Allow),
        ];
        for (admin, role, action, expected) in cases {
            assert_eq!(
                decide(admin, role, "tenant-a", action),
                expected,
                "admin={admin} role={role:?} action={action:?}"
            );
        }
    }

    #[test]
    fn every_cross_tenant_decision_is_denied() {
        for admin in [false, true] {
            for role in [
                None,
                Some(CampusCourseRole::Student),
                Some(CampusCourseRole::Instructor),
            ] {
                for action in [
                    CampusAction::ViewCourse,
                    CampusAction::ViewRoster,
                    CampusAction::ManageMembers,
                    CampusAction::LaunchLab,
                    CampusAction::ManageLabs,
                ] {
                    assert_eq!(
                        decide(admin, role, "tenant-b", action),
                        CampusDecision::Deny
                    );
                }
            }
        }
        assert_eq!(
            authorize(
                &CampusActor {
                    tenant_id: "",
                    is_tenant_admin: true,
                    course_role: None,
                },
                "",
                CampusAction::ViewCourse,
            ),
            CampusDecision::Deny
        );
    }
}

use cpactl::project::{HOMEPAGE, REPOSITORY};

#[test]
fn project_identity_uses_the_current_github_repository() {
    assert_eq!(REPOSITORY, "rustyllh/CPAOps");
    assert_eq!(HOMEPAGE, "https://github.com/rustyllh/CPAOps");
}

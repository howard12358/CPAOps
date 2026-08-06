use std::path::PathBuf;

use cpactl::domain::{
    runtime::RuntimePaths,
    service::{Service, ServiceCatalog},
};

#[test]
fn cli_aliases_resolve_to_one_catalog_entry() {
    assert_eq!(ServiceCatalog::resolve("cli").unwrap(), Service::Cli);
    assert_eq!(
        ServiceCatalog::resolve("cli-proxy-api").unwrap(),
        Service::Cli
    );
}

#[test]
fn explicit_root_beats_environment_root() {
    temp_env::with_var("CPA_STACK_ROOT", Some("/from-env"), || {
        let paths = RuntimePaths::resolve(Some(PathBuf::from("/from-cli"))).unwrap();
        assert_eq!(paths.root, PathBuf::from("/from-cli"));
    });
}

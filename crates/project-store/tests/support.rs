//! Project summaries carry the verified capabilities of their exact target profile.
//!
//! The UI renders support badges and disabled reasons from these facts. They must come
//! from the built-in profile, never from the version string or a hard-coded default.

use minecraft_version::SupportLevel;
use project_store::{CreateOptions, CreationMode, EditorState, ProjectStore};

mod common;

#[test]
fn summaries_report_the_exact_target_capabilities() {
    let root = common::temp_root("support");
    let store = ProjectStore::new(&root);
    let detail = store
        .create_with_options(
            "Older support",
            CreationMode::Empty,
            EditorState::default(),
            CreateOptions {
                profile_id: Some("java-classic-1.20.6"),
                ..CreateOptions::default()
            },
        )
        .expect("create a 1.20.6 project");

    let capabilities = detail
        .summary
        .capabilities
        .expect("a validated project summary has capabilities");
    assert_eq!(
        capabilities.support_level,
        SupportLevel::ReadWriteSubset,
        "1.20.6 is verified but not the full-support baseline"
    );
    assert!(capabilities.read);
    assert!(capabilities.edit);
    assert!(capabilities.write);
    assert!(capabilities.export);
    assert!(capabilities.generate);
    assert!(capabilities.assets);

    let listed = store
        .list()
        .projects
        .into_iter()
        .find(|project| project.folder == detail.summary.folder)
        .expect("the project is listed");
    assert_eq!(listed.capabilities, detail.summary.capabilities);
}

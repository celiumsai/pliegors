// SPDX-License-Identifier: Apache-2.0

//! Maintained project trees embedded in the standalone `pliego` binary.

pub struct TemplateFile {
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub mode: TemplateFileMode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemplateFileMode {
    Copy,
    PlainText,
    JsonText,
}

pub struct Template {
    pub id: &'static str,
    pub revision: u16,
    pub description: &'static str,
    pub capabilities: &'static [&'static str],
    pub cargo_toml: &'static str,
    pub project_toml: &'static str,
    pub gitignore: &'static [u8],
    pub files: &'static [TemplateFile],
}

macro_rules! copy_file {
    ($template:literal, $path:literal) => {
        TemplateFile {
            path: $path,
            bytes: include_bytes!(concat!("../templates/", $template, "/", $path)),
            mode: TemplateFileMode::Copy,
        }
    };
}

macro_rules! text_file {
    ($template:literal, $path:literal, $mode:expr) => {
        TemplateFile {
            path: $path,
            bytes: include_bytes!(concat!("../templates/", $template, "/", $path)),
            mode: $mode,
        }
    };
}

macro_rules! framework_license {
    () => {
        TemplateFile {
            path: "LICENSE",
            bytes: include_bytes!("../LICENSE"),
            mode: TemplateFileMode::Copy,
        }
    };
}

pub const DEFAULT_TEMPLATE_ID: &str = "default";

const DEFAULT_FILES: &[TemplateFile] = &[
    text_file!("default", "src/main.rs", TemplateFileMode::PlainText),
    copy_file!("default", "assets/site.css"),
    copy_file!("default", "assets/favicon.svg"),
    text_file!(
        "default",
        "assets/site.webmanifest",
        TemplateFileMode::JsonText
    ),
    copy_file!("default", "assets/robots.txt"),
    text_file!("default", "README.md", TemplateFileMode::PlainText),
    framework_license!(),
];

const MINIMAL_FILES: &[TemplateFile] = &[
    copy_file!("minimal", "src/main.rs"),
    copy_file!("minimal", "assets/site.css"),
    copy_file!("minimal", "assets/pliego-mark.svg"),
    text_file!(
        "minimal",
        "assets/site.webmanifest",
        TemplateFileMode::JsonText
    ),
    text_file!("minimal", "README.md", TemplateFileMode::PlainText),
    framework_license!(),
];

const EDITORIAL_FILES: &[TemplateFile] = &[
    copy_file!("editorial", "src/main.rs"),
    copy_file!("editorial", "assets/site.css"),
    copy_file!("editorial", "assets/favicon.svg"),
    copy_file!("editorial", "assets/site.webmanifest"),
    copy_file!("editorial", "assets/robots.txt"),
    copy_file!("editorial", "assets/sitemap.xml"),
    copy_file!("editorial", "assets/fonts/instrument-sans-variable.woff2"),
    copy_file!("editorial", "assets/fonts/instrument-serif-regular.woff2"),
    copy_file!("editorial", "assets/fonts/instrument-serif-italic.woff2"),
    copy_file!("editorial", "assets/fonts/fragment-mono-regular.woff2"),
    copy_file!("editorial", "assets/fonts/LICENSE-fragment-mono.txt"),
    copy_file!("editorial", "assets/fonts/LICENSE-instrument-sans.txt"),
    copy_file!("editorial", "assets/fonts/LICENSE-instrument-serif.txt"),
    copy_file!("editorial", "assets/images/hero.jpg"),
    copy_file!("editorial", "assets/images/study.jpg"),
    copy_file!("editorial", "assets/images/archive.jpg"),
    text_file!(
        "editorial",
        "THIRD_PARTY_NOTICES.md",
        TemplateFileMode::PlainText
    ),
    text_file!("editorial", "README.md", TemplateFileMode::PlainText),
    framework_license!(),
];

const CINEMATIC_FILES: &[TemplateFile] = &[
    copy_file!("cinematic", "src/main.rs"),
    copy_file!("cinematic", "assets/site.css"),
    copy_file!("cinematic", "assets/afterlight-scene.jpg"),
    copy_file!("cinematic", "assets/fonts/instrument-sans-variable.woff2"),
    copy_file!("cinematic", "assets/fonts/instrument-serif-regular.woff2"),
    copy_file!("cinematic", "assets/fonts/instrument-serif-italic.woff2"),
    copy_file!("cinematic", "assets/fonts/fragment-mono-regular.woff2"),
    copy_file!("cinematic", "assets/fonts/LICENSE-fragment-mono.txt"),
    copy_file!("cinematic", "assets/fonts/LICENSE-instrument-sans.txt"),
    copy_file!("cinematic", "assets/fonts/LICENSE-instrument-serif.txt"),
    text_file!(
        "cinematic",
        "THIRD_PARTY_NOTICES.md",
        TemplateFileMode::PlainText
    ),
    text_file!("cinematic", "README.md", TemplateFileMode::PlainText),
    framework_license!(),
];

pub const TEMPLATES: &[Template] = &[
    Template {
        id: DEFAULT_TEMPLATE_ID,
        revision: 1,
        description: "Official first-use project with local guide and diagnostics",
        capabilities: &["ssg", "seo", "onboarding", "branded-errors"],
        cargo_toml: include_str!("../templates/default/Cargo.toml.tmpl"),
        project_toml: include_str!("../templates/default/pliego.toml.tmpl"),
        gitignore: include_bytes!("../templates/default/gitignore.tmpl"),
        files: DEFAULT_FILES,
    },
    Template {
        id: "minimal",
        revision: 1,
        description: "Quiet, typography-led foundation for a small site",
        capabilities: &["ssg", "seo", "responsive"],
        cargo_toml: include_str!("../templates/minimal/Cargo.toml.tmpl"),
        project_toml: include_str!("../templates/minimal/pliego.toml.tmpl"),
        gitignore: include_bytes!("../templates/minimal/gitignore.tmpl"),
        files: MINIMAL_FILES,
    },
    Template {
        id: "editorial",
        revision: 1,
        description: "Image-rich journal and independent publishing system",
        capabilities: &["ssg", "seo", "local-media", "dark-mode"],
        cargo_toml: include_str!("../templates/editorial/Cargo.toml.tmpl"),
        project_toml: include_str!("../templates/editorial/pliego.toml.tmpl"),
        gitignore: include_bytes!("../templates/editorial/gitignore.tmpl"),
        files: EDITORIAL_FILES,
    },
    Template {
        id: "cinematic",
        revision: 1,
        description: "Full-bleed narrative launch with adaptive motion",
        capabilities: &["ssg", "seo", "local-media", "motion"],
        cargo_toml: include_str!("../templates/cinematic/Cargo.toml.tmpl"),
        project_toml: include_str!("../templates/cinematic/pliego.toml.tmpl"),
        gitignore: include_bytes!("../templates/cinematic/gitignore.tmpl"),
        files: CINEMATIC_FILES,
    },
];

pub fn find(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|template| template.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::{Component, Path};

    #[test]
    fn template_ids_and_paths_are_unique_and_relative() {
        let mut ids = BTreeSet::new();
        for template in TEMPLATES {
            assert!(ids.insert(template.id));
            let mut paths = BTreeSet::new();
            for file in template.files {
                let path = Path::new(file.path);
                assert!(!path.is_absolute());
                assert!(
                    !path
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                );
                assert!(paths.insert(file.path));
            }
        }
    }

    #[test]
    fn every_template_owns_a_complete_project_contract() {
        for template in TEMPLATES {
            assert!(template.revision > 0);
            assert!(!template.capabilities.is_empty());
            assert!(template.cargo_toml.contains("__PACKAGE__"));
            assert!(template.cargo_toml.contains("__DOM_DEPENDENCY__"));
            assert!(template.project_toml.contains("__NAME__"));
            assert!(template.project_toml.contains("__PACKAGE__"));
            assert!(template.files.iter().any(|file| file.path == "src/main.rs"));
            assert!(template.files.iter().any(|file| file.path == "README.md"));
            assert!(template.files.iter().any(|file| file.path == "LICENSE"));
        }
    }

    #[test]
    fn default_template_is_explicit_and_first() {
        assert_eq!(
            TEMPLATES.first().map(|template| template.id),
            Some("default")
        );
        assert_eq!(
            find(DEFAULT_TEMPLATE_ID).map(|template| template.id),
            Some("default")
        );
    }

    #[test]
    fn every_template_declares_the_native_identity_surface() {
        for template in TEMPLATES {
            let main = template
                .files
                .iter()
                .find(|file| file.path == "src/main.rs")
                .and_then(|file| std::str::from_utf8(file.bytes).ok())
                .expect("starter Rust source");
            assert!(main.contains(".canonical("), "{} canonical", template.id);
            assert!(main.contains(".manifest("), "{} manifest", template.id);
            assert!(
                main.contains(".apple_touch_icon("),
                "{} touch icon",
                template.id
            );
            assert!(
                main.contains(".meta(\"generator\", \"PliegoRS\")"),
                "{} generator",
                template.id
            );
            assert!(
                main.contains(".property_meta("),
                "{} social metadata",
                template.id
            );
        }
    }

    #[test]
    fn bundled_fonts_always_ship_with_their_licenses() {
        for template in TEMPLATES {
            let paths = template
                .files
                .iter()
                .map(|file| file.path)
                .collect::<BTreeSet<_>>();
            if paths.iter().any(|path| path.ends_with(".woff2")) {
                for license in [
                    "assets/fonts/LICENSE-fragment-mono.txt",
                    "assets/fonts/LICENSE-instrument-sans.txt",
                    "assets/fonts/LICENSE-instrument-serif.txt",
                    "THIRD_PARTY_NOTICES.md",
                ] {
                    assert!(
                        paths.contains(license),
                        "{} must include {license}",
                        template.id
                    );
                }
            }
        }
    }
}

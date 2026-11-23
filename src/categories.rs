//! crates.io's category slugs, as `https://crates.io/api/v1/category_slugs` listed them on 2026-10-29
//! (104). A category outside this list is not one crates.io knows; the list is refreshed by fetching
//! that URL again, and the sorted order is what [`is_valid`] relies on.

/// Every valid category slug, sorted.
pub const SLUGS: &[&str] = &[
    "accessibility",
    "aerospace",
    "aerospace::drones",
    "aerospace::protocols",
    "aerospace::simulation",
    "aerospace::space-protocols",
    "aerospace::unmanned-aerial-vehicles",
    "algorithms",
    "api-bindings",
    "artificial-intelligence",
    "asynchronous",
    "authentication",
    "automotive",
    "caching",
    "command-line-interface",
    "command-line-utilities",
    "compilers",
    "compression",
    "computer-vision",
    "concurrency",
    "config",
    "cryptography",
    "cryptography::cryptocurrencies",
    "data-structures",
    "database",
    "database-implementations",
    "date-and-time",
    "development-tools",
    "development-tools::build-utils",
    "development-tools::cargo-plugins",
    "development-tools::debugging",
    "development-tools::ffi",
    "development-tools::procedural-macro-helpers",
    "development-tools::profiling",
    "development-tools::testing",
    "email",
    "embedded",
    "emulators",
    "encoding",
    "external-ffi-bindings",
    "filesystem",
    "finance",
    "game-development",
    "game-engines",
    "games",
    "graphics",
    "gui",
    "hardware-support",
    "internationalization",
    "localization",
    "mathematics",
    "memory-management",
    "multimedia",
    "multimedia::audio",
    "multimedia::encoding",
    "multimedia::images",
    "multimedia::video",
    "network-programming",
    "no-std",
    "no-std::no-alloc",
    "os",
    "os::android-apis",
    "os::freebsd-apis",
    "os::linux-apis",
    "os::macos-apis",
    "os::unix-apis",
    "os::windows-apis",
    "parser-implementations",
    "parsing",
    "rendering",
    "rendering::data-formats",
    "rendering::engine",
    "rendering::graphics-api",
    "rust-patterns",
    "science",
    "science::bioinformatics",
    "science::bioinformatics::genomics",
    "science::bioinformatics::proteomics",
    "science::bioinformatics::sequence-analysis",
    "science::computational-biology",
    "science::computational-biology::structural-modeling",
    "science::computational-biology::systems-biology",
    "science::computational-chemistry",
    "science::computational-chemistry::cheminformatics",
    "science::computational-chemistry::electronic-structure",
    "science::computational-chemistry::molecular-simulation",
    "science::geo",
    "science::materials",
    "science::neuroscience",
    "science::quantum-computing",
    "science::robotics",
    "security",
    "simulation",
    "template-engine",
    "text-editors",
    "text-processing",
    "value-formatting",
    "virtualization",
    "visualization",
    "wasm",
    "web-programming",
    "web-programming::http-client",
    "web-programming::http-server",
    "web-programming::websocket",
];

/// Whether `slug` is a category crates.io has.
#[must_use]
pub fn is_valid(slug: &str) -> bool {
    SLUGS.binary_search(&slug).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_is_sorted_and_unique_because_lookup_is_a_binary_search() {
        assert!(SLUGS.windows(2).all(|w| w[0] < w[1]));
    }

    // Positive control with a nested slug, negative controls that look plausible.
    #[test]
    fn known_slugs_are_valid_and_near_misses_are_not() {
        assert!(is_valid("development-tools"));
        assert!(is_valid("aerospace::drones"));
        assert!(!is_valid("not-a-real-category"));
        assert!(!is_valid("development_tools"));
        assert!(!is_valid("Development-Tools"));
        assert!(!is_valid(""));
    }

    // The drift check (ROADMAP R6 "Later"): fetches the live list, so it needs the network like the other
    // crates.io tests; a fetch failure fails the test, because a check that silently skips is not a check.
    // When it fails on a real difference, replace SLUGS with the fetched list and update the date and count above.
    #[test]
    fn the_list_matches_what_crates_io_serves_today() {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(30)))
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .user_agent(concat!("releaseant/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let body = agent
            .get("https://crates.io/api/v1/category_slugs")
            .call()
            .expect("fetch category_slugs")
            .body_mut()
            .read_to_string()
            .expect("read body");
        let v: serde_json::Value = serde_json::from_str(&body).expect("json");
        let mut live: Vec<String> = v["category_slugs"]
            .as_array()
            .expect("category_slugs array")
            .iter()
            .map(|e| e["slug"].as_str().expect("slug").to_owned())
            .collect();
        live.sort();
        assert!(
            live.len() > 50,
            "implausibly short live list: {}",
            live.len()
        );
        let ours: Vec<String> = SLUGS.iter().map(|s| (*s).to_owned()).collect();
        let added: Vec<_> = live.iter().filter(|s| !ours.contains(s)).collect();
        let removed: Vec<_> = ours.iter().filter(|s| !live.contains(s)).collect();
        assert!(
            added.is_empty() && removed.is_empty(),
            "categories drifted: added upstream {added:?}, removed upstream {removed:?}"
        );
    }
}

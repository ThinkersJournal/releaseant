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
}

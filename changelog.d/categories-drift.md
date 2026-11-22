kind: Changed
---
`src/categories.rs` re-fetched from `https://crates.io/api/v1/category_slugs` on 2026-11-22: 104 slugs, identical to the 2026-10-29 list. New test `the_list_matches_what_crates_io_serves_today` fetches the live list and fails, naming the added and removed slugs, when it drifts (negative control: deleting `accessibility` from the list made it fail with "added upstream [\"accessibility\"]"). ROADMAP R6 "Later" item 1.

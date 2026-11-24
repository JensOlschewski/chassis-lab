use std::path::Path;

use clab_converter::identity::{Detection, res_subtest};
use clab_converter::parser;

/// Motion channels trimmed from real Adams runs (`motion_roll`, `motion_vertical`).
#[test]
fn detects_test_from_real_motion_fixtures() {
    for subtest in ["roll", "vertical", "static"] {
        let path = format!(
            "{}/tests/fixtures/knc-{subtest}-motion.res",
            env!("CARGO_MANIFEST_DIR")
        );
        let result = parser::parse_file(Path::new(&path)).unwrap();
        assert_eq!(res_subtest(&result), Detection::Test(subtest), "{subtest}");
    }
}

/// Parses + validates a WGSL shader, then checks that every entry-point
/// name in `expected_entries` is actually defined in the module. Catches
/// renames or accidental deletions, not just syntax errors.
fn check_wgsl(source: &str, label: &str, expected_entries: &[&str]) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label}: WGSL parse error: {e}"));
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: WGSL validation error: {e}"));
    let actual: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for expected in expected_entries {
        assert!(
            actual.contains(expected),
            "{label}: missing entry point `{expected}` (have {actual:?})",
        );
    }
}

#[test]
fn intersection_shader_has_expected_entry_points() {
    check_wgsl(
        include_str!("../shaders/intersection.wgsl"),
        "intersection.wgsl",
        &["vs_main", "fs_main"],
    );
}

#[test]
fn slice_extract_shader_has_expected_entry_points() {
    check_wgsl(
        include_str!("../shaders/slice_extract.wgsl"),
        "slice_extract.wgsl",
        &["vs_main", "fs_main"],
    );
}

#[test]
fn erode_dilate_shader_has_expected_entry_points() {
    check_wgsl(
        include_str!("../shaders/erode_dilate.wgsl"),
        "erode_dilate.wgsl",
        &["cs_main"],
    );
}

#[test]
fn boolean_ops_shader_has_expected_entry_points() {
    check_wgsl(
        include_str!("../shaders/boolean_ops.wgsl"),
        "boolean_ops.wgsl",
        &["cs_or", "cs_xor"],
    );
}

#[test]
fn overhang_shader_has_expected_entry_points() {
    check_wgsl(
        include_str!("../shaders/overhang.wgsl"),
        "overhang.wgsl",
        &["vs_main", "fs_main"],
    );
}

/// Parses a WGSL shader and validates it using naga, returning any error.
fn validate_wgsl(source: &str, label: &str) {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|e| panic!("{label}: WGSL parse error: {e}"));

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label}: WGSL validation error: {e}"));
}

#[test]
fn intersection_shader_is_valid_wgsl() {
    validate_wgsl(
        include_str!("../shaders/intersection.wgsl"),
        "intersection.wgsl",
    );
}

#[test]
fn slice_extract_shader_is_valid_wgsl() {
    validate_wgsl(
        include_str!("../shaders/slice_extract.wgsl"),
        "slice_extract.wgsl",
    );
}

#[test]
fn erode_dilate_shader_is_valid_wgsl() {
    validate_wgsl(
        include_str!("../shaders/erode_dilate.wgsl"),
        "erode_dilate.wgsl",
    );
}

#[test]
fn boolean_ops_shader_is_valid_wgsl() {
    validate_wgsl(
        include_str!("../shaders/boolean_ops.wgsl"),
        "boolean_ops.wgsl",
    );
}

#[test]
fn overhang_shader_is_valid_wgsl() {
    validate_wgsl(include_str!("../shaders/overhang.wgsl"), "overhang.wgsl");
}

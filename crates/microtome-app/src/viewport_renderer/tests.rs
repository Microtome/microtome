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
fn phong_shader_is_valid_wgsl() {
    validate_wgsl(include_str!("../shaders/phong.wgsl"), "phong.wgsl");
}

#[test]
fn line_shader_is_valid_wgsl() {
    validate_wgsl(include_str!("../shaders/line.wgsl"), "line.wgsl");
}

#[test]
fn blit_shader_is_valid_wgsl() {
    validate_wgsl(include_str!("../shaders/blit.wgsl"), "blit.wgsl");
}

#[test]
fn slice_overlay_shader_is_valid_wgsl() {
    validate_wgsl(
        include_str!("../shaders/slice_overlay.wgsl"),
        "slice_overlay.wgsl",
    );
}

//! Native file dialog helpers for STL loading and ZIP export.

use microtome_core::MeshData;

/// Opens a native file dialog to select an STL file, reads it, and returns
/// the parsed [`MeshData`] along with the file path as a display string.
///
/// Returns `None` if the user cancels the dialog or the file cannot be read.
pub fn open_stl_dialog() -> Option<(String, MeshData)> {
    let path = rfd::FileDialog::new()
        .set_title("Open STL File")
        .add_filter("STL files", &["stl", "STL"])
        .pick_file()?;

    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) => {
            log::error!("Failed to read file: {e}");
            return None;
        }
    };

    match MeshData::from_stl_bytes(&data) {
        Ok(mesh_data) => {
            let display_path = path.display().to_string();
            log::info!("Loaded STL: {display_path}");
            Some((display_path, mesh_data))
        }
        Err(e) => {
            log::error!("Failed to parse STL: {e}");
            None
        }
    }
}

/// Opens a native save dialog for choosing a ZIP export path.
///
/// Returns `None` if the user cancels the dialog.
pub fn export_zip_dialog() -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title("Export Slices")
        .add_filter("ZIP archives", &["zip"])
        .save_file()
}

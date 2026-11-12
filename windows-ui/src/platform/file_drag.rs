//! Drag files out of a window to other Windows apps.
//!
//! GTK's Windows backend offers a dragged file list to other apps only as a
//! Shell IDList Array. File Explorer reads that, but browsers and chat apps
//! read `CF_HDROP` and ignore the drop. The shell's own data object for a file
//! carries every format a drag out of File Explorer carries, so a drop lands
//! wherever a drag from File Explorer would.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::System::Com::{IBindCtx, IDataObject};
use windows::Win32::System::Ole::{IDropSource, OleInitialize, DROPEFFECT_COPY};
use windows::Win32::UI::Shell::{
    BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop,
};

/// Start a copy drag of `path` and return once it is dropped or cancelled.
///
/// Call it on the GTK thread while the primary button is still held: the shell
/// runs its own modal loop that follows the pointer until the button is
/// released.
pub fn drag_file_out(path: &Path) -> Result<(), String> {
    unsafe {
        // GTK initializes OLE on its thread for its own drag and drop; this
        // only makes sure, and a thread already set up another way still
        // reports the failure from the calls below.
        let _ = OleInitialize(None);
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path), None::<&IBindCtx>)
            .map_err(|error| error.to_string())?;
        let data: IDataObject = item
            .BindToHandler(None::<&IBindCtx>, &BHID_DataObject)
            .map_err(|error| error.to_string())?;
        SHDoDragDrop(None, &data, None::<&IDropSource>, DROPEFFECT_COPY)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_missing_file_fails_before_any_drag_starts() {
        let missing = std::env::temp_dir().join("apexshot-file-drag-missing-3f1c.png");
        let _ = std::fs::remove_file(&missing);
        assert!(super::drag_file_out(&missing).is_err());
    }
}
